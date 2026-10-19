//! Betalningar av fakturor i reskontran (#139): registrera en betalning och rätta en
//! felregistrerad, med verifikatet i samma transaktion som betalningshändelsen (ADR 0003, L5).
//!
//! Betalningen prövas med spärrarna i [`maya_domain::betalning`] mot det som återstår i
//! transaktionen, så att två samtidiga betalningar aldrig överbetalar fakturan, och bokförs
//! enligt tabellen över automatkonteringar med fakturan som referens. *Rätta betalning* vänder
//! betalningens verifikat med en rättelse (R1) och tar bort betalningen ur reskontran;
//! betalningen finns kvar och visar rättelsen (R2), och betalningen kan registreras om. Det som
//! återstår och statusen räknas fram ur betalningarna och lagras inte.
//!
//! Betalningen hör till en [`Faktura`]: en kundfaktura nu. Leverantörsfakturorna (#147) lägger
//! till sin variant; tabellen har redan kolumnen.

use std::fmt;
use std::str::FromStr;

use jiff::civil::Date;
use maya_domain::betalning::{Förutsättningar, NyBetalning, Reskontra, Spärr, beskrivning};
use maya_domain::kontoplan::Kontonummer;
use serde_json::json;
use sqlx::SqliteConnection;

use crate::aktor::{Aktör, Handling};
use crate::atkomst::{self, Ärende, ÅtkomstFel};
use crate::behandlingshistorik::Händelse;
use crate::foretag::{FöretagsId, datum};
use crate::kundfaktura::KundfakturaId;
use crate::verifikat::{
    self, Bokfört, Flöde, Länk, Signatur, VerifikatFel, VerifikatId, signatur
};
use crate::{Databas, DatabasFel, Klocka};

/// En betalnings id i installationen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BetalningsId(pub(crate) i64);

impl fmt::Display for BetalningsId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for BetalningsId {
    type Err = std::num::ParseIntError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse().map(BetalningsId)
    }
}

/// Fakturan som betalningen gäller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Faktura {
    /// En utfärdad kundfaktura.
    Kund(KundfakturaId),
}

impl Faktura {
    fn reskontra(self) -> Reskontra {
        match self {
            Faktura::Kund(_) => Reskontra::Kund,
        }
    }

    /// Värdena för `betalning.kundfaktura_id` och `betalning.leverantorsfaktura_id`: precis
    /// ett är satt.
    fn kolumner(self) -> (Option<i64>, Option<i64>) {
        match self {
            Faktura::Kund(id) => (Some(id.0), None),
        }
    }

    /// Nyckeln i behandlingshistorikens beskrivning, som fakturans historik läses med.
    fn nyckel(self) -> &'static str {
        match self {
            Faktura::Kund(_) => "kundfaktura",
        }
    }

    fn id(self) -> i64 {
        match self {
            Faktura::Kund(id) => id.0,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BetalningFel {
    /// Spärrarna i `domain`, med samma koder som panelen visar.
    #[error("betalningen kan inte registreras")]
    Spärrat(Vec<Spärr>),
    /// Ingen utfärdad faktura med det id:t i företaget.
    #[error("fakturan finns inte")]
    FinnsInte,
    /// Ingen betalning med det id:t i företaget.
    #[error("betalningen finns inte")]
    IngenBetalning,
    /// Betalningen är redan rättad.
    #[error("betalningen är redan rättad")]
    RedanRättad,
    /// Betalningens verifikat kunde inte bokföras eller rättas, t.ex. ett rättelsedatum före
    /// betaldagen.
    #[error(transparent)]
    Verifikat(VerifikatFel),
    #[error(transparent)]
    Åtkomst(#[from] ÅtkomstFel),
}

impl From<sqlx::Error> for BetalningFel {
    fn from(e: sqlx::Error) -> Self {
        BetalningFel::Åtkomst(e.into())
    }
}

impl From<DatabasFel> for BetalningFel {
    fn from(e: DatabasFel) -> Self {
        BetalningFel::Åtkomst(e.into())
    }
}

impl From<VerifikatFel> for BetalningFel {
    fn from(e: VerifikatFel) -> Self {
        match e {
            VerifikatFel::Åtkomst(e) => BetalningFel::Åtkomst(e),
            e => BetalningFel::Verifikat(e),
        }
    }
}

/// En registrerad betalning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Registrerad {
    pub id: BetalningsId,
    pub verifikat: Bokfört,
    /// Det som återstår av fakturan efter betalningen, i öre.
    pub återstår: i64,
}

/// En rättad betalning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rättad {
    pub rättelse: Bokfört,
    /// Det som återstår av fakturan när betalningen inte längre räknas, i öre.
    pub återstår: i64,
}

/// Rättelsen av en betalning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Betalningsrättelse {
    pub verifikat: Länk,
    pub orsak: String,
    pub rättad: Signatur,
}

