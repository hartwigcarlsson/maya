//! Kundfakturor som skapas i Maya: utkast, utfärdande och läsning (#135).
//!
//! Ett utkast kan ändras och raderas fritt; raderingen loggas med en sammanfattning. Att
//! *utfärda* gör i en transaktion: prövar spärrarna i [`maya_domain::kundfaktura`] mot det som
//! gäller i transaktionen, ger fakturan nästa nummer i företagets obrutna fakturaserie,
//! kopierar in kundens och företagets uppgifter, skriver PDF:en en gång och arkiverar den som
//! underlag med SHA-256 (VF5), bokför verifikatet daterat på fakturadatumet med PDF:en som
//! underlag (fakturametoden) och loggar utfärdandet (H1). Schemat avvisar att en utfärdad
//! faktura, dess nummer eller PDF ändras (L1).
//!
//! En ny kund som läggs upp på utkastet ("+ Ny kund…", #137) har status `ny` och syns inte i
//! kundregistret. Den blir verklig i utfärdandets transaktion och raderas, med Maya som aktör,
//! när inget utkast längre hänvisar till den.
//!
//! En *kreditfaktura* (#140) är en kundfaktura i samma tabell och serie, med en hänvisning till
//! originalet: [`kreditera`] skapar utkastet ur ett utfärdat original, och [`utfärda`] utfärdar
//! det som en faktura. Hänvisningen, originalets momsläge och kund och det som finns kvar att
//! kreditera läses ur databasen, aldrig ur formuläret.

use std::fmt;
use std::str::FromStr;

use jiff::Timestamp;
use jiff::civil::Date;
use maya_domain::faktura::{Antal, Momsrad, Momssats, Rabatt, momsrader};
use maya_domain::kontoplan::Kontonummer;
use maya_domain::kreditfaktura::{Hänvisning, Kreditspärr, NyKreditering};
use maya_domain::kund::OmvändSkattskyldighet;
use maya_domain::kundfaktura::{
    Fakturakund, Fakturarad, Förutsättningar, Momsläge, NyKundfaktura, Spärr, Säljare, Utfärdbar,
    Utkast, Utkastrad,
};
use serde_json::json;
use sqlx::SqliteConnection;

use crate::aktor::{Aktör, Handling};
use crate::atkomst::{self, Ärende, ÅtkomstFel};
use crate::behandlingshistorik::{self, Händelse};
use crate::betalning::Faktura;
use crate::fordran;
use crate::foretag::{self, FöretagsId, datum};
use crate::kund::{self, KundId};
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
    /// En extern kundfaktura som är registrerad (#143).
    Registrerad,
    /// En extern kundfaktura vars registrering är vänd med Rätta registrering och som skrivs
    /// av igen (#143).
    Upplåst,
}

impl Status {
    pub(crate) fn från_kod(kod: &str) -> Option<Self> {
        match kod {
            "utkast" => Some(Status::Utkast),
            "utfardad" => Some(Status::Utfärdad),
            "registrerad" => Some(Status::Registrerad),
            "upplast" => Some(Status::Upplåst),
            _ => None,
        }
    }

    /// Fakturan är utfärdad eller registrerad: dess fordran står i reskontran och den kan
    /// betalas, krediteras och skrivas av (#143).
    pub fn i_reskontran(self) -> bool {
        matches!(self, Status::Utfärdad | Status::Registrerad)
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
    /// Valet i panelen Kreditfaktura… klarar inte spärrarna (#140).
    #[error("kreditfakturan kan inte skapas")]
    Kreditering(Vec<Kreditspärr>),
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
    /// Utfärdandets verifikat; `None` när inget bokfördes (kontantmetoden, #149).
    pub verifikat: Option<Bokfört>,
    /// Den arkiverade PDF:en.
    pub underlag: UnderlagId,
}

/// En kundfaktura i listan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kundfakturarad {
    pub id: KundfakturaId,
    pub status: Status,
    /// Numret i fakturaserien; `None` för ett utkast och en extern faktura.
    pub nummer: Option<u64>,
    /// Numret som fakturan visar: numret i serien, eller det andra systemets för en extern
    /// faktura (#143). Tomt för ett utkast till Mayas egen faktura.
    pub fakturanummer: String,
    /// Fakturan har skapats i ett annat system (#143).
    pub är_extern: bool,
    /// Kundens namn, som det kopierades in vid utfärdandet; tomt för ett utkast utan kund.
    pub kund: String,
    pub fakturadatum: Option<Date>,
    pub förfallodag: Option<Date>,
    /// Totalen i öre; för ett utkast summan av de rader som går att räkna.
    pub total: i64,
    /// Kunden är ny och läggs i registret när fakturan utfärdas (#137).
    pub ny_kund: bool,
    /// Det som återstår att betala i öre (#139); för ett utkast totalen. En kreditfaktura
    /// räknas av mot originalet och har 0 kvar (#140); negativt är ett tillgodohavande.
    pub återstår: i64,
    /// En kundförlust som inte är återkallad gäller fakturan (#141).
    pub kundförlust: bool,
    /// Originalets nummer när fakturan är en kreditfaktura (#140, extern #143).
    pub krediterar: Option<String>,
    /// Det som utfärdade kreditfakturor har krediterat av fakturan, i öre och positivt (#140).
    pub krediterat: i64,
}

