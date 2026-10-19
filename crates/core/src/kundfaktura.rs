//! Kundfakturor som skapas i Maya: utkast, utfärdande och läsning (#135).
//!
//! Ett utkast kan ändras och raderas fritt; raderingen loggas med en sammanfattning. Att
//! *utfärda* gör i en transaktion: prövar spärrarna i [`maya_domain::kundfaktura`] mot det som
//! gäller i transaktionen, ger fakturan nästa nummer i företagets obrutna fakturaserie,
//! kopierar in kundens och företagets uppgifter, skriver PDF:en en gång och arkiverar den som
//! underlag med SHA-256 (VF5), bokför verifikatet daterat på fakturadatumet med PDF:en som
//! underlag (fakturametoden) och loggar utfärdandet (H1). Schemat avvisar att en utfärdad
//! faktura, dess nummer eller PDF ändras (L1).

use std::fmt;
use std::str::FromStr;

use jiff::Timestamp;
use jiff::civil::Date;
use maya_domain::faktura::{Antal, Momsrad, Momssats, Rabatt, momsrader};
use maya_domain::foretag::Bokföringsmetod;
use maya_domain::kontoplan::Kontonummer;
use maya_domain::kundfaktura::{
    Fakturakund, Fakturarad, Förutsättningar, NyKundfaktura, Spärr, Säljare, Utfärdbar, Utkast,
    Utkastrad,
};
use serde_json::json;
use sqlx::SqliteConnection;

use crate::aktor::{Aktör, Handling};
use crate::atkomst::{self, Ärende, ÅtkomstFel};
use crate::behandlingshistorik::{self, Händelse};
use crate::foretag::{self, FöretagsId, datum};
use crate::underlag::{self, UnderlagFel, UnderlagId};
use crate::utskrift::Pdf;
use crate::verifikat::{
    self, Bokfört, Historikpost, Signatur, VerifikatFel, VerifikatId, signatur,
};
use crate::{Databas, DatabasFel, Klocka, fakturauppgifter};

/// En kundfakturas id i installationen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct KundfakturaId(pub(crate) i64);

impl fmt::Display for KundfakturaId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for KundfakturaId {
    type Err = std::num::ParseIntError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse().map(KundfakturaId)
    }
}

/// Var fakturan är i sin livscykel. Betald, förfallen och de andra räknas fram ur händelserna
/// och lagras inte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Utkast,
    Utfärdad,
}