/// En betalning av fakturan, rättad eller inte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Betalningspost {
    pub id: BetalningsId,
    pub betaldag: Date,
    /// I öre.
    pub belopp: i64,
    /// Resten som stängdes mot öresutjämning, i öre.
    pub öresutjämning: i64,
    pub konto: Kontonummer,
    pub verifikat: Länk,
    pub registrerad: Signatur,
    /// `Some` när betalningen är rättad och inte längre räknas i reskontran.
    pub rättad: Option<Betalningsrättelse>,
}

/// Fakturan i reskontran: totalen, det som återstår och betalningarna i ordning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reskontrapost {
    pub total: i64,
    pub återstår: i64,
    pub förfallodag: Date,
    pub betalningar: Vec<Betalningspost>,
}

/// Fakturan som betalningen gäller, läst i transaktionen.
struct Fakturan {
    nummer: String,
    motpart: String,
    total: i64,
    förfallodag: Date,
}

impl Fakturan {
    /// Fakturan som verifikatets referens: "kundfaktura 1007, Kund AB".
    fn referens(&self, faktura: Faktura) -> String {
        match faktura {
            Faktura::Kund(_) => format!("kundfaktura {}, {}", self.nummer, self.motpart),
        }
    }
}

async fn läs_fakturan(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    faktura: Faktura,
) -> Result<Fakturan, BetalningFel> {
    let rad: Option<(i64, String, i64, String)> = match faktura {
        Faktura::Kund(id) => {
            sqlx::query_as(
                "SELECT nummer, kunduppgifter ->> '$.namn', total, forfallodag FROM kundfaktura
                 WHERE id = ? AND foretag_id = ? AND status <> 'utkast'",
            )
            .bind(id.0)
            .bind(företag.0)
            .fetch_optional(&mut *conn)
            .await?
        }
    };
    let (nummer, motpart, total, förfallodag) = rad.ok_or(BetalningFel::FinnsInte)?;
    Ok(Fakturan {
        nummer: nummer.to_string(),
        motpart,
        total,
        förfallodag: datum(&förfallodag)?,
    })
}

/// Det som är betalt av fakturan: betalningarna som inte är rättade, med öresutjämningen.
async fn betalt_i(conn: &mut SqliteConnection, faktura: Faktura) -> Result<i64, sqlx::Error> {
    let (kund, leverantör) = faktura.kolumner();
    sqlx::query_scalar(
        "SELECT coalesce(sum(belopp + oresutjamning), 0) FROM betalning_i_reskontran
         WHERE kundfaktura_id = ?1 OR leverantorsfaktura_id = ?2",
    )
    .bind(kund)
    .bind(leverantör)
    .fetch_one(conn)
    .await
}

/// Det som betalningspanelen prövas mot: fakturans reskontra, det som återstår, kontona och
/// räkenskapsåren.
pub async fn förutsättningar(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    faktura: Faktura,
) -> Result<Förutsättningar, BetalningFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    läs_förutsättningar(&mut tx, företag, faktura).await
}