/// En utfärdad kreditfaktura på originalets dokument (#140).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kreditlänk {
    pub id: KundfakturaId,
    pub nummer: u64,
    pub fakturadatum: Date,
    /// Kreditfakturans total, negativ.
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
    /// Momsläget som fakturan utfärdades med.
    pub momsläge: Momsläge,
    /// Originalet när fakturan är en kreditfaktura (#140).
    pub krediterar: Option<Hänvisning>,
    /// Utfärdandets verifikat; `None` när inget bokfördes (kontantmetoden, #149).
    pub verifikat: Option<verifikat::Länk>,
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
    /// De utfärdade kreditfakturorna som krediterar fakturan, i nummerordning (#140).
    pub krediteras_av: Vec<Kreditlänk>,
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
    type Kundrad = (i64, String, String, String, String, String, i64, String);
    let kunder: Vec<Kundrad> = sqlx::query_as(
        "SELECT id, namn, identitet, adress, epost, momsregnr, betalningsvillkor,
                omvand_skattskyldighet
         FROM kund
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
                |(id, namn, identitet, adress, epost, momsregnr, villkor, omvänd)| {
                    Ok(Fakturakund {
                        id,
                        namn,
                        identitet,
                        adress,
                        epost,
                        momsregnr,
                        betalningsvillkor: u16::try_from(villkor).unwrap_or(u16::MAX),
                        omvänd_skattskyldighet: OmvändSkattskyldighet::från_kod(&omvänd)
                            .ok_or_else(|| {
                                DatabasFel::Korrupt("kund.omvand_skattskyldighet".into())
                            })?,
                    })
                },
            )
            .collect::<Result<_, DatabasFel>>()?,
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
    let mut formulär = formulär.clone();
    if let Some(id) = utkast {
        kräv_utkast(&mut ärende, id).await?;
        // Ett kreditutkast behåller originalets hänvisning, kund och momsläge (#140).
        if let Some(h) = utkastets_hänvisning(ärende.conn(), företag, id).await? {
            formulär.kund = h.kund.id.to_string();
            formulär.ny_kund = Default::default();
            formulär.omvänd_skattskyldighet = h.momsläge.omvänd().kod().to_string();
            formulär.startnummer = String::new();
            formulär.krediterar = Some(h);
        }
    }
    let formulär = &formulär;
    let mut u = formulär.som_utkast().map_err(KundfakturaFel::Spärrat)?;
    // Utan fri momssats (omvänd skattskyldighet, eller ett företag som inte är
    // momsregistrerat på fakturadatumet) sparas varje rad med 0 %.
    if !formulär
        .momsläge(&läs_förutsättningar(ärende.conn(), företag).await?)
        .fri_momssats()
    {
        for r in &mut u.rader {
            r.momssats = Momssats::Noll;
        }
    }
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
    // En ny kund sparas med utkastet; den som utkastet redan har skrivs om.
    let kund_id = match &u.ny_kund {
        Some(ny) => {
            let nuvarande = match utkast {
                Some(id) => ny_kund_på(&mut ärende, id).await?,
                None => None,
            };
            Some(kund::spara_ny_i(&mut ärende, nuvarande, ny).await?.0)
        }
        None => u.kund,
    };
    let (nu, kanal, av) = (
        ärende.nu.to_string(),
        ärende.aktör.kanal.namn(),
        ärende.aktör.användare.map(|a| a.0),
    );
    let id = match utkast {
        Some(id) => {
            sqlx::query(
                "UPDATE kundfaktura SET kund_id = ?, fakturadatum = ?, leveransdatum = ?,
                     forfallodag = ?, er_referens = ?, omvand_skattskyldighet = ?
                 WHERE id = ? AND status = 'utkast'",
            )
            .bind(kund_id)
            .bind(u.fakturadatum.map(|d| d.to_string()))
            .bind(&u.leveransdatum)
            .bind(u.förfallodag.map(|d| d.to_string()))
            .bind(&u.er_referens)
            .bind(u.omvänd_skattskyldighet.kod())
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
                      er_referens, omvand_skattskyldighet, skapad, skapad_kanal, skapad_av)
                 VALUES (?, 'utkast', ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
            )
            .bind(företag.0)
            .bind(kund_id)
            .bind(u.fakturadatum.map(|d| d.to_string()))
            .bind(&u.leveransdatum)
            .bind(u.förfallodag.map(|d| d.to_string()))
            .bind(&u.er_referens)
            .bind(u.omvänd_skattskyldighet.kod())
            .bind(nu)
            .bind(kanal)
            .bind(av)
            .fetch_one(ärende.conn())
            .await?,
        ),
    };
    skriv_rader(ärende.conn(), id, &u.rader).await?;
    kund::rensa_nya_i(&mut ärende).await?;
    ärende.slutför().await?;
    Ok(id)
}