impl Status {
    fn från_kod(kod: &str) -> Option<Self> {
        match kod {
            "utkast" => Some(Status::Utkast),
            "utfardad" => Some(Status::Utfärdad),
            _ => None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum KundfakturaFel {
    /// Spärrarna i `domain`, med samma koder som formuläret visar.
    #[error("kundfakturan kan inte utfärdas")]
    Spärrat(Vec<Spärr>),
    /// Ingen kundfaktura med det id:t i företaget.
    #[error("kundfakturan finns inte")]
    FinnsInte,
    /// Fakturan är inget utkast i företaget: den finns inte eller är redan utfärdad.
    #[error("utkastet finns inte")]
    IngetUtkast,
    /// Startnumret väljs bara innan företaget har utfärdat sin första faktura.
    #[error("startnumret kan inte ändras efter första utfärdandet")]
    StartnummerLåst,
    /// Kontantmetoden på fakturadatumet: utfärdandet bokförs inte, och det flödet kommer i
    /// #149.
    #[error("kundfakturor under kontantmetoden går inte att utfärda ännu")]
    Kontantmetoden,
    #[error(transparent)]
    Pdf(#[from] maya_pdf::PdfFel),
    #[error(transparent)]
    Underlag(UnderlagFel),
    #[error(transparent)]
    Verifikat(VerifikatFel),
    #[error(transparent)]
    Åtkomst(#[from] ÅtkomstFel),
}

impl From<sqlx::Error> for KundfakturaFel {
    fn from(e: sqlx::Error) -> Self {
        KundfakturaFel::Åtkomst(e.into())
    }
}

impl From<DatabasFel> for KundfakturaFel {
    fn from(e: DatabasFel) -> Self {
        KundfakturaFel::Åtkomst(e.into())
    }
}

impl From<UnderlagFel> for KundfakturaFel {
    fn from(e: UnderlagFel) -> Self {
        match e {
            UnderlagFel::Åtkomst(e) => KundfakturaFel::Åtkomst(e),
            e => KundfakturaFel::Underlag(e),
        }
    }
}

impl From<VerifikatFel> for KundfakturaFel {
    fn from(e: VerifikatFel) -> Self {
        match e {
            VerifikatFel::Åtkomst(e) => KundfakturaFel::Åtkomst(e),
            e => KundfakturaFel::Verifikat(e),
        }
    }
}

/// Resultatet av ett utfärdande.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Utfärdad {
    pub id: KundfakturaId,
    /// Numret i fakturaserien.
    pub nummer: u64,
    pub verifikat: Bokfört,
    /// Den arkiverade PDF:en.
    pub underlag: UnderlagId,
}

/// En kundfaktura i listan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kundfakturarad {
    pub id: KundfakturaId,
    pub status: Status,
    pub nummer: Option<u64>,
    /// Kundens namn, som det kopierades in vid utfärdandet; tomt för ett utkast utan kund.
    pub kund: String,
    pub fakturadatum: Option<Date>,
    pub förfallodag: Option<Date>,
    /// Totalen i öre; för ett utkast summan av de rader som går att räkna.
    pub total: i64,
}

/// Den arkiverade PDF:en, som aldrig skrivs om.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArkiveradPdf {
    pub underlag: UnderlagId,
    pub filnamn: String,
    /// SHA-256 i hex, räknad när PDF:en arkiverades.
    pub sha256: String,
    pub storlek: u64,
    pub arkiverad: Timestamp,
}

/// Det en utfärdad faktura bär: uppgifterna som de var när den utfärdades.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Utfärdning {
    pub nummer: u64,
    /// OCR-referensen med OCR på, annars numret.
    pub referens: String,
    pub kund: Fakturakund,
    pub säljare: Säljare,
    pub fakturadatum: Date,
    pub leveransdatum: String,
    pub förfallodag: Date,
    pub er_referens: String,
    pub rader: Vec<Fakturarad>,
    pub momsrader: Vec<Momsrad>,
    pub netto: i64,
    pub moms: i64,
    pub total: i64,
    /// Utfärdandets verifikat.
    pub verifikat: verifikat::Länk,
    pub pdf: ArkiveradPdf,
    pub utfärdad: Signatur,
}

/// En kundfaktura: ett utkast med sitt formulär, eller en utfärdad faktura.
#[derive(Debug, Clone, PartialEq)]
pub struct Kundfaktura {
    pub id: KundfakturaId,
    pub status: Status,
    /// Kundens namn: ur registret för ett utkast, som det kopierades in för en utfärdad.
    pub kund: String,
    /// Fakturan som formulär. För ett utkast det som är sparat.
    pub formulär: NyKundfaktura,
    /// `None` för ett utkast.
    pub utfärdad: Option<Utfärdning>,
    pub skapad: Signatur,
    /// Posterna i behandlingshistoriken som rör fakturan, i ordning.
    pub historik: Vec<Historikpost>,
}

/// Det som spärrarna prövas mot, för formuläret och förhandsvisningen.
pub async fn förutsättningar(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
) -> Result<Förutsättningar, ÅtkomstFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    Ok(läs_förutsättningar(&mut tx, företag).await?)
}

async fn läs_förutsättningar(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
) -> Result<Förutsättningar, DatabasFel> {
    let f = foretag::läs_företag(&mut *conn, företag).await?;
    let inställningar = fakturauppgifter::läs_i(&mut *conn, företag).await?;
    let u = &f.uppgifter;
    let b = &inställningar.uppgifter;
    let kunder: Vec<(i64, String, String, String, String, String, i64)> = sqlx::query_as(
        "SELECT id, namn, identitet, adress, epost, momsregnr, betalningsvillkor FROM kund
         WHERE foretag_id = ? AND status = 'aktiv' ORDER BY namn COLLATE NOCASE, id",
    )
    .bind(företag.0)
    .fetch_all(&mut *conn)
    .await?;
    let konton: Vec<(i64, bool)> =
        sqlx::query_as("SELECT nummer, aktiv FROM konto WHERE foretag_id = ? ORDER BY nummer")
            .bind(företag.0)
            .fetch_all(&mut *conn)
            .await?;
    let (nästa, startnummer) = serien(&mut *conn, företag).await?;
    Ok(Förutsättningar {
        kunder: kunder
            .into_iter()
            .map(
                |(id, namn, identitet, adress, epost, momsregnr, villkor)| Fakturakund {
                    id,
                    namn,
                    identitet,
                    adress,
                    epost,
                    momsregnr,
                    betalningsvillkor: u16::try_from(villkor).unwrap_or(u16::MAX),
                },
            )
            .collect(),
        säljare: Säljare {
            namn: u.namn.clone(),
            orgnr: f.orgnr.to_string(),
            gatuadress: u.gatuadress.clone(),
            postnummer: u.postnummer.clone(),
            postort: u.postort.clone(),
            säte: u.säte.clone(),
            momsregnr: u.momsregnr.clone(),
            f_skatt: u.f_skatt,
            bankgiro: b.bankgiro.clone(),
            plusgiro: b.plusgiro.clone(),
            iban: b.iban.clone(),
            bic: b.bic.clone(),
            ocr: b.ocr,
        },
        konton: konton
            .into_iter()
            .map(|(nummer, aktiv)| Ok((kontonummer(nummer)?, aktiv)))
            .collect::<Result<_, DatabasFel>>()?,
        räkenskapsår: f
            .räkenskapsår
            .iter()
            .map(|å| maya_domain::rakenskapsar::Räkenskapsår::befintligt(å.start, å.slut))
            .collect(),
        momsperioder: f.momsperioder,
        nästa_nummer: nästa,
        startnummer,
    })
}