async fn läs_förutsättningar(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    faktura: Faktura,
) -> Result<Förutsättningar, BetalningFel> {
    let fakturan = läs_fakturan(&mut *conn, företag, faktura).await?;
    let återstår = fakturan.total - betalt_i(&mut *conn, faktura).await?;
    let v = verifikat::läs_förutsättningar(&mut *conn, företag).await?;
    Ok(Förutsättningar {
        reskontra: faktura.reskontra(),
        återstår,
        konton: v.konton,
        räkenskapsår: v.räkenskapsår,
    })
}

/// Registrerar en betalning av fakturan och bokför dess verifikat, daterat på betaldagen med
/// fakturan som referens, i samma transaktion: betalkontot mot reskontrakontot, och en rest på
/// högst 10 kr mot öresutjämning om användaren valde det. En överbetalning spärras.
pub async fn registrera(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    faktura: Faktura,
    formulär: &NyBetalning,
) -> Result<Registrerad, BetalningFel> {
    let mut ärende =
        Ärende::börja(db, klocka, aktör, företag, Handling::RegistreraBetalning).await?;
    let fakturan = läs_fakturan(ärende.conn(), företag, faktura).await?;
    let f = läs_förutsättningar(ärende.conn(), företag, faktura).await?;
    let b = formulär.kontrollera(&f).map_err(BetalningFel::Spärrat)?;
    let bokfört = verifikat::bokför_i_ärende(
        &mut ärende,
        &verifikat::formulär_av(
            b.betaldag,
            &beskrivning(&fakturan.referens(faktura)),
            &b.kontering(),
        ),
        &[],
    )
    .await?;
    let (nu, kanal, av) = (
        ärende.nu.to_string(),
        ärende.aktör.kanal.namn(),
        ärende.aktör.användare.map(|a| a.0),
    );
    let (kund, leverantör) = faktura.kolumner();
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO betalning
             (foretag_id, kundfaktura_id, leverantorsfaktura_id, betaldag, belopp, konto,
              oresutjamning, verifikat_id, registrerad, registrerad_kanal, registrerad_av)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(företag.0)
    .bind(kund)
    .bind(leverantör)
    .bind(b.betaldag.to_string())
    .bind(b.belopp)
    .bind(i64::from(b.konto.get()))
    .bind(b.öresutjämning)
    .bind(bokfört.id.0)
    .bind(nu)
    .bind(kanal)
    .bind(av)
    .fetch_one(ärende.conn())
    .await?;
    let mut post = json!({
                "betalning": id,
                "fakturanummer": fakturan.nummer,
                "motpart": fakturan.motpart,
                "betaldag": b.betaldag.to_string(),
                "konto": b.konto.get(),
                "summa_öre": b.belopp,
                "öresutjämning_öre": b.öresutjämning,
                "återstår_öre": b.återstår_efter,
                "verifikat": { "id": bokfört.id.0, "nummer": bokfört.nummer.to_string() },
    });
    post[faktura.nyckel()] = json!(faktura.id());
    ärende
        .logga(Händelse::BetalningRegistrerad, &fakturan.nummer, post)
        .await?;
    ärende.slutför().await?;
    Ok(Registrerad {
        id: BetalningsId(id),
        verifikat: bokfört,
        återstår: b.återstår_efter,
    })
}