/// Den nya kunden som utkastet `id` hänvisar till, om den har en.
pub(crate) async fn ny_kund_på(
    ärende: &mut Ärende,
    id: KundfakturaId,
) -> Result<Option<KundId>, sqlx::Error> {
    let kund: Option<i64> = sqlx::query_scalar(
        "SELECT k.id FROM kundfaktura f JOIN kund k ON k.id = f.kund_id
         WHERE f.id = ? AND k.status = 'ny'",
    )
    .bind(id.0)
    .fetch_optional(ärende.conn())
    .await?;
    Ok(kund.map(KundId))
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
    let (kund_id, kundnamn) = match &utkast.ny_kund {
        Some(ny) => (
            ny_kund_på(&mut ärende, id).await?.map(|k| k.0),
            ny.namn.clone(),
        ),
        None => (utkast.kund, kundens_namn(ärende.conn(), utkast.kund).await?),
    };
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
                "kund": kund_id,
                "kundnamn": kundnamn,
                "ny_kund": utkast.ny_kund.is_some(),
                "fakturadatum": utkast.fakturadatum.map(|d| d.to_string()),
                "summa_öre": summa,
                "antal_rader": utkast.rader.len(),
            }),
        )
        .await?;
    kund::rensa_nya_i(&mut ärende).await?;
    ärende.slutför().await?;
    Ok(())
}

/// Utfärdar utkastet: nästa nummer i fakturaserien, kund- och företagsuppgifterna inkopierade,
/// PDF:en skriven och arkiverad som underlag, verifikatet bokfört med PDF:en som underlag och
/// utfärdandet loggat, i en transaktion. Under kontantmetoden på fakturadatumet bokförs inget
/// (#149): fordran är inte bokförd, och betalningen bokför intäkten och momsen. En kreditfaktura
/// speglar originalet när originalets fordran är bokförd, och bokför annars bara den del som
/// överstiger det som var obetalt av originalet.
pub async fn utfärda(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    id: KundfakturaId,
) -> Result<Utfärdad, KundfakturaFel> {
    let mut ärende = Ärende::börja(db, klocka, aktör, företag, Handling::UtfärdaFaktura).await?;
    kräv_utkast(&mut ärende, id).await?;
    let (mut u, säljare) = utfärdbar(ärende.conn(), företag, id).await?;
    // En ny kund från utkastet blir verklig i samma transaktion, och fakturan får dess id.
    if let Some(ny) = &u.ny_kund {
        let kund_id = ny_kund_på(&mut ärende, id)
            .await?
            .ok_or_else(|| DatabasFel::Korrupt(format!("kundfaktura {}: ny kund", id.0)))?;
        kund::gör_verklig_i(&mut ärende, kund_id, ny).await?;
        u.kund.id = kund_id.0;
    }
    let logotyp = fakturauppgifter::logotypen_i(ärende.conn(), företag).await?;
    let (nu, utfärdare) = (ärende.nu, ärende.aktör);
    let pdf = skriv_pdf(&u, &säljare, logotyp, false, nu, ärende.conn(), &utfärdare).await?;
    let arkiverad =
        underlag::ladda_upp_i(&mut ärende, &pdf.filnamn, "application/pdf", &pdf.innehåll).await?;
    let (kontering, fordran_bokförs) = bokföringen(ärende.conn(), företag, &u).await?;
    let bokfört = match kontering {
        Some(rader) => Some(
            verifikat::bokför_i_ärende(
                &mut ärende,
                &verifikat::formulär_av(u.fakturadatum, &u.beskrivning(), &rader),
                &[arkiverad.id],
            )
            .await?,
        ),
        None => None,
    };

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
             omvand_skattskyldighet = ?, momsregistrerat = ?,
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
    .bind(u.momsläge.omvänd().kod())
    .bind(u.momsläge != Momsläge::UtanMoms)
    .bind(arkiverad.id.0)
    .bind(bokfört.map(|b| b.id.0))
    .bind(nu)
    .bind(kanal)
    .bind(av)
    .bind(id.0)
    .execute(ärende.conn())
    .await?;
    if let (Some(b), true) = (bokfört, fordran_bokförs) {
        fordran::bokför_i(ärende.conn(), företag, Faktura::Kund(id), b.id).await?;
    }
    let händelse = match u.krediterar {
        Some(_) => Händelse::KreditfakturaUtfärdad,
        None => Händelse::KundfakturaUtfärdad,
    };
    let mut post = json!({
                "kundfaktura": id.0,
                "nummer": u.nummer,
                "referens": u.referens,
                "kund": { "id": u.kund.id, "namn": u.kund.namn },
                "fakturadatum": u.fakturadatum.to_string(),
                "förfallodag": u.förfallodag.to_string(),
                "netto_öre": u.netto,
                "moms_öre": u.moms,
                "summa_öre": u.total,
                "verifikat": bokfört.map(|b| json!({ "id": b.id.0, "nummer": b.nummer.to_string() })),
                "pdf": {
                    "underlag": arkiverad.id.0,
                    "filnamn": pdf.filnamn,
                    "sha256": arkiverad.sha256,
                },
    });
    // #140: kreditfakturans post syns också i originalets historik.
    if let Some(h) = &u.krediterar {
        post["krediterar"] = json!(h.faktura);
        post["krediterar_nummer"] = json!(h.nummer);
    }
    ärende.logga(händelse, &u.nummer.to_string(), post).await?;
    ärende.slutför().await?;
    Ok(Utfärdad {
        id,
        nummer: u.nummer,
        verifikat: bokfört,
        underlag: arkiverad.id,
    })
}