/// Nästa nummer i serien när företaget har utfärdat en faktura, och det sparade startnumret.
async fn serien(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
) -> Result<(Option<u64>, u64), sqlx::Error> {
    let högsta: Option<i64> = sqlx::query_scalar(
        "SELECT max(nummer) FROM kundfaktura WHERE foretag_id = ? AND status <> 'utkast'",
    )
    .bind(företag.0)
    .fetch_one(&mut *conn)
    .await?;
    let startnummer: Option<i64> =
        sqlx::query_scalar("SELECT startnummer FROM fakturaserie WHERE foretag_id = ?")
            .bind(företag.0)
            .fetch_optional(&mut *conn)
            .await?;
    Ok((
        högsta.map(|n| n.unsigned_abs() + 1),
        startnummer.map_or(1, i64::unsigned_abs),
    ))
}

/// Sparar formuläret som ett nytt utkast, eller som nytt innehåll i utkastet `utkast`. Bara det
/// som inte går att lagra spärrar (se [`NyKundfaktura::som_utkast`]). Ett startnummer i
/// formuläret blir företagets första fakturanummer, och ändringen loggas, så länge företaget
/// inte har utfärdat någon faktura.
pub async fn spara_utkast(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    utkast: Option<KundfakturaId>,
    formulär: &NyKundfaktura,
) -> Result<KundfakturaId, KundfakturaFel> {
    let mut ärende =
        Ärende::börja(db, klocka, aktör, företag, Handling::HanteraFakturautkast).await?;
    if let Some(id) = utkast {
        kräv_utkast(&mut ärende, id).await?;
    }
    let u = formulär.som_utkast().map_err(KundfakturaFel::Spärrat)?;
    if let Some(id) = u.kund {
        let finns: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM kund WHERE id = ? AND foretag_id = ?
                                AND status IN ('aktiv', 'arkiverad'))",
        )
        .bind(id)
        .bind(företag.0)
        .fetch_one(ärende.conn())
        .await?;
        if !finns {
            return Err(KundfakturaFel::Spärrat(
                formulär
                    .kontrollera(&läs_förutsättningar(ärende.conn(), företag).await?)
                    .err()
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|s| s.kod == "kund_saknas")
                    .collect(),
            ));
        }
    }
    if let Some(start) = u.startnummer {
        välj_startnummer(&mut ärende, start).await?;
    }
    let (nu, kanal, av) = (
        ärende.nu.to_string(),
        ärende.aktör.kanal.namn(),
        ärende.aktör.användare.map(|a| a.0),
    );
    let id = match utkast {
        Some(id) => {
            sqlx::query(
                "UPDATE kundfaktura SET kund_id = ?, fakturadatum = ?, leveransdatum = ?,
                     forfallodag = ?, er_referens = ?
                 WHERE id = ? AND status = 'utkast'",
            )
            .bind(u.kund)
            .bind(u.fakturadatum.map(|d| d.to_string()))
            .bind(&u.leveransdatum)
            .bind(u.förfallodag.map(|d| d.to_string()))
            .bind(&u.er_referens)
            .bind(id.0)
            .execute(ärende.conn())
            .await?;
            sqlx::query("DELETE FROM kundfakturarad WHERE faktura_id = ?")
                .bind(id.0)
                .execute(ärende.conn())
                .await?;
            id
        }
        None => KundfakturaId(
            sqlx::query_scalar(
                "INSERT INTO kundfaktura
                     (foretag_id, status, kund_id, fakturadatum, leveransdatum, forfallodag,
                      er_referens, skapad, skapad_kanal, skapad_av)
                 VALUES (?, 'utkast', ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
            )
            .bind(företag.0)
            .bind(u.kund)
            .bind(u.fakturadatum.map(|d| d.to_string()))
            .bind(&u.leveransdatum)
            .bind(u.förfallodag.map(|d| d.to_string()))
            .bind(&u.er_referens)
            .bind(nu)
            .bind(kanal)
            .bind(av)
            .fetch_one(ärende.conn())
            .await?,
        ),
    };
    for (i, r) in u.rader.iter().enumerate() {
        sqlx::query(
            "INSERT INTO kundfakturarad
                 (faktura_id, radnr, beskrivning, antal, enhet, a_pris, rabatt, momssats, konto)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(id.0)
        .bind(i as i64 + 1)
        .bind(&r.beskrivning)
        .bind(r.antal.map(Antal::hundradelar))
        .bind(&r.enhet)
        .bind(r.à_pris)
        .bind(r.rabatt.hundradelar())
        .bind(i64::from(r.momssats.procent()))
        .bind(r.konto.map(|k| i64::from(k.get())))
        .execute(ärende.conn())
        .await?;
    }
    ärende.slutför().await?;
    Ok(id)
}

/// Sparar företagets första fakturanummer och loggar ändringen (H2). Efter det första
/// utfärdandet är numret låst.
async fn välj_startnummer(ärende: &mut Ärende, start: u64) -> Result<(), KundfakturaFel> {
    let företag = ärende.företag;
    let (nästa, nuvarande) = serien(ärende.conn(), företag).await?;
    if start == nuvarande {
        return Ok(());
    }
    if nästa.is_some() {
        return Err(KundfakturaFel::StartnummerLåst);
    }
    sqlx::query(
        "INSERT INTO fakturaserie (foretag_id, startnummer) VALUES (?1, ?2)
         ON CONFLICT (foretag_id) DO UPDATE SET startnummer = ?2",
    )
    .bind(företag.0)
    .bind(start as i64)
    .execute(ärende.conn())
    .await?;
    ärende
        .logga(
            Händelse::StartnummerÄndrat,
            &start.to_string(),
            json!({ "från": nuvarande, "till": start }),
        )
        .await?;
    Ok(())
}

/// Raderar ett utkast och loggar `fakturautkast raderat` med vem, när och en sammanfattning:
/// kund, fakturadatum, summa och antal rader. En utfärdad faktura raderas aldrig (L1, A1).
pub async fn radera_utkast(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    id: KundfakturaId,
) -> Result<(), KundfakturaFel> {
    let mut ärende =
        Ärende::börja(db, klocka, aktör, företag, Handling::HanteraFakturautkast).await?;
    kräv_utkast(&mut ärende, id).await?;
    let utkast = läs_utkast(ärende.conn(), id).await?;
    let kundnamn = kundens_namn(ärende.conn(), utkast.kund).await?;
    let summa = utkast.som_formulär().beräkna().total;
    sqlx::query("DELETE FROM kundfakturarad WHERE faktura_id = ?")
        .bind(id.0)
        .execute(ärende.conn())
        .await?;
    sqlx::query("DELETE FROM kundfaktura WHERE id = ? AND status = 'utkast'")
        .bind(id.0)
        .execute(ärende.conn())
        .await?;
    ärende
        .logga(
            Händelse::FakturautkastRaderat,
            if kundnamn.is_empty() {
                "kundfaktura utan kund"
            } else {
                &kundnamn
            },
            json!({
                "sort": "kundfaktura",
                "kundfaktura": id.0,
                "kund": utkast.kund,
                "kundnamn": kundnamn,
                "fakturadatum": utkast.fakturadatum.map(|d| d.to_string()),
                "summa_öre": summa,
                "antal_rader": utkast.rader.len(),
            }),
        )
        .await?;
    ärende.slutför().await?;
    Ok(())
}

/// Utfärdar utkastet: nästa nummer i fakturaserien, kund- och företagsuppgifterna inkopierade,
/// PDF:en skriven och arkiverad som underlag, verifikatet bokfört med PDF:en som underlag och
/// utfärdandet loggat, i en transaktion. Under kontantmetoden på fakturadatumet utfärdas
/// fakturan inte än (#149).
pub async fn utfärda(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    id: KundfakturaId,
) -> Result<Utfärdad, KundfakturaFel> {
    let mut ärende = Ärende::börja(db, klocka, aktör, företag, Handling::UtfärdaFaktura).await?;
    kräv_utkast(&mut ärende, id).await?;
    let (u, säljare) = utfärdbar(ärende.conn(), företag, id).await?;
    if bokföringsmetod(ärende.conn(), företag, u.fakturadatum).await? != Bokföringsmetod::Faktura
    {
        return Err(KundfakturaFel::Kontantmetoden);
    }
    let logotyp = fakturauppgifter::logotypen_i(ärende.conn(), företag).await?;
    let (nu, utfärdare) = (ärende.nu, ärende.aktör);
    let pdf = skriv_pdf(&u, &säljare, logotyp, false, nu, ärende.conn(), &utfärdare).await?;
    let arkiverad =
        underlag::ladda_upp_i(&mut ärende, &pdf.filnamn, "application/pdf", &pdf.innehåll).await?;
    let bokfört = verifikat::bokför_i_ärende(
        &mut ärende,
        &verifikat::formulär_av(u.fakturadatum, &u.beskrivning(), &u.kontering()),
        &[arkiverad.id],
    )
    .await?;

    // Raderna får sitt netto innan fakturan låses.
    for (i, r) in u.rader.iter().enumerate() {
        sqlx::query(
            "UPDATE kundfakturarad SET beskrivning = ?, netto = ? WHERE faktura_id = ? AND radnr = ?",
        )
        .bind(&r.beskrivning)
        .bind(r.netto)
        .bind(id.0)
        .bind(i as i64 + 1)
        .execute(ärende.conn())
        .await?;
    }
    // Startnumret som serien började på, så att det syns och är låst efter utfärdandet.
    sqlx::query(
        "INSERT INTO fakturaserie (foretag_id, startnummer)
         SELECT ?1, ?2 WHERE NOT EXISTS (SELECT 1 FROM fakturaserie WHERE foretag_id = ?1)",
    )
    .bind(företag.0)
    .bind(u.nummer as i64)
    .execute(ärende.conn())
    .await?;
    let (nu, kanal, av) = (
        ärende.nu.to_string(),
        ärende.aktör.kanal.namn(),
        ärende.aktör.användare.map(|a| a.0),
    );
    sqlx::query(
        "UPDATE kundfaktura SET status = 'utfardad', nummer = ?, referens = ?,
             leveransdatum = ?, forfallodag = ?, er_referens = ?,
             kunduppgifter = ?, saljaruppgifter = ?, netto = ?, moms = ?, total = ?,
             underlag_id = ?, verifikat_id = ?, utfardad = ?, utfardad_kanal = ?, utfardad_av = ?
         WHERE id = ? AND status = 'utkast'",
    )
    .bind(u.nummer as i64)
    .bind(&u.referens)
    .bind(&u.leveransdatum)
    .bind(u.förfallodag.to_string())
    .bind(&u.er_referens)
    .bind(serde_json::to_string(&u.kund).expect("kunden som JSON"))
    .bind(serde_json::to_string(&säljare).expect("säljaren som JSON"))
    .bind(u.netto)
    .bind(u.moms)
    .bind(u.total)
    .bind(arkiverad.id.0)
    .bind(bokfört.id.0)
    .bind(nu)
    .bind(kanal)
    .bind(av)
    .bind(id.0)
    .execute(ärende.conn())
    .await?;
    ärende
        .logga(
            Händelse::KundfakturaUtfärdad,
            &u.nummer.to_string(),
            json!({
                "kundfaktura": id.0,
                "nummer": u.nummer,
                "referens": u.referens,
                "kund": { "id": u.kund.id, "namn": u.kund.namn },
                "fakturadatum": u.fakturadatum.to_string(),
                "förfallodag": u.förfallodag.to_string(),
                "netto_öre": u.netto,
                "moms_öre": u.moms,
                "summa_öre": u.total,
                "verifikat": { "id": bokfört.id.0, "nummer": bokfört.nummer.to_string() },
                "pdf": {
                    "underlag": arkiverad.id.0,
                    "filnamn": pdf.filnamn,
                    "sha256": arkiverad.sha256,
                },
            }),
        )
        .await?;
    ärende.slutför().await?;
    Ok(Utfärdad {
        id,
        nummer: u.nummer,
        verifikat: bokfört,
        underlag: arkiverad.id,
    })
}

/// PDF:en som utkastet skulle få om det utfärdades nu, med numret det skulle få, märkt
/// UTKAST. Den arkiveras inte.
pub async fn förhandsvisning(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    id: KundfakturaId,
) -> Result<Pdf, KundfakturaFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    let status = status(&mut tx, företag, id).await?;
    if status != Status::Utkast {
        return Err(KundfakturaFel::IngetUtkast);
    }
    let (u, säljare) = utfärdbar(&mut tx, företag, id).await?;
    let logotyp = fakturauppgifter::logotypen_i(&mut tx, företag).await?;
    skriv_pdf(&u, &säljare, logotyp, true, klocka.nu(), &mut tx, aktör).await
}