/// Rättar en felregistrerad betalning: vänder dess verifikat med en rättelse daterad `datum`
/// med den valfria `orsak`en (R1) och tar bort betalningen ur reskontran, så att det som
/// återstår blir som före betalningen. Betalningen och dess verifikat finns kvar och visar
/// rättelsen (R2, L1). En betalning rättas högst en gång.
#[allow(clippy::too_many_arguments)]
pub async fn rätta(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    betalning: BetalningsId,
    datum: &str,
    orsak: &str,
) -> Result<Rättad, BetalningFel> {
    let mut ärende = Ärende::börja(db, klocka, aktör, företag, Handling::RättaFlöde).await?;
    let rad: Option<(Option<i64>, i64, i64, i64, bool)> = sqlx::query_as(
        "SELECT kundfaktura_id, verifikat_id, belopp, oresutjamning,
                EXISTS (SELECT 1 FROM betalningsrattelse r WHERE r.betalning_id = b.id)
         FROM betalning b WHERE id = ? AND foretag_id = ?",
    )
    .bind(betalning.0)
    .bind(företag.0)
    .fetch_optional(ärende.conn())
    .await?;
    let (kundfaktura, verifikat_id, belopp, öresutjämning, rättad) =
        rad.ok_or(BetalningFel::IngenBetalning)?;
    if rättad {
        return Err(BetalningFel::RedanRättad);
    }
    let faktura = match kundfaktura {
        Some(id) => Faktura::Kund(KundfakturaId(id)),
        None => return Err(BetalningFel::IngenBetalning),
    };
    let fakturan = läs_fakturan(ärende.conn(), företag, faktura).await?;
    let datum = rättelsedatum(datum)?;
    let rättelse = verifikat::rätta_i(&mut ärende, VerifikatId(verifikat_id), datum, orsak).await?;
    let (nu, kanal, av) = (
        ärende.nu.to_string(),
        ärende.aktör.kanal.namn(),
        ärende.aktör.användare.map(|a| a.0),
    );
    sqlx::query(
        "INSERT INTO betalningsrattelse
             (betalning_id, verifikat_id, orsak, rattad, rattad_kanal, rattad_av)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(betalning.0)
    .bind(rättelse.id.0)
    .bind(orsak.trim())
    .bind(nu)
    .bind(kanal)
    .bind(av)
    .execute(ärende.conn())
    .await?;
    let återstår = fakturan.total - betalt_i(ärende.conn(), faktura).await?;
    let verifikatnummer = verifikat::länk(ärende.conn(), Some(verifikat_id))
        .await?
        .and_then(|l| l.nummer)
        .map(|n| n.to_string())
        .unwrap_or_default();
    let mut post = json!({
                "betalning": betalning.0,
                "fakturanummer": fakturan.nummer,
                "motpart": fakturan.motpart,
                "summa_öre": belopp,
                "öresutjämning_öre": öresutjämning,
                "orsak": orsak.trim(),
                "återstår_öre": återstår,
                "verifikat": { "id": verifikat_id, "nummer": verifikatnummer },
                "rättelse": { "id": rättelse.id.0, "nummer": rättelse.nummer.to_string() },
    });
    post[faktura.nyckel()] = json!(faktura.id());
    ärende
        .logga(Händelse::BetalningRättad, &fakturan.nummer, post)
        .await?;
    ärende.slutför().await?;
    Ok(Rättad {
        rättelse, återstår
    })
}

/// Rättelsens datum som användaren skrev det, med samma spärrar som ett verifikatdatum.
fn rättelsedatum(text: &str) -> Result<Date, BetalningFel> {
    let spärr = |kod: &'static str, text: &str| {
        BetalningFel::Verifikat(VerifikatFel::Spärrat(vec![
            maya_domain::verifikat::Spärr {
                fält: maya_domain::verifikat::Fält::Datum,
                kod,
                text: text.into(),
            },
        ]))
    };
    match text.trim() {
        "" => Err(spärr("datum_saknas", "Ange datum.")),
        text => text
            .parse()
            .map_err(|_| spärr("datum_ogiltigt", "Skriv datumet som ÅÅÅÅ-MM-DD.")),
    }
}