/// Utfärdandets kontering och om den bokför fakturans fordran (#149). Under fakturametoden på
/// fakturadatumet bokförs 1510 mot intäkt och moms. En kreditfaktura följer originalets
/// fordran: är den bokförd speglar krediten originalet, annars bokförs bara den del av krediten
/// som överstiger det obetalda, mot 1510, och kreditfakturan bokför ingen egen fordran.
async fn bokföringen(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    u: &Utfärdbar,
) -> Result<(Option<Vec<maya_domain::verifikat::Rad>>, bool), KundfakturaFel> {
    let Some(h) = &u.krediterar else {
        return Ok(
            if fordran::fakturametoden_på(&mut *conn, företag, u.fakturadatum).await? {
                (Some(u.kontering()), true)
            } else {
                (None, false)
            },
        );
    };
    let kontering = fordran::kreditens_kontering(
        conn,
        Faktura::Kund(KundfakturaId(h.faktura)),
        u.total,
        u.kontering(),
    )
    .await?;
    Ok((kontering, false))
}

/// Det som utfärdandet av utkastet skulle bokföra nu, för dialogen Granska och utfärda: `None`
/// när inget bokförs (kontantmetoden på fakturadatumet, eller en kredit som ryms i det
/// obetalda av ett original vars fordran inte är bokförd, #149).
pub async fn utfärdandets_kontering(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    id: KundfakturaId,
) -> Result<Option<Vec<maya_domain::verifikat::Rad>>, KundfakturaFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    if status(&mut tx, företag, id).await? != Status::Utkast {
        return Err(KundfakturaFel::IngetUtkast);
    }
    let (u, _) = utfärdbar(&mut tx, företag, id).await?;
    Ok(bokföringen(&mut tx, företag, &u).await?.0)
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

