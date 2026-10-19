//! Utskick av utfärdade kundfakturor (#138): med e-post från panelen Skicka eller direkt efter
//! utfärdandet, eller markerade som lämnade på annat sätt (post, personligen, kundens portal).
//!
//! E-posten är en port, [`Epost`]: webben ger en adapter som skickar med SMTP, och testerna en
//! brevlåda. Ett utskick görs alltid utanför transaktionen och efter att utfärdandet har
//! committats, så ett misslyckat utskick rullar aldrig tillbaka något. Utfallet sparas sedan i
//! ett eget ärende: en rad i `kundfaktura_utskick`, som schemat aldrig låter ändras eller raderas,
//! och en post i behandlingshistoriken med tid, mottagare, vem och resultat (H1). Att skicka är
//! en loggad händelse och inget tillstånd på fakturan.

use std::future::Future;

use jiff::Timestamp;
use jiff::civil::Date;
use maya_domain::utskick::{
    Lämningssätt, förvalt_meddelande, kreditmeddelande, kreditämne, ämne
};
use serde_json::json;
use sqlx::SqliteConnection;

use crate::aktor::{Aktör, Handling};
use crate::atkomst::{self, Ärende, ÅtkomstFel};
use crate::behandlingshistorik::Händelse;
use crate::foretag::{FöretagsId, datum};
use crate::kundfaktura::{self, KundfakturaFel, KundfakturaId, Status, Utfärdad};
use crate::smtp::Bilaga;
use crate::verifikat::{Signatur, signatur};
use crate::{Databas, DatabasFel, Klocka, fakturauppgifter};

/// Ett fakturamejl som porten skickar. Avsändaradressen väljer adaptern.
#[derive(Debug, Clone)]
pub struct Fakturamejl {
    pub till: String,
    /// Företagets svarsadress (`Reply-To`), om den är angiven.
    pub svara_till: Option<String>,
    /// Företagets namn, som avsändarens visningsnamn.
    pub avsändarnamn: String,
    pub ämne: String,
    pub text: String,
    /// Den arkiverade PDF:en, oförändrad.
    pub bilaga: Bilaga,
}

/// Porten mot e-posten. Felet är det som visas och loggas, t.ex. SMTP-serverns svar.
pub trait Epost: Send + Sync {
    fn skicka(
        &self,
        mejl: &Fakturamejl,
        nu: Timestamp,
    ) -> impl Future<Output = Result<(), String>> + Send;
}

/// Det användaren skickar från panelen Skicka.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NyttUtskick {
    pub till: String,
    pub meddelande: String,
}

/// Hur ett utskick gick. Båda loggas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Utfall {
    Skickat,
    /// Felet från e-posten.
    Misslyckat(String),
}

#[derive(Debug, thiserror::Error)]
pub enum UtskickFel {
    #[error("kundfakturan finns inte")]
    FinnsInte,
    /// Bara en utfärdad faktura skickas.
    #[error("fakturan är inte utfärdad")]
    InteUtfärdad,
    /// Mottagaren är ingen e-postadress. Inget skickas och inget loggas.
    #[error("{0}")]
    Mottagare(String),
    #[error(transparent)]
    Åtkomst(#[from] ÅtkomstFel),
}

impl From<sqlx::Error> for UtskickFel {
    fn from(e: sqlx::Error) -> Self {
        UtskickFel::Åtkomst(e.into())
    }
}

impl From<DatabasFel> for UtskickFel {
    fn from(e: DatabasFel) -> Self {
        UtskickFel::Åtkomst(e.into())
    }
}

/// Hur fakturan lämnades.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sätt {
    Epost {
        till: String,
        svarsadress: Option<String>,
    },
    Lämnad(Lämningssätt),
}

/// Ett utskick på fakturan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Utskick {
    /// När och vem.
    pub signatur: Signatur,
    pub sätt: Sätt,
    /// Mejlets text.
    pub meddelande: Option<String>,
    /// Felet när utskicket misslyckades.
    pub fel: Option<String>,
}