/// Utkastet kontrollerat mot det som gäller i transaktionen.
async fn utfärdbar(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    id: KundfakturaId,
) -> Result<(Utfärdbar, Säljare), KundfakturaFel> {
    let förutsättningar = läs_förutsättningar(&mut *conn, företag).await?;
    let formulär = läs_utkast(&mut *conn, id).await?.som_formulär();
    let u = formulär
        .kontrollera(&förutsättningar)
        .map_err(KundfakturaFel::Spärrat)?;
    Ok((u, förutsättningar.säljare))
}

async fn skriv_pdf(
    u: &Utfärdbar,
    säljare: &Säljare,
    logotyp: Option<fakturauppgifter::Logotypfil>,
    utkast: bool,
    nu: Timestamp,
    conn: &mut SqliteConnection,
    aktör: &Aktör,
) -> Result<Pdf, KundfakturaFel> {
    let sidfot = maya_pdf::Sidfot {
        tidpunkt: nu,
        användare: verifikat::namn(conn, aktör).await?,
    };
    let dokument = maya_pdf::Dokument::Kundfaktura(maya_pdf::Kundfaktura {
        faktura: u.clone(),
        säljare: säljare.clone(),
        logotyp: logotyp.map(|l| maya_pdf::Logotyp {
            mime: l.mime,
            innehåll: l.innehåll,
        }),
        utkast,
    });
    Ok(Pdf {
        filnamn: format!("Kundfaktura_{}.pdf", u.nummer),
        innehåll: maya_pdf::skriv(&dokument, &sidfot)?,
    })
}