/// Fakturan i reskontran med det som återstår och varje betalning, också de rättade.
pub async fn reskontrapost(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    faktura: Faktura,
) -> Result<Reskontrapost, BetalningFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    let fakturan = läs_fakturan(&mut tx, företag, faktura).await?;
    let återstår = fakturan.total - betalt_i(&mut tx, faktura).await?;
    type Rad = (
        i64,
        String,
        i64,
        i64,
        i64,
        i64,
        String,
        String,
        Option<i64>,
        Option<String>,
        Option<i64>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<String>,
    );
    let (kund, leverantör) = faktura.kolumner();
    let rader: Vec<Rad> = sqlx::query_as(
        "SELECT b.id, b.betaldag, b.belopp, b.oresutjamning, b.konto, b.verifikat_id,
                b.registrerad, b.registrerad_kanal, b.registrerad_av, a.visningsnamn,
                r.verifikat_id, r.orsak, r.rattad, r.rattad_kanal, r.rattad_av, ra.visningsnamn
         FROM betalning b
         LEFT JOIN anvandare a ON a.id = b.registrerad_av
         LEFT JOIN betalningsrattelse r ON r.betalning_id = b.id
         LEFT JOIN anvandare ra ON ra.id = r.rattad_av
         WHERE b.kundfaktura_id = ?1 OR b.leverantorsfaktura_id = ?2
         ORDER BY b.betaldag, b.id",
    )
    .bind(kund)
    .bind(leverantör)
    .fetch_all(&mut *tx)
    .await?;
    let mut betalningar = Vec::new();
    for (
        id,
        betaldag,
        belopp,
        öresutjämning,
        konto,
        verifikat_id,
        tid,
        kanal,
        av,
        namn,
        rättelse,
        orsak,
        rättad,
        rättad_kanal,
        rättad_av,
        rättad_namn,
    ) in rader
    {
        let verifikat = verifikat::länk(&mut tx, Some(verifikat_id))
            .await?
            .ok_or_else(|| DatabasFel::Korrupt("betalning.verifikat_id".into()))?;
        let rättad = match (rättelse, rättad, rättad_kanal) {
            (Some(v), Some(tid), Some(kanal)) => Some(Betalningsrättelse {
                verifikat: verifikat::länk(&mut tx, Some(v))
                    .await?
                    .ok_or_else(|| DatabasFel::Korrupt("betalningsrattelse".into()))?,
                orsak: orsak.unwrap_or_default(),
                rättad: signatur(&tid, &kanal, rättad_av, rättad_namn)?,
            }),
            _ => None,
        };
        betalningar.push(Betalningspost {
            id: BetalningsId(id),
            betaldag: datum(&betaldag)?,
            belopp,
            öresutjämning,
            konto: u16::try_from(konto)
                .ok()
                .and_then(Kontonummer::new)
                .ok_or_else(|| DatabasFel::Korrupt("betalning.konto".into()))?,
            verifikat,
            registrerad: signatur(&tid, &kanal, av, namn)?,
            rättad,
        });
    }
    Ok(Reskontrapost {
        total: fakturan.total,
        återstår,
        förfallodag: fakturan.förfallodag,
        betalningar,
    })
}

/// Fakturan som en betalnings verifikat, eller rättelsen av det, bokfördes från, för länken
/// från verifikatet och spärren mot en vanlig rättelse.
pub(crate) async fn fakturan_för_verifikat(
    conn: &mut SqliteConnection,
    verifikat: VerifikatId,
) -> Result<Option<(Flöde, i64, String)>, DatabasFel> {
    let rad: Option<(i64, i64, String)> = sqlx::query_as(
        "SELECT k.id, k.nummer, k.kunduppgifter ->> '$.namn'
         FROM betalning b JOIN kundfaktura k ON k.id = b.kundfaktura_id
         WHERE b.verifikat_id = ?1
            OR b.id IN (SELECT betalning_id FROM betalningsrattelse WHERE verifikat_id = ?1)",
    )
    .bind(verifikat.0)
    .fetch_optional(conn)
    .await?;
    Ok(rad.map(|(id, nummer, kund)| {
        (
            Flöde::Kundfaktura,
            id,
            format!("betalningen av kundfaktura {nummer} till {kund}"),
        )
    }))
}