/// Det som panelen Skicka förvalt visar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Förslag {
    /// Mottagaren: adressen i det senaste utskicket om det misslyckades, annars kundens adress
    /// i kundregistret (eller den som kopierades in på fakturan).
    pub till: String,
    /// Kundens adress i kundregistret nu, att föreslå när ett utskick har misslyckats.
    pub kundregistret: String,
    /// Företagets svarsadress, tom om den saknas.
    pub svarsadress: String,
    /// Företagets standardmeddelande, eller en förvald text.
    pub meddelande: String,
    /// Den arkiverade PDF:ens filnamn.
    pub bilaga: String,
    /// Det senaste utskicket, när det var ett mejl som misslyckades.
    pub misslyckat: Option<Utskick>,
}

/// En utfärdad faktura som den skickas.
struct Fakturan {
    nummer: u64,
    total: i64,
    förfallodag: Date,
    /// Kundens adress som den kopierades in vid utfärdandet.
    kopierad_epost: String,
    /// Kundens adress i kundregistret nu.
    registrets_epost: Option<String>,
    företagsnamn: String,
    pdf: Bilaga,
    /// Originalets nummer när fakturan är en kreditfaktura (#140).
    krediterar: Option<u64>,
}

impl Fakturan {
    fn ämne(&self) -> String {
        match self.krediterar {
            Some(_) => kreditämne(self.nummer, &self.företagsnamn),
            None => ämne(self.nummer, &self.företagsnamn),
        }
    }

    fn kundens_adress(&self) -> String {
        match &self.registrets_epost {
            Some(e) if !e.trim().is_empty() => e.clone(),
            _ => self.kopierad_epost.clone(),
        }
    }
}

type Fakturarad = (
    String,
    Option<i64>,
    Option<i64>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<Vec<u8>>,
    Option<i64>,
);

async fn läs_fakturan(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    id: KundfakturaId,
) -> Result<Fakturan, UtskickFel> {
    let rad: Option<Fakturarad> = sqlx::query_as(
        "SELECT f.status, f.nummer, f.total, f.forfallodag,
                f.kunduppgifter ->> '$.epost', k.epost, f.saljaruppgifter ->> '$.namn',
                u.filnamn, u.mime, u.innehall,
                (SELECT o.nummer FROM kundfaktura o WHERE o.id = f.krediterar)
         FROM kundfaktura f
         LEFT JOIN kund k ON k.id = f.kund_id
         LEFT JOIN underlag u ON u.id = f.underlag_id
         WHERE f.id = ? AND f.foretag_id = ?",
    )
    .bind(id.0)
    .bind(företag.0)
    .fetch_optional(conn)
    .await?;
    let Some((
        status,
        nummer,
        total,
        förfallodag,
        kopierad,
        registret,
        företagsnamn,
        filnamn,
        mime,
        pdf,
        krediterar,
    )) = rad
    else {
        return Err(UtskickFel::FinnsInte);
    };
    // #143: bara Mayas egna fakturor utfärdas och skickas; en extern har status registrerad.
    if Status::från_kod(&status) != Some(Status::Utfärdad) {
        return Err(UtskickFel::InteUtfärdad);
    }
    let korrupt = |vad: &str| DatabasFel::Korrupt(format!("kundfaktura {}: {vad}", id.0));
    Ok(Fakturan {
        nummer: nummer.ok_or_else(|| korrupt("nummer"))?.unsigned_abs(),
        total: total.ok_or_else(|| korrupt("total"))?,
        förfallodag: datum(&förfallodag.ok_or_else(|| korrupt("förfallodag"))?)?,
        kopierad_epost: kopierad.unwrap_or_default(),
        registrets_epost: registret,
        företagsnamn: företagsnamn.unwrap_or_default(),
        pdf: Bilaga {
            filnamn: filnamn.ok_or_else(|| korrupt("pdf"))?,
            mime: mime.ok_or_else(|| korrupt("pdf"))?,
            innehåll: pdf.ok_or_else(|| korrupt("pdf"))?,
        },
        krediterar: krediterar.map(i64::unsigned_abs),
    })
}