/// Bokföringsmetoden på dagen: den senaste som gäller från dagen eller tidigare.
async fn bokföringsmetod(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    dag: Date,
) -> Result<Bokföringsmetod, DatabasFel> {
    let metoder = foretag::läs_företag(conn, företag).await?.bokföringsmetoder;
    metoder
        .iter()
        .rev()
        .find(|(från, _)| *från <= dag)
        .or(metoder.first())
        .map(|(_, metod)| *metod)
        .ok_or_else(|| DatabasFel::Korrupt("bokforingsmetod".into()))
}

/// Att `id` är ett utkast i ärendets företag.
async fn kräv_utkast(ärende: &mut Ärende, id: KundfakturaId) -> Result<(), KundfakturaFel> {
    let företag = ärende.företag;
    match status(ärende.conn(), företag, id).await {
        Ok(Status::Utkast) => Ok(()),
        Ok(_) | Err(KundfakturaFel::FinnsInte) => Err(KundfakturaFel::IngetUtkast),
        Err(e) => Err(e),
    }
}

async fn status(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    id: KundfakturaId,
) -> Result<Status, KundfakturaFel> {
    let kod: Option<String> =
        sqlx::query_scalar("SELECT status FROM kundfaktura WHERE id = ? AND foretag_id = ?")
            .bind(id.0)
            .bind(företag.0)
            .fetch_optional(conn)
            .await?;
    let kod = kod.ok_or(KundfakturaFel::FinnsInte)?;
    Ok(Status::från_kod(&kod).ok_or_else(|| DatabasFel::Korrupt("kundfaktura.status".into()))?)
}