/// Utkastet kontrollerat mot det som gäller i transaktionen, med säljarens uppgifter som
/// fakturan har dem.
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
    let säljare = u.säljare_på_fakturan(&förutsättningar.säljare);
    Ok((u, säljare))
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
        filnamn: match u.krediterar {
            Some(_) => format!("Kreditfaktura_{}.pdf", u.nummer),
            None => format!("Kundfaktura_{}.pdf", u.nummer),
        },
        innehåll: maya_pdf::skriv(&dokument, &sidfot)?,
    })
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
        // #143: en extern faktura har sina egna användningsfall i `extern_kundfaktura`.
        sqlx::query_scalar(
            "SELECT status FROM kundfaktura WHERE id = ? AND foretag_id = ? AND extern = 0",
        )
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
    type Huvud = (
        i64,
        Option<i64>,
        Option<String>,
        String,
        Option<String>,
        String,
        String,
        Option<i64>,
    );
    let (företag, kund, fakturadatum, leveransdatum, förfallodag, er_referens, omvänd, original): Huvud = sqlx::query_as(
        "SELECT foretag_id, kund_id, fakturadatum, leveransdatum, forfallodag, er_referens,
                omvand_skattskyldighet, krediterar
         FROM kundfaktura WHERE id = ?",
    )
    .bind(id.0)
    .fetch_one(&mut *conn)
    .await?;
    // En ny kund från utkastet läses som utkastets egen, inte som en kund ur registret.
    let ny_kund = match kund {
        Some(k) => kund::ny_i(&mut *conn, FöretagsId(företag), KundId(k)).await?,
        None => None,
    };
    let kund = if ny_kund.is_some() { None } else { kund };
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
        ny_kund,
        fakturadatum: fakturadatum.as_deref().map(datum).transpose()?,
        leveransdatum,
        förfallodag: förfallodag.as_deref().map(datum).transpose()?,
        er_referens,
        startnummer: None,
        omvänd_skattskyldighet: OmvändSkattskyldighet::från_kod(&omvänd)
            .ok_or_else(|| korrupt("omvand_skattskyldighet"))?,
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
        krediterar: match original {
            Some(o) => Some(
                hänvisning_i(&mut *conn, FöretagsId(företag), o)
                    .await?
                    .ok_or_else(|| korrupt("krediterar"))?,
            ),
            None => None,
        },
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
    Ok(lista_i(&mut tx, företag, None).await?)
}

/// Kundens fakturor i företaget, i samma ordning som [`lista`], för kundens detalj (story 11).
pub async fn kundens_fakturor(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    kund: KundId,
) -> Result<Vec<Kundfakturarad>, ÅtkomstFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    Ok(lista_i(&mut tx, företag, Some(kund)).await?)
}