/// Mottagare, svarsadress, meddelande och bilaga som panelen Skicka förvalt visar.
pub async fn förslag(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    id: KundfakturaId,
) -> Result<Förslag, UtskickFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    let f = läs_fakturan(&mut tx, företag, id).await?;
    let uppgifter = fakturauppgifter::läs_i(&mut tx, företag).await?.uppgifter;
    let senaste = läs_utskicken(&mut tx, företag, id).await?.pop();
    let misslyckat = senaste.filter(|u| u.fel.is_some());
    let till = match &misslyckat {
        Some(Utskick {
            sätt: Sätt::Epost { till, .. },
            ..
        }) => till.clone(),
        _ => f.kundens_adress(),
    };
    Ok(Förslag {
        till,
        kundregistret: f.kundens_adress(),
        svarsadress: uppgifter.svarsadress.clone(),
        meddelande: match f.krediterar {
            Some(original) => kreditmeddelande(f.nummer, f.total, original, &f.företagsnamn),
            None => förvalt_meddelande(
                &uppgifter.standardmeddelande,
                f.nummer,
                f.total,
                f.förfallodag,
                &f.företagsnamn,
            ),
        },
        bilaga: f.pdf.filnamn,
        misslyckat,
    })
}

/// Skickar den utfärdade fakturan med e-post genom `epost`: den arkiverade PDF:en som bilaga,
/// företagets svarsadress och meddelandet. Utfallet loggas, och ett misslyckat utskick är ett
/// [`Utfall`], inget fel. En ogiltig mottagare ger [`UtskickFel::Mottagare`] utan att något
/// skickas eller loggas.
pub async fn skicka<E: Epost>(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    id: KundfakturaId,
    utskick: &NyttUtskick,
    epost: &E,
) -> Result<Utfall, UtskickFel> {
    let till = maya_domain::epost::normalisera(&utskick.till)
        .map_err(|e| UtskickFel::Mottagare(e.to_string()))?;
    let (mejl, nummer) = {
        let mut tx = db.pool().begin().await?;
        atkomst::kräv(&mut tx, aktör, företag, Handling::SkickaFaktura).await?;
        let f = läs_fakturan(&mut tx, företag, id).await?;
        let svarsadress = fakturauppgifter::läs_i(&mut tx, företag)
            .await?
            .uppgifter
            .svarsadress;
        let mejl = Fakturamejl {
            till,
            svara_till: (!svarsadress.is_empty()).then_some(svarsadress),
            avsändarnamn: f.företagsnamn.clone(),
            ämne: f.ämne(),
            text: utskick.meddelande.replace("\r\n", "\n"),
            bilaga: f.pdf,
        };
        (mejl, f.nummer)
    };
    // Utanför transaktionen: ingen skrivning väntar på SMTP-servern.
    let utfall = match epost.skicka(&mejl, klocka.nu()).await {
        Ok(()) => Utfall::Skickat,
        Err(fel) => {
            tracing::warn!("kundfaktura {nummer} kunde inte skickas: {fel}");
            Utfall::Misslyckat(fel)
        }
    };

    let mut ärende = Ärende::börja(db, klocka, aktör, företag, Handling::SkickaFaktura).await?;
    let fel = match &utfall {
        Utfall::Skickat => None,
        Utfall::Misslyckat(fel) => Some(fel.clone()),
    };
    let (nu, kanal, av) = (
        ärende.nu.to_string(),
        ärende.aktör.kanal.namn(),
        ärende.aktör.användare.map(|a| a.0),
    );
    sqlx::query(
        "INSERT INTO kundfaktura_utskick
             (foretag_id, faktura_id, tid, kanal, av, satt, mottagare, svarsadress, meddelande,
              resultat, fel)
         VALUES (?, ?, ?, ?, ?, 'epost', ?, ?, ?, ?, ?)",
    )
    .bind(företag.0)
    .bind(id.0)
    .bind(nu)
    .bind(kanal)
    .bind(av)
    .bind(&mejl.till)
    .bind(&mejl.svara_till)
    .bind(&mejl.text)
    .bind(if fel.is_some() {
        "misslyckat"
    } else {
        "skickat"
    })
    .bind(&fel)
    .execute(ärende.conn())
    .await?;
    let händelse = if fel.is_some() {
        Händelse::UtskickMisslyckat
    } else {
        Händelse::UtskickGjort
    };
    ärende
        .logga(
            händelse,
            &nummer.to_string(),
            json!({
                "kundfaktura": id.0,
                "nummer": nummer,
                "till": mejl.till,
                "svarsadress": mejl.svara_till,
                "bilaga": mejl.bilaga.filnamn,
                "fel": fel,
            }),
        )
        .await?;
    ärende.slutför().await?;
    Ok(utfall)
}