type LagradRad = (
    String,
    Option<i64>,
    String,
    Option<i64>,
    i64,
    i64,
    Option<i64>,
);

/// Det sparade utkastet (eller den utfärdade fakturans fält) som det lagrades.
async fn läs_utkast(conn: &mut SqliteConnection, id: KundfakturaId) -> Result<Utkast, DatabasFel> {
    let (kund, fakturadatum, leveransdatum, förfallodag, er_referens): (
        Option<i64>,
        Option<String>,
        String,
        Option<String>,
        String,
    ) = sqlx::query_as(
        "SELECT kund_id, fakturadatum, leveransdatum, forfallodag, er_referens
         FROM kundfaktura WHERE id = ?",
    )
    .bind(id.0)
    .fetch_one(&mut *conn)
    .await?;
    let rader: Vec<LagradRad> = sqlx::query_as(
        "SELECT beskrivning, antal, enhet, a_pris, rabatt, momssats, konto
         FROM kundfakturarad WHERE faktura_id = ? ORDER BY radnr",
    )
    .bind(id.0)
    .fetch_all(&mut *conn)
    .await?;
    let korrupt = |vad: &str| DatabasFel::Korrupt(format!("kundfaktura {}: {vad}", id.0));
    Ok(Utkast {
        kund,
        fakturadatum: fakturadatum.as_deref().map(datum).transpose()?,
        leveransdatum,
        förfallodag: förfallodag.as_deref().map(datum).transpose()?,
        er_referens,
        startnummer: None,
        rader: rader
            .into_iter()
            .map(|(beskrivning, antal, enhet, à_pris, rabatt, sats, konto)| {
                Ok(Utkastrad {
                    beskrivning,
                    antal: antal.map(Antal::från_hundradelar),
                    enhet,
                    à_pris,
                    rabatt: Rabatt::från_hundradelar(rabatt).ok_or_else(|| korrupt("rabatt"))?,
                    momssats: u8::try_from(sats)
                        .ok()
                        .and_then(Momssats::från_procent)
                        .ok_or_else(|| korrupt("momssats"))?,
                    konto: konto.map(kontonummer).transpose()?,
                })
            })
            .collect::<Result<_, DatabasFel>>()?,
    })
}