pub(crate) async fn lista_i(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    kund: Option<KundId>,
) -> Result<Vec<Kundfakturarad>, DatabasFel> {
    type Rad = (
        i64,
        String,
        Option<i64>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<i64>,
        bool,
        Option<i64>,
        bool,
        Option<String>,
        i64,
        bool,
        String,
    );
    // Det som återstår: totalen minus krediteringarna (#140), betalningarna i reskontran
    // (#139) och det som en gällande kundförlust skrev av (#141), samma regel som
    // `betalning::återstår_i`. En kreditfaktura räknas av mot originalet och har inget kvar.
    let rader: Vec<Rad> = sqlx::query_as(
        "SELECT f.id, f.status, f.nummer,
                coalesce(f.kunduppgifter ->> '$.namn', k.namn),
                f.fakturadatum, f.forfallodag, f.total, coalesce(k.status = 'ny', 0),
                CASE WHEN f.krediterar IS NOT NULL THEN 0 ELSE
                    f.total - coalesce((SELECT sum(b.belopp + b.oresutjamning)
                                        FROM betalning_i_reskontran b
                                        WHERE b.kundfaktura_id = f.id), 0)
                            + coalesce((SELECT sum(c.total) FROM kundfaktura c
                                        WHERE c.krediterar = f.id
                                          AND c.status IN ('utfardad', 'registrerad')), 0)
                            - coalesce((SELECT sum(l.belopp) FROM kundforlust_i_reskontran l
                                        WHERE l.kundfaktura_id = f.id), 0)
                END,
                EXISTS (SELECT 1 FROM kundforlust_i_reskontran l WHERE l.kundfaktura_id = f.id),
                (SELECT coalesce(CAST(o.nummer AS TEXT), o.fakturanummer) FROM kundfaktura o
                 WHERE o.id = f.krediterar),
                -coalesce((SELECT sum(c.total) FROM kundfaktura c
                           WHERE c.krediterar = f.id
                             AND c.status IN ('utfardad', 'registrerad')), 0),
                f.extern, coalesce(CAST(f.nummer AS TEXT), f.fakturanummer)
         FROM kundfaktura f LEFT JOIN kund k ON k.id = f.kund_id
         WHERE f.foretag_id = ?1 AND (?2 IS NULL OR f.kund_id = ?2)
         ORDER BY f.status = 'utkast' DESC, f.nummer DESC, f.id DESC",
    )
    .bind(företag.0)
    .bind(kund.map(|k| k.0))
    .fetch_all(&mut *conn)
    .await?;
    let mut lista = Vec::new();
    for (
        id,
        status,
        nummer,
        kund,
        fakturadatum,
        förfallodag,
        total,
        ny_kund,
        återstår,
        kundförlust,
        krediterar,
        krediterat,
        är_extern,
        fakturanummer,
    ) in rader
    {
        let status = Status::från_kod(&status)
            .ok_or_else(|| DatabasFel::Korrupt("kundfaktura.status".into()))?;
        let total = match total {
            Some(total) => total,
            // #143: ett externt utkast har belopp per rad.
            None if är_extern => {
                crate::extern_kundfaktura::utkastets_total_i(&mut *conn, KundfakturaId(id)).await?
            }
            None => {
                läs_utkast(&mut *conn, KundfakturaId(id))
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
            fakturanummer,
            är_extern,
            kund: kund.unwrap_or_default(),
            fakturadatum: fakturadatum.as_deref().map(datum).transpose()?,
            förfallodag: förfallodag.as_deref().map(datum).transpose()?,
            total,
            ny_kund,
            // Ett kreditutkast räknas inte av mot originalet förrän det är utfärdat.
            återstår: match status {
                Status::Utkast => total,
                _ => återstår.unwrap_or(total),
            },
            kundförlust,
            krediterar,
            krediterat,
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
    // #140: kreditfakturornas utfärdande syns också i originalets historik.
    let poster =
        behandlingshistorik::läs_om_någon(&mut tx, företag, ["kundfaktura", "krediterar"], id.0)
            .await?;
    for post in poster {
        let namn = verifikat::namn(&mut tx, &post.aktör).await?;
        historik.push(Historikpost { post, namn });
    }
    let krediteras_av = krediteringar(&mut tx, id).await?;
    Ok(Kundfaktura {
        id,
        status,
        kund,
        formulär,
        utfärdad: utfärdning,
        skapad: signatur(&skapad, &skapad_kanal, skapad_av, skapad_namn)?,
        historik,
        krediteras_av,
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
    // En faktura utfärdad före #136 har momsregistrerat NULL och utfärdades med moms.
    type Moms = (String, Option<bool>, i64, Option<i64>);
    let (omvänd, momsregistrerat, företag, original): Moms = sqlx::query_as(
        "SELECT omvand_skattskyldighet, momsregistrerat, foretag_id, krediterar
         FROM kundfaktura WHERE id = ?",
    )
    .bind(id.0)
    .fetch_one(&mut *conn)
    .await?;
    let momsläge =
        momsläge(&omvänd, momsregistrerat).ok_or_else(|| korrupt("omvand_skattskyldighet"))?;
    let krediterar = match original {
        Some(o) => Some(
            hänvisning_i(&mut *conn, FöretagsId(företag), o)
                .await?
                .ok_or_else(|| korrupt("krediterar"))?,
        ),
        None => None,
    };
    let rader = läs_utfärdade_rader(&mut *conn, id).await?;
    let (filnamn, sha256, storlek, arkiverad): (String, String, i64, String) =
        sqlx::query_as("SELECT filnamn, sha256, storlek, uppladdad FROM underlag WHERE id = ?")
            .bind(underlag_id)
            .fetch_one(&mut *conn)
            .await?;
    let verifikat = verifikat::länk(&mut *conn, verifikat_id).await?;
    if verifikat_id.is_some() && verifikat.is_none() {
        return Err(korrupt("verifikat").into());
    }
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
        momsläge,
        krediterar,
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

/// Fakturan som verifikatet bokfördes från, för länken från verifikatet (story 73): utfärdandets
/// verifikat, eller rättelsen och det nya verifikatet från Rätta kontering (#142).
pub(crate) async fn fakturan_för_verifikat(
    conn: &mut SqliteConnection,
    verifikat: VerifikatId,
) -> Result<Option<(i64, String)>, DatabasFel> {
    // #143: en extern fakturas registreringar och deras rättelser länkar också hit.
    let rad: Option<(i64, String, String, bool, bool)> = sqlx::query_as(
        "SELECT id, coalesce(CAST(nummer AS TEXT), fakturanummer), kunduppgifter ->> '$.namn',
                krediterar IS NOT NULL, extern
         FROM kundfaktura
         WHERE status <> 'utkast' AND (verifikat_id = ?1 OR id IN (
             SELECT faktura_id FROM kundfaktura_kontering
             WHERE rattelse_id = ?1 OR verifikat_id = ?1
             UNION ALL
             SELECT faktura_id FROM kundfaktura_registrering
             WHERE rattelse_id = ?1 OR verifikat_id = ?1))",
    )
    .bind(verifikat.0)
    .fetch_optional(conn)
    .await?;
    Ok(rad.map(|(id, nummer, kund, kredit, är_extern)| {
        let sort = match (är_extern, kredit) {
            (false, true) => "kreditfaktura",
            (false, false) => "kundfaktura",
            (true, true) => "extern kreditfaktura",
            (true, false) => "extern kundfaktura",
        };
        (id, format!("{sort} {nummer} till {kund}"))
    }))
}

/// Momsläget som en utfärdad faktura bär: utan moms när företaget inte var momsregistrerat,
/// annars efter den omvända skattskyldigheten. En faktura utfärdad före #136 har
/// `momsregistrerat` NULL och utfärdades med moms.
fn momsläge(omvänd: &str, momsregistrerat: Option<bool>) -> Option<Momsläge> {
    match momsregistrerat {
        Some(false) => Some(Momsläge::UtanMoms),
        _ => OmvändSkattskyldighet::från_kod(omvänd).map(Momsläge::av),
    }
}

/// Skriver utkastets rader i ordning.
async fn skriv_rader(
    conn: &mut SqliteConnection,
    id: KundfakturaId,
    rader: &[Utkastrad],
) -> Result<(), sqlx::Error> {
    for (i, r) in rader.iter().enumerate() {
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
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

// #140 Kreditfakturor

/// Det som utfärdade kreditfakturor har krediterat av fakturan `faktura`, i öre och positivt.
pub(crate) async fn krediterat_i(
    conn: &mut SqliteConnection,
    faktura: KundfakturaId,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT -coalesce(sum(total), 0) FROM kundfaktura
         WHERE krediterar = ? AND status IN ('utfardad', 'registrerad')",
    )
    .bind(faktura.0)
    .fetch_one(conn)
    .await
}

/// Hänvisningen till originalet `original`, om det är en utfärdad faktura i företaget som inte
/// själv är en kreditfaktura: numret, datumet, momsläget, kunden som den kopierades in och det
/// som finns kvar att kreditera.
pub(crate) async fn hänvisning_i(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    original: i64,
) -> Result<Option<Hänvisning>, DatabasFel> {
    type Rad = (i64, String, String, i64, String, Option<bool>);
    let rad: Option<Rad> = sqlx::query_as(
        "SELECT nummer, fakturadatum, kunduppgifter, total, omvand_skattskyldighet,
                momsregistrerat
         FROM kundfaktura
         WHERE id = ? AND foretag_id = ? AND status <> 'utkast' AND krediterar IS NULL
           AND extern = 0",
    )
    .bind(original)
    .bind(företag.0)
    .fetch_optional(&mut *conn)
    .await?;
    let Some((nummer, fakturadatum, kund, total, omvänd, momsregistrerat)) = rad else {
        return Ok(None);
    };
    let korrupt = |vad: &str| DatabasFel::Korrupt(format!("kundfaktura {original}: {vad}"));
    let krediterat = krediterat_i(&mut *conn, KundfakturaId(original)).await?;
    Ok(Some(Hänvisning {
        faktura: original,
        nummer: nummer.unsigned_abs(),
        fakturadatum,
        momsläge: momsläge(&omvänd, momsregistrerat)
            .ok_or_else(|| korrupt("omvand_skattskyldighet"))?,
        kund: serde_json::from_str(&kund).map_err(|_| korrupt("kunduppgifter"))?,
        total,
        kvar: total - krediterat,
    }))
}

/// Hänvisningen när utkastet `id` är en kreditfaktura.
async fn utkastets_hänvisning(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    id: KundfakturaId,
) -> Result<Option<Hänvisning>, DatabasFel> {
    let original: Option<i64> =
        sqlx::query_scalar("SELECT krediterar FROM kundfaktura WHERE id = ?")
            .bind(id.0)
            .fetch_one(&mut *conn)
            .await?;
    match original {
        Some(o) => Ok(Some(hänvisning_i(conn, företag, o).await?.ok_or_else(
            || DatabasFel::Korrupt(format!("kundfaktura {}: krediterar", id.0)),
        )?)),
        None => Ok(None),
    }
}

/// De utfärdade kreditfakturorna som krediterar `id`, i nummerordning.
async fn krediteringar(
    conn: &mut SqliteConnection,
    id: KundfakturaId,
) -> Result<Vec<Kreditlänk>, DatabasFel> {
    let rader: Vec<(i64, i64, String, i64)> = sqlx::query_as(
        "SELECT id, nummer, fakturadatum, total FROM kundfaktura
         WHERE krediterar = ? AND status <> 'utkast' ORDER BY nummer",
    )
    .bind(id.0)
    .fetch_all(&mut *conn)
    .await?;
    rader
        .into_iter()
        .map(|(id, nummer, fakturadatum, total)| {
            Ok(Kreditlänk {
                id: KundfakturaId(id),
                nummer: nummer.unsigned_abs(),
                fakturadatum: datum(&fakturadatum)?,
                total,
            })
        })
        .collect()
}

/// *Kreditfaktura…* (berättelse 37): skapar ett utkast till kreditfaktura för den utfärdade
/// fakturan `original`, med hela fakturan eller antalet per rad i `kreditering`. Raderna får
/// originalets beskrivning, à-pris, rabatt, momssats och det intäktskonto som gäller nu (efter
/// en Rätta kontering, #142), med negativt antal. Utkastet dateras i dag och utfärdas med
/// [`utfärda`]. En kreditfaktura kan inte själv krediteras.
pub async fn kreditera(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    original: KundfakturaId,
    kreditering: &NyKreditering,
) -> Result<KundfakturaId, KundfakturaFel> {
    let mut ärende =
        Ärende::börja(db, klocka, aktör, företag, Handling::HanteraFakturautkast).await?;
    let h = hänvisning_i(ärende.conn(), företag, original.0)
        .await?
        .ok_or(KundfakturaFel::FinnsInte)?;
    let (leveransdatum, er_referens): (String, String) =
        sqlx::query_as("SELECT leveransdatum, er_referens FROM kundfaktura WHERE id = ?")
            .bind(original.0)
            .fetch_one(ärende.conn())
            .await?;
    let gällande = crate::kontering::gällande_i(ärende.conn(), företag, original).await?;
    let mut rader = läs_utfärdade_rader(ärende.conn(), original).await?;
    for (rad, g) in rader.iter_mut().zip(&gällande.rader) {
        rad.konto = g.konto;
    }
    let idag = maya_domain::tid::datum(ärende.nu);
    let mut formulär = kreditering
        .utkast(&h, &rader, idag)
        .map_err(KundfakturaFel::Kreditering)?;
    formulär.leveransdatum = leveransdatum;
    formulär.er_referens = er_referens;
    let u = formulär.som_utkast().map_err(KundfakturaFel::Spärrat)?;
    let (nu, kanal, av) = (
        ärende.nu.to_string(),
        ärende.aktör.kanal.namn(),
        ärende.aktör.användare.map(|a| a.0),
    );
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO kundfaktura
             (foretag_id, status, kund_id, fakturadatum, leveransdatum, forfallodag,
              er_referens, omvand_skattskyldighet, krediterar, skapad, skapad_kanal, skapad_av)
         VALUES (?, 'utkast', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(företag.0)
    .bind(h.kund.id)
    .bind(u.fakturadatum.map(|d| d.to_string()))
    .bind(&u.leveransdatum)
    .bind(u.förfallodag.map(|d| d.to_string()))
    .bind(&u.er_referens)
    .bind(u.omvänd_skattskyldighet.kod())
    .bind(original.0)
    .bind(nu)
    .bind(kanal)
    .bind(av)
    .fetch_one(ärende.conn())
    .await?;
    skriv_rader(ärende.conn(), KundfakturaId(id), &u.rader).await?;
    ärende.slutför().await?;
    Ok(KundfakturaId(id))
}

/// Raderna på en utfärdad faktura, med kontot som det utfärdades.
async fn läs_utfärdade_rader(
    conn: &mut SqliteConnection,
    id: KundfakturaId,
) -> Result<Vec<Fakturarad>, DatabasFel> {
    let korrupt = |vad: &str| DatabasFel::Korrupt(format!("kundfaktura {}: {vad}", id.0));
    type UtfärdadRad = (String, i64, String, i64, i64, i64, i64, i64);
    let rader: Vec<UtfärdadRad> = sqlx::query_as(
        "SELECT beskrivning, antal, enhet, a_pris, rabatt, momssats, konto, netto
         FROM kundfakturarad WHERE faktura_id = ? ORDER BY radnr",
    )
    .bind(id.0)
    .fetch_all(&mut *conn)
    .await?;
    rader
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
        .collect()
}

pub(crate) fn kontonummer(nummer: i64) -> Result<Kontonummer, DatabasFel> {
    u16::try_from(nummer)
        .ok()
        .and_then(Kontonummer::new)
        .ok_or_else(|| DatabasFel::Korrupt(format!("ett kontonummer: {nummer}")))
}