/// Utfärdar utkastet och skickar sedan fakturan till kundens e-post med företagets förvalda
/// meddelande. Utfärdandet är committat innan något skickas: ett misslyckat utskick loggas och
/// ges som utfall, men fakturan förblir utfärdad. `None` när kunden saknar e-postadress; då
/// skickas inget.
pub async fn utfärda_och_skicka<E: Epost>(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    id: KundfakturaId,
    epost: &E,
) -> Result<(Utfärdad, Option<Utfall>), KundfakturaFel> {
    let utfärdad = kundfaktura::utfärda(db, klocka, aktör, företag, id).await?;
    let utfall = async {
        let förslag = förslag(db, aktör, företag, utfärdad.id).await?;
        if förslag.till.trim().is_empty() {
            return Ok(None);
        }
        let utskick = NyttUtskick {
            till: förslag.till,
            meddelande: förslag.meddelande,
        };
        skicka(db, klocka, aktör, företag, utfärdad.id, &utskick, epost)
            .await
            .map(Some)
    }
    .await;
    let utfall = utfall.unwrap_or_else(|e: UtskickFel| {
        tracing::error!(
            "kundfaktura {} utfärdades men skickades inte: {e}",
            utfärdad.nummer
        );
        Some(Utfall::Misslyckat(e.to_string()))
    });
    Ok((utfärdad, utfall))
}

/// Markerar att fakturan lämnades på annat sätt, t.ex. när Maya saknar SMTP. Loggas som ett
/// utskick; fakturan byter inte tillstånd.
pub async fn lämnad_på_annat_sätt(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    id: KundfakturaId,
    sätt: Lämningssätt,
) -> Result<(), UtskickFel> {
    let mut ärende = Ärende::börja(db, klocka, aktör, företag, Handling::SkickaFaktura).await?;
    let f = läs_fakturan(ärende.conn(), företag, id).await?;
    let (nu, kanal, av) = (
        ärende.nu.to_string(),
        ärende.aktör.kanal.namn(),
        ärende.aktör.användare.map(|a| a.0),
    );
    sqlx::query(
        "INSERT INTO kundfaktura_utskick (foretag_id, faktura_id, tid, kanal, av, satt, resultat)
         VALUES (?, ?, ?, ?, ?, ?, 'lamnad')",
    )
    .bind(företag.0)
    .bind(id.0)
    .bind(nu)
    .bind(kanal)
    .bind(av)
    .bind(sätt.kod())
    .execute(ärende.conn())
    .await?;
    ärende
        .logga(
            Händelse::UtskickLämnatPåAnnatSätt,
            &f.nummer.to_string(),
            json!({
                "kundfaktura": id.0,
                "nummer": f.nummer,
                "sätt": sätt.kod(),
                "hur": sätt.namn(),
            }),
        )
        .await?;
    ärende.slutför().await?;
    Ok(())
}

/// Fakturans utskick i ordning, det äldsta först.
pub async fn utskicken(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    id: KundfakturaId,
) -> Result<Vec<Utskick>, UtskickFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    läs_utskicken(&mut tx, företag, id).await
}

type Utskicksrad = (
    String,
    String,
    Option<i64>,
    Option<String>,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

async fn läs_utskicken(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    id: KundfakturaId,
) -> Result<Vec<Utskick>, UtskickFel> {
    let rader: Vec<Utskicksrad> = sqlx::query_as(
        "SELECT u.tid, u.kanal, u.av, a.visningsnamn, u.satt, u.mottagare, u.svarsadress,
                u.meddelande, u.fel
         FROM kundfaktura_utskick u LEFT JOIN anvandare a ON a.id = u.av
         WHERE u.foretag_id = ? AND u.faktura_id = ? ORDER BY u.id",
    )
    .bind(företag.0)
    .bind(id.0)
    .fetch_all(conn)
    .await?;
    rader
        .into_iter()
        .map(
            |(tid, kanal, av, namn, sätt, mottagare, svarsadress, meddelande, fel)| {
                let sätt =
                    match (sätt.as_str(), mottagare) {
                        ("epost", Some(till)) => Sätt::Epost { till, svarsadress },
                        (kod, _) => Sätt::Lämnad(Lämningssätt::från_kod(kod).ok_or_else(|| {
                            DatabasFel::Korrupt("kundfaktura_utskick.satt".into())
                        })?),
                    };
                Ok(Utskick {
                    signatur: signatur(&tid, &kanal, av, namn)?,
                    sätt,
                    meddelande,
                    fel,
                })
            },
        )
        .collect()
}