async fn kundens_namn(
    conn: &mut SqliteConnection,
    kund: Option<i64>,
) -> Result<String, sqlx::Error> {
    let Some(kund) = kund else {
        return Ok(String::new());
    };
    Ok(
        sqlx::query_scalar::<_, String>("SELECT namn FROM kund WHERE id = ?")
            .bind(kund)
            .fetch_optional(conn)
            .await?
            .unwrap_or_default(),
    )
}

/// Företagets kundfakturor: utkasten först, sedan de utfärdade med det högsta numret först.
pub async fn lista(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
) -> Result<Vec<Kundfakturarad>, ÅtkomstFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    type Rad = (
        i64,
        String,
        Option<i64>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<i64>,
    );
    let rader: Vec<Rad> = sqlx::query_as(
        "SELECT f.id, f.status, f.nummer,
                coalesce(f.kunduppgifter ->> '$.namn', k.namn),
                f.fakturadatum, f.forfallodag, f.total
         FROM kundfaktura f LEFT JOIN kund k ON k.id = f.kund_id
         WHERE f.foretag_id = ?
         ORDER BY f.status = 'utkast' DESC, f.nummer DESC, f.id DESC",
    )
    .bind(företag.0)
    .fetch_all(&mut *tx)
    .await?;
    let mut lista = Vec::new();
    for (id, status, nummer, kund, fakturadatum, förfallodag, total) in rader {
        let status = Status::från_kod(&status)
            .ok_or_else(|| DatabasFel::Korrupt("kundfaktura.status".into()))?;
        let total = match total {
            Some(total) => total,
            None => {
                läs_utkast(&mut tx, KundfakturaId(id))
                    .await?
                    .som_formulär()
                    .beräkna()
                    .total
            }
        };
        lista.push(Kundfakturarad {
            id: KundfakturaId(id),
            status,
            nummer: nummer.map(i64::unsigned_abs),
            kund: kund.unwrap_or_default(),
            fakturadatum: fakturadatum.as_deref().map(datum).transpose()?,
            förfallodag: förfallodag.as_deref().map(datum).transpose()?,
            total,
        });
    }
    Ok(lista)
}

/// En kundfaktura i företaget med formulär, uppgifterna från utfärdandet och historik.
pub async fn kundfakturan(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    id: KundfakturaId,
) -> Result<Kundfaktura, KundfakturaFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    let status = status(&mut tx, företag, id).await?;
    type Huvud = (
        String,
        String,
        Option<i64>,
        Option<String>,
        Option<i64>,
        Option<String>,
        Option<String>,
        Option<String>,
    );
    let (
        skapad,
        skapad_kanal,
        skapad_av,
        skapad_namn,
        kund_id,
        kunduppgifter,
        saljaruppgifter,
        utfardad,
    ): Huvud = sqlx::query_as(
        "SELECT f.skapad, f.skapad_kanal, f.skapad_av, a.visningsnamn, f.kund_id,
                    f.kunduppgifter, f.saljaruppgifter, f.utfardad
             FROM kundfaktura f LEFT JOIN anvandare a ON a.id = f.skapad_av WHERE f.id = ?",
    )
    .bind(id.0)
    .fetch_one(&mut *tx)
    .await?;
    let utkast = läs_utkast(&mut tx, id).await?;
    let mut formulär = utkast.som_formulär();
    let utfärdning = match (status, kunduppgifter, saljaruppgifter, utfardad) {
        (Status::Utfärdad, Some(kund), Some(säljare), Some(_)) => {
            Some(läs_utfärdning(&mut tx, id, &kund, &säljare).await?)
        }
        (Status::Utfärdad, ..) => {
            return Err(DatabasFel::Korrupt("kundfaktura: utfärdandet".into()).into());
        }
        _ => None,
    };
    if let Some(u) = &utfärdning {
        formulär.leveransdatum = u.leveransdatum.clone();
    }
    let kund = match &utfärdning {
        Some(u) => u.kund.namn.clone(),
        None => kundens_namn(&mut tx, kund_id).await?,
    };
    let mut historik = Vec::new();
    for post in behandlingshistorik::läs_om(&mut tx, företag, "kundfaktura", id.0).await? {
        let namn = verifikat::namn(&mut tx, &post.aktör).await?;
        historik.push(Historikpost { post, namn });
    }
    Ok(Kundfaktura {
        id,
        status,
        kund,
        formulär,
        utfärdad: utfärdning,
        skapad: signatur(&skapad, &skapad_kanal, skapad_av, skapad_namn)?,
        historik,
    })
}

async fn läs_utfärdning(
    conn: &mut SqliteConnection,
    id: KundfakturaId,
    kund: &str,
    säljare: &str,
) -> Result<Utfärdning, KundfakturaFel> {
    let korrupt = |vad: &str| DatabasFel::Korrupt(format!("kundfaktura {}: {vad}", id.0));
    type Rad = (
        i64,
        String,
        String,
        String,
        String,
        String,
        i64,
        i64,
        i64,
        i64,
        Option<i64>,
        String,
        String,
        Option<i64>,
        Option<String>,
    );
    let (
        nummer,
        referens,
        fakturadatum,
        leveransdatum,
        förfallodag,
        er_referens,
        netto,
        moms,
        total,
        underlag_id,
        verifikat_id,
        utfardad,
        utfardad_kanal,
        utfardad_av,
        utfardad_namn,
    ): Rad = sqlx::query_as(
        "SELECT f.nummer, f.referens, f.fakturadatum, f.leveransdatum, f.forfallodag,
                f.er_referens, f.netto, f.moms, f.total, f.underlag_id, f.verifikat_id,
                f.utfardad, f.utfardad_kanal, f.utfardad_av, a.visningsnamn
         FROM kundfaktura f LEFT JOIN anvandare a ON a.id = f.utfardad_av WHERE f.id = ?",
    )
    .bind(id.0)
    .fetch_one(&mut *conn)
    .await?;
    type UtfärdadRad = (String, i64, String, i64, i64, i64, i64, i64);
    let rader: Vec<UtfärdadRad> = sqlx::query_as(
        "SELECT beskrivning, antal, enhet, a_pris, rabatt, momssats, konto, netto
         FROM kundfakturarad WHERE faktura_id = ? ORDER BY radnr",
    )
    .bind(id.0)
    .fetch_all(&mut *conn)
    .await?;
    let rader = rader
        .into_iter()
        .map(
            |(beskrivning, antal, enhet, à_pris, rabatt, sats, konto, netto)| {
                Ok(Fakturarad {
                    beskrivning,
                    antal: Antal::från_hundradelar(antal),
                    enhet,
                    à_pris,
                    rabatt: Rabatt::från_hundradelar(rabatt).ok_or_else(|| korrupt("rabatt"))?,
                    momssats: u8::try_from(sats)
                        .ok()
                        .and_then(Momssats::från_procent)
                        .ok_or_else(|| korrupt("momssats"))?,
                    konto: kontonummer(konto)?,
                    netto,
                })
            },
        )
        .collect::<Result<Vec<_>, DatabasFel>>()?;
    let (filnamn, sha256, storlek, arkiverad): (String, String, i64, String) =
        sqlx::query_as("SELECT filnamn, sha256, storlek, uppladdad FROM underlag WHERE id = ?")
            .bind(underlag_id)
            .fetch_one(&mut *conn)
            .await?;
    let verifikat_id = verifikat_id.ok_or_else(|| korrupt("verifikat"))?;
    let verifikat = verifikat::länk(&mut *conn, Some(verifikat_id))
        .await?
        .ok_or_else(|| korrupt("verifikat"))?;
    Ok(Utfärdning {
        nummer: nummer.unsigned_abs(),
        referens,
        kund: serde_json::from_str(kund).map_err(|_| korrupt("kunduppgifter"))?,
        säljare: serde_json::from_str(säljare).map_err(|_| korrupt("saljaruppgifter"))?,
        fakturadatum: datum(&fakturadatum)?,
        leveransdatum,
        förfallodag: datum(&förfallodag)?,
        er_referens,
        momsrader: momsrader(rader.iter().map(|r| (r.momssats, r.netto))),
        rader,
        netto,
        moms,
        total,
        verifikat,
        pdf: ArkiveradPdf {
            underlag: UnderlagId(underlag_id),
            filnamn,
            sha256,
            storlek: storlek.unsigned_abs(),
            arkiverad: arkiverad.parse().map_err(|_| korrupt("arkiverad"))?,
        },
        utfärdad: signatur(&utfardad, &utfardad_kanal, utfardad_av, utfardad_namn)?,
    })
}

/// Fakturan som verifikatet bokfördes från, för länken från verifikatet (story 73).
pub(crate) async fn fakturan_för_verifikat(
    conn: &mut SqliteConnection,
    verifikat: VerifikatId,
) -> Result<Option<(i64, String)>, DatabasFel> {
    let rad: Option<(i64, i64, String)> = sqlx::query_as(
        "SELECT id, nummer, kunduppgifter ->> '$.namn' FROM kundfaktura
         WHERE verifikat_id = ? AND status <> 'utkast'",
    )
    .bind(verifikat.0)
    .fetch_optional(conn)
    .await?;
    Ok(rad.map(|(id, nummer, kund)| (id, format!("kundfaktura {nummer} till {kund}"))))
}

fn kontonummer(nummer: i64) -> Result<Kontonummer, DatabasFel> {
    u16::try_from(nummer)
        .ok()
        .and_then(Kontonummer::new)
        .ok_or_else(|| DatabasFel::Korrupt(format!("ett kontonummer: {nummer}")))
}
