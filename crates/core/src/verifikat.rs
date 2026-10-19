//! Verifikat: att registrera och bokföra ett verifikat, och att läsa dem.
//!
//! *Bokföra* godkänner, låser och numrerar i ett steg (ADR 0001). Spärrarna prövas i `domain`
//! med samma kod som formuläret, mot kontoplanen och räkenskapsåren som de är i
//! bokföringstransaktionen. Numret är nästa i räkenskapsårets V-serie och tilldelas i samma
//! transaktion (`BEGIN IMMEDIATE`), så samtidiga bokföringar ger varken dubbletter eller luckor
//! (VF3). Ett bokfört verifikat har ingen kodväg som ändrar eller raderar det (L1), och schemat
//! avvisar det också.

use std::fmt;
use std::str::FromStr;

use jiff::Timestamp;
use jiff::civil::Date;
use maya_domain::belopp::Belopp;
use maya_domain::kontoplan::Kontonummer;
use maya_domain::rakenskapsar::{Period, Räkenskapsår};
use maya_domain::rattelse::{Original, Rättelsespärr};
use maya_domain::sok::{Fråga, Sökbart};
use maya_domain::verifikat::{
    BokförbartVerifikat, Förutsättningar, NyttVerifikat, Spärr, Årsval
};
use sqlx::SqliteConnection;

use crate::aktor::{Aktör, AnvändarId, Handling, Kanal};
use crate::atkomst::{self, Ärende, ÅtkomstFel};
use crate::behandlingshistorik::{self, Händelse, Post};
use crate::foretag::{self, FöretagsId, datum};
use crate::underlag::{self, UnderlagId, VerifikatetsUnderlag};
use crate::{Databas, DatabasFel, Klocka};

/// Ett verifikats id i installationen. Vem som helst kan skriva ett id; `core` avgör om aktören
/// har åtkomst till verifikatets företag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VerifikatId(pub(crate) i64);

impl fmt::Display for VerifikatId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for VerifikatId {
    type Err = std::num::ParseIntError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse().map(VerifikatId)
    }
}

/// Ett verifikationsnummer i räkenskapsårets V-serie, skrivet V1, V2 …
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Verifikationsnummer(pub(crate) u32);

impl Verifikationsnummer {
    pub fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for Verifikationsnummer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "V{}", self.0)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum VerifikatFel {
    /// Spärrarna i `domain`, med samma koder som formuläret visar.
    #[error("verifikatet kan inte bokföras")]
    Spärrat(Vec<Spärr>),
    /// Inget utkast med det id:t i företaget: det finns inte, eller är redan bokfört.
    #[error("utkastet finns inte")]
    IngetUtkast,
    /// Ett underlag som ska kopplas finns inte i företaget.
    #[error("underlaget finns inte")]
    OkäntUnderlag,
    /// Inget bokfört verifikat med det id:t i företaget.
    #[error("verifikatet finns inte")]
    IngetVerifikat,
    /// Verifikatet kan inte rättas: det är redan rättat eller är en rättelse.
    #[error("{0}")]
    InteRättbart(Rättelsespärr),
    #[error(transparent)]
    Åtkomst(#[from] ÅtkomstFel),
}

impl From<sqlx::Error> for VerifikatFel {
    fn from(e: sqlx::Error) -> Self {
        VerifikatFel::Åtkomst(e.into())
    }
}

impl From<DatabasFel> for VerifikatFel {
    fn from(e: DatabasFel) -> Self {
        VerifikatFel::Åtkomst(e.into())
    }
}

/// Resultatet av en bokföring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bokfört {
    pub id: VerifikatId,
    pub nummer: Verifikationsnummer,
}

/// Vem som gjorde något och när, med namnet som visas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signatur {
    pub tid: Timestamp,
    pub aktör: Aktör,
    /// Användarens visningsnamn, eller kanalens namn när ingen användare finns.
    pub namn: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rad {
    /// `None` bara i ett utkast.
    pub konto: Option<Kontonummer>,
    pub debet: Belopp,
    pub kredit: Belopp,
    pub text: Option<String>,
}

/// Var ett verifikat kommer ifrån.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Källa {
    Manuell,
    /// En rättelse av ett annat verifikat (R1).
    Rättelse,
}

/// En länk till ett annat verifikat i företaget, med vem som bokförde det och när.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Länk {
    pub id: VerifikatId,
    /// `None` för ett utkast.
    pub nummer: Option<Verifikationsnummer>,
    pub bokförd: Option<Signatur>,
}

/// Ett verifikat med rader och historik.
#[derive(Debug, Clone, PartialEq)]
pub struct Verifikat {
    pub id: VerifikatId,
    /// `None` för ett utkast.
    pub nummer: Option<Verifikationsnummer>,
    pub källa: Källa,
    /// Verifikatet som det här rättar, för en rättelse.
    pub rättar: Option<Länk>,
    /// Rättelsen av det här verifikatet, om det är rättat (R2).
    pub rättad_av: Option<Länk>,
    /// Verifikatet som det här är en kopia av, för ett utkast från "registrera om".
    pub kopia_av: Option<Länk>,
    pub datum: Option<Date>,
    pub beskrivning: String,
    pub räkenskapsår: Option<Räkenskapsår>,
    pub period: Option<Period>,
    pub rader: Vec<Rad>,
    /// Summan av debet.
    pub summa: Belopp,
    pub skapad: Signatur,
    /// `None` för ett utkast.
    pub bokförd: Option<Signatur>,
    /// Kopplade underlag, i den ordning de kopplades, med kompletteringar sist (VF2, VF5).
    pub underlag: Vec<VerifikatetsUnderlag>,
    /// Posterna i behandlingshistoriken som rör verifikatet, i ordning.
    pub historik: Vec<Historikpost>,
    /// Fakturan som verifikatet bokfördes från, när det kommer från ett av Mayas flöden.
    pub flöde: Option<Flödeslänk>,
}

/// Fakturaflödet som ett verifikat kommer ifrån.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flöde {
    /// Registreringen av en leverantörsfaktura (#145).
    Leverantörsfaktura,
    /// Utfärdandet av en kundfaktura (#135).
    Kundfaktura,
}

/// En länk från ett verifikat till fakturan som det bokfördes från.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flödeslänk {
    pub flöde: Flöde,
    /// Fakturans id i sitt flöde.
    pub id: i64,
    /// Fakturan i text, t.ex. "leverantörsfaktura 8812 från Kontorsvaror AB".
    pub text: String,
}

impl Verifikat {
    /// Verifikatet som original till en rättelse, med det som avgör om det kan rättas. `None`
    /// för ett utkast.
    pub fn som_original(&self) -> Option<Original> {
        let nummer = |l: &Option<Länk>| {
            l.as_ref()
                .map(|l| l.nummer.map(|n| n.to_string()).unwrap_or_default())
        };
        Some(Original {
            nummer: self.nummer?.to_string(),
            datum: self.datum?,
            rader: self
                .rader
                .iter()
                .map(|r| {
                    Some(maya_domain::verifikat::Rad {
                        konto: r.konto?,
                        debet: r.debet,
                        kredit: r.kredit,
                        text: r.text.clone(),
                    })
                })
                .collect::<Option<_>>()?,
            rättad_av: nummer(&self.rättad_av),
            rättar: nummer(&self.rättar),
            flöde: self.flöde.as_ref().map(|f| f.text.clone()),
        })
    }
}

/// En post i verifikatets historik, med namnet på den som gjorde det.
#[derive(Debug, Clone, PartialEq)]
pub struct Historikpost {
    pub post: Post,
    /// Användarens visningsnamn, eller kanalens namn när ingen användare finns.
    pub namn: String,
}

/// Ett verifikat i listan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verifikatrad {
    pub id: VerifikatId,
    pub nummer: Option<Verifikationsnummer>,
    pub datum: Option<Date>,
    pub beskrivning: String,
    pub summa: Belopp,
}

/// Filtren i fliken Verifikat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Urval {
    #[default]
    Alla,
    Utkast,
    Bokförda,
}

impl Urval {
    pub fn kod(self) -> &'static str {
        match self {
            Urval::Alla => "alla",
            Urval::Utkast => "utkast",
            Urval::Bokförda => "bokforda",
        }
    }

    pub fn från_kod(kod: &str) -> Option<Self> {
        [Urval::Alla, Urval::Utkast, Urval::Bokförda]
            .into_iter()
            .find(|u| u.kod() == kod)
    }
}

/// Kontoplanen och räkenskapsåren som spärrarna prövas mot, för formuläret.
pub async fn förutsättningar(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
) -> Result<Förutsättningar, ÅtkomstFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    Ok(läs_förutsättningar(&mut tx, företag).await?)
}

pub(crate) async fn läs_förutsättningar(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
) -> Result<Förutsättningar, DatabasFel> {
    let konton: Vec<(i64, bool)> =
        sqlx::query_as("SELECT nummer, aktiv FROM konto WHERE foretag_id = ? ORDER BY nummer")
            .bind(företag.0)
            .fetch_all(&mut *conn)
            .await?;
    let år: Vec<(String, String)> =
        sqlx::query_as("SELECT start, slut FROM rakenskapsar WHERE foretag_id = ? ORDER BY start")
            .bind(företag.0)
            .fetch_all(&mut *conn)
            .await?;
    Ok(Förutsättningar {
        konton: konton
            .into_iter()
            .map(|(nummer, aktiv)| Ok((kontonummer(nummer)?, aktiv)))
            .collect::<Result<_, DatabasFel>>()?,
        räkenskapsår: år
            .iter()
            .map(|(start, slut)| Ok(Räkenskapsår::befintligt(datum(start)?, datum(slut)?)))
            .collect::<Result<_, DatabasFel>>()?,
    })
}

/// Nästa nummer i V-serien för varje räkenskapsår, för förhandsvisningen "Bokförs som V9" i
/// fakturaflödena. Numret tilldelas först vid bokföringen och kan hinna tas av någon annan.
pub async fn nästa_nummer(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
) -> Result<Vec<(Räkenskapsår, Verifikationsnummer)>, ÅtkomstFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    let rader: Vec<(String, String, i64)> = sqlx::query_as(
        "SELECT r.start, r.slut,
                coalesce((SELECT max(nummer) FROM verifikat v WHERE v.rakenskapsar_id = r.id), 0) + 1
         FROM rakenskapsar r WHERE r.foretag_id = ? ORDER BY r.start",
    )
    .bind(företag.0)
    .fetch_all(&mut *tx)
    .await?;
    Ok(rader
        .into_iter()
        .map(|(start, slut, nästa)| {
            Ok((
                Räkenskapsår::befintligt(datum(&start)?, datum(&slut)?),
                verifikationsnummer(nästa)?,
            ))
        })
        .collect::<Result<_, DatabasFel>>()?)
}

/// Registrerar och bokför ett verifikat i ett steg: prövar spärrarna, skapar räkenskapsåret
/// om datumet ligger i året direkt efter det senaste, tilldelar nästa nummer i årets V-serie
/// och loggar bokföringen med aktör och tid (H1), i en transaktion.
pub async fn bokför(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    formulär: &NyttVerifikat,
) -> Result<Bokfört, VerifikatFel> {
    let ärende = Ärende::börja(db, klocka, aktör, företag, Handling::Bokföra).await?;
    bokför_i(ärende, None, formulär, None).await
}

/// Bokför ett nytt verifikat eller utkastet `utkast`, som [`bokför`] och [`bokför_utkast`], med
/// exakt underlagen i `underlag` kopplade. Kopplingarna görs i bokföringstransaktionen och kan
/// sedan aldrig tas bort (ADR 0002).
pub async fn bokför_med_underlag(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    utkast: Option<VerifikatId>,
    formulär: &NyttVerifikat,
    underlag: &[UnderlagId],
) -> Result<Bokfört, VerifikatFel> {
    let mut ärende = Ärende::börja(db, klocka, aktör, företag, Handling::Bokföra).await?;
    if let Some(id) = utkast {
        kräv_utkast(&mut ärende, id).await?;
    }
    bokför_i(ärende, utkast, formulär, Some(underlag)).await
}

/// Bokför ett utkast med innehållet i formuläret, under samma spärrar som [`bokför`]. Utkastet
/// får sitt nummer först nu.
pub async fn bokför_utkast(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    utkast: VerifikatId,
    formulär: &NyttVerifikat,
) -> Result<Bokfört, VerifikatFel> {
    let mut ärende = Ärende::börja(db, klocka, aktör, företag, Handling::Bokföra).await?;
    kräv_utkast(&mut ärende, utkast).await?;
    bokför_i(ärende, Some(utkast), formulär, None).await
}

/// Bokför i ärendet och committar. `underlag` är `None` när kopplingarna ska lämnas som de är.
async fn bokför_i(
    mut ärende: Ärende,
    utkast: Option<VerifikatId>,
    formulär: &NyttVerifikat,
    underlag: Option<&[UnderlagId]>,
) -> Result<Bokfört, VerifikatFel> {
    let bokfört = bokför_utan_att_slutföra(&mut ärende, utkast, formulär, underlag).await?;
    ärende.slutför().await?;
    Ok(bokfört)
}

/// Bokför ett nytt verifikat inne i ärendet utan att committa, för fakturaflödena: fakturans
/// händelse och dess verifikat skapas i samma transaktion, så att reskontran och huvudboken
/// aldrig går isär. Raderna prövas med samma spärrar som ett manuellt verifikat, underlagen
/// kopplas före numreringen (inte som komplettering) och bokföringen loggas. Den som anropar
/// slutför ärendet.
pub(crate) async fn bokför_i_ärende(
    ärende: &mut Ärende,
    formulär: &NyttVerifikat,
    underlag: &[UnderlagId],
) -> Result<Bokfört, VerifikatFel> {
    bokför_utan_att_slutföra(ärende, None, formulär, Some(underlag)).await
}

/// Formuläret för ett verifikat med kontrollerade rader, t.ex. ett flödes kontering, så att
/// det går genom samma spärrar som ett verifikat som skrivs för hand.
pub(crate) fn formulär_av(
    datum: Date,
    beskrivning: &str,
    rader: &[maya_domain::verifikat::Rad],
) -> NyttVerifikat {
    let belopp = |b: Belopp| {
        if b == Belopp::NOLL {
            String::new()
        } else {
            b.to_string()
        }
    };
    NyttVerifikat {
        datum: datum.to_string(),
        beskrivning: beskrivning.to_string(),
        rader: rader
            .iter()
            .map(|r| maya_domain::verifikat::NyRad {
                konto: r.konto.to_string(),
                debet: belopp(r.debet),
                kredit: belopp(r.kredit),
                text: r.text.clone().unwrap_or_default(),
            })
            .collect(),
    }
}

async fn bokför_utan_att_slutföra(
    ärende: &mut Ärende,
    utkast: Option<VerifikatId>,
    formulär: &NyttVerifikat,
    underlag: Option<&[UnderlagId]>,
) -> Result<Bokfört, VerifikatFel> {
    let företag = ärende.företag;
    let förutsättningar = läs_förutsättningar(ärende.conn(), företag).await?;
    let v = formulär
        .kontrollera(&förutsättningar)
        .map_err(VerifikatFel::Spärrat)?;
    let år_id = räkenskapsår_id(ärende, v.räkenskapsår).await?;
    let innehåll = Innehåll::bokförbart(år_id, &v);
    let id = match utkast {
        Some(id) => {
            skriv_om(ärende, id, &innehåll).await?;
            id
        }
        None => skapa(ärende, &innehåll).await?,
    };
    if let Some(underlag) = underlag {
        koppla(ärende, id, underlag).await?;
    }
    let nummer = tilldela_nummer(ärende, id, år_id).await?;
    logga_bokfört(ärende, id, nummer, &v, None).await?;
    Ok(Bokfört { id, nummer })
}

/// Loggar att verifikatet `id` bokfördes (H1), med underlagen (VF2) och, för en rättelse,
/// verifikatet som det rättar.
async fn logga_bokfört(
    ärende: &mut Ärende,
    id: VerifikatId,
    nummer: Verifikationsnummer,
    v: &BokförbartVerifikat,
    rättar: Option<(VerifikatId, Verifikationsnummer)>,
) -> Result<(), VerifikatFel> {
    let kopplade = underlag::verifikatets(ärende.conn(), id).await?;
    let år = v.räkenskapsår.år();
    let mut beskrivning = serde_json::json!({
        "verifikat": id.0,
        "nummer": nummer.to_string(),
        "räkenskapsår": { "start": år.start().to_string(), "slut": år.slut().to_string() },
        "datum": v.datum.to_string(),
        "beskrivning": v.beskrivning,
        "summa_öre": v.summa().öre(),
        "rader": v.rader.iter().map(|r| serde_json::json!({
            "konto": r.konto.get(),
            "debet_öre": r.debet.öre(),
            "kredit_öre": r.kredit.öre(),
            "text": r.text,
        })).collect::<Vec<_>>(),
        "underlag": underlag::referenser(&kopplade),
    });
    if let Some((original, original_nummer)) = rättar {
        beskrivning["rättar"] = serde_json::json!({
            "verifikat": original.0,
            "nummer": original_nummer.to_string(),
        });
    }
    ärende
        .logga(Händelse::VerifikatBokfört, &nummer.to_string(), beskrivning)
        .await?;
    Ok(())
}

/// Vad som händer efter rättelsen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rättelseval {
    /// Bokför rättelsen och skapa i samma transaktion ett utkast med originalets rader och
    /// underlag, märkt *kopia av* originalet.
    RegistreraOm,
    /// Bara bokför rättelsen.
    BaraRättelse,
}

/// Resultatet av en rättelse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rättad {
    pub rättelse: Bokfört,
    /// Utkastet från "registrera om".
    pub kopia: Option<VerifikatId>,
}

/// Rättar det bokförda verifikatet `original` med ett nytt verifikat som vänder varje rad (R1),
/// daterat `datum` och med beskrivningen "Rättelse av V5[, <orsak>]". Originalet ändras inte
/// (L1); rättelsen pekar på det, och originalet visar rättelsen när det läses (R2, VF7). Ett
/// verifikat rättas högst en gång och en rättelse rättas inte. Med
/// [`Rättelseval::RegistreraOm`] skapas i samma transaktion ett utkast med originalets datum,
/// beskrivning, rader och underlag, märkt *kopia av* originalet.
#[allow(clippy::too_many_arguments)]
pub async fn rätta(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    original: VerifikatId,
    datum: &str,
    orsak: &str,
    val: Rättelseval,
) -> Result<Rättad, VerifikatFel> {
    let mut ärende = Ärende::börja(db, klocka, aktör, företag, Handling::Bokföra).await?;
    let (o, som_original) = läs_original(&mut ärende, original).await?;
    som_original
        .kan_rättas()
        .map_err(VerifikatFel::InteRättbart)?;
    let (rättelse, datum) = vänd_i(&mut ärende, &o, &som_original, datum, orsak).await?;

    let kopia = match val {
        Rättelseval::BaraRättelse => None,
        Rättelseval::RegistreraOm => {
            let år_id: Option<i64> =
                sqlx::query_scalar("SELECT rakenskapsar_id FROM verifikat WHERE id = ?")
                    .bind(original.0)
                    .fetch_one(ärende.conn())
                    .await?;
            let kopia = skapa(
                &mut ärende,
                &Innehåll {
                    år_id,
                    datum: o.datum,
                    beskrivning: &o.beskrivning,
                    rader: o
                        .rader
                        .iter()
                        .map(|r| (r.konto, r.debet, r.kredit, r.text.as_deref()))
                        .collect(),
                    källa: Källa::Manuell,
                    rättar: None,
                    kopia_av: Some(original),
                },
            )
            .await?;
            let underlag: Vec<UnderlagId> = o.underlag.iter().map(|u| u.id).collect();
            koppla(&mut ärende, kopia, &underlag).await?;
            Some(kopia)
        }
    };

    logga_rättat(&mut ärende, &o, rättelse, datum, orsak, kopia).await?;
    ärende.slutför().await?;
    Ok(Rättad { rättelse, kopia })
}

/// Rättar det bokförda verifikatet `original` inne i ärendet utan att committa, för
/// fakturaflödenas Rätta kontering, avskrift, registrering och betalning: en rättelse som vänder
/// varje rad (R1), daterad `datum`, bokförd och loggad som av [`rätta`] (R2, VF7). Spärren mot
/// verifikat från ett flöde gäller inte här, eftersom det är flödet som rättar; ett verifikat
/// rättas ändå högst en gång och en rättelse rättas inte. Flödet länkar rättelsen till sin
/// faktura, loggar sin egen händelse och slutför ärendet.
pub(crate) async fn rätta_i(
    ärende: &mut Ärende,
    original: VerifikatId,
    datum: Date,
    orsak: &str,
) -> Result<Bokfört, VerifikatFel> {
    let (o, som_original) = läs_original(ärende, original).await?;
    som_original
        .kan_rättas_i_flödet()
        .map_err(VerifikatFel::InteRättbart)?;
    let (rättelse, datum) = vänd_i(ärende, &o, &som_original, &datum.to_string(), orsak).await?;
    logga_rättat(ärende, &o, rättelse, datum, orsak, None).await?;
    Ok(rättelse)
}

/// Det bokförda verifikatet `id` i ärendets företag, också som original till en rättelse.
async fn läs_original(
    ärende: &mut Ärende,
    id: VerifikatId,
) -> Result<(Verifikat, Original), VerifikatFel> {
    let företag = ärende.företag;
    let o = läs_verifikat(ärende.conn(), företag, id)
        .await?
        .ok_or(VerifikatFel::IngetVerifikat)?;
    let som_original = o.som_original().ok_or(VerifikatFel::IngetVerifikat)?;
    Ok((o, som_original))
}

/// Bokför rättelsen av `o` daterad `datum`: prövar den, skapar räkenskapsåret vid behov,
/// numrerar och loggar bokföringen. Ger rättelsen och dess datum.
async fn vänd_i(
    ärende: &mut Ärende,
    o: &Verifikat,
    som_original: &Original,
    datum: &str,
    orsak: &str,
) -> Result<(Bokfört, Date), VerifikatFel> {
    let original_nummer = o.nummer.ok_or(VerifikatFel::IngetVerifikat)?;
    let företag = ärende.företag;
    let förutsättningar = läs_förutsättningar(ärende.conn(), företag).await?;
    let v = som_original
        .rättelse(datum, orsak, &förutsättningar)
        .map_err(VerifikatFel::Spärrat)?;
    let år_id = räkenskapsår_id(ärende, v.räkenskapsår).await?;
    let innehåll = Innehåll {
        källa: Källa::Rättelse,
        rättar: Some(o.id),
        ..Innehåll::bokförbart(år_id, &v)
    };
    let id = skapa(ärende, &innehåll).await?;
    let nummer = tilldela_nummer(ärende, id, år_id).await?;
    logga_bokfört(ärende, id, nummer, &v, Some((o.id, original_nummer))).await?;
    Ok((Bokfört { id, nummer }, v.datum))
}

/// Loggar att `o` rättades av `rättelse` (R2), med kopian från "registrera om".
async fn logga_rättat(
    ärende: &mut Ärende,
    o: &Verifikat,
    rättelse: Bokfört,
    datum: Date,
    orsak: &str,
    kopia: Option<VerifikatId>,
) -> Result<(), VerifikatFel> {
    let original_nummer = o.nummer.ok_or(VerifikatFel::IngetVerifikat)?;
    ärende
        .logga(
            Händelse::VerifikatRättat,
            &original_nummer.to_string(),
            serde_json::json!({
                "verifikat": o.id.0,
                "nummer": original_nummer.to_string(),
                "rättelse": { "verifikat": rättelse.id.0, "nummer": rättelse.nummer.to_string() },
                "datum": datum.to_string(),
                "orsak": orsak.trim(),
                "kopia": kopia.map(|k| k.0),
            }),
        )
        .await?;
    Ok(())
}

/// Sparar formuläret som ett nytt utkast, eller som nytt innehåll i utkastet `utkast`. Ett
/// utkast får vara obalanserat och ofullständigt och har inget nummer. Ett datum i året direkt
/// efter det senaste skapar året i samma transaktion, som när ett verifikat bokförs. Spärrar
/// bara det som inte går att lagra: ett ogiltigt datum, ett datum utan räkenskapsår, ett
/// kontonummer eller ett belopp som inte går att tolka.
pub async fn spara_utkast(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    utkast: Option<VerifikatId>,
    formulär: &NyttVerifikat,
) -> Result<VerifikatId, VerifikatFel> {
    spara_utkast_i(db, klocka, aktör, företag, utkast, formulär, None).await
}

/// Sparar som [`spara_utkast`], med exakt underlagen i `underlag` kopplade till utkastet: nya
/// kopplas, och de som inte längre finns med kopplas bort och blir okopplade igen.
pub async fn spara_utkast_med_underlag(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    utkast: Option<VerifikatId>,
    formulär: &NyttVerifikat,
    underlag: &[UnderlagId],
) -> Result<VerifikatId, VerifikatFel> {
    spara_utkast_i(db, klocka, aktör, företag, utkast, formulär, Some(underlag)).await
}

async fn spara_utkast_i(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    utkast: Option<VerifikatId>,
    formulär: &NyttVerifikat,
    underlag: Option<&[UnderlagId]>,
) -> Result<VerifikatId, VerifikatFel> {
    let mut ärende = Ärende::börja(db, klocka, aktör, företag, Handling::HanteraUtkast).await?;
    if let Some(id) = utkast {
        kräv_utkast(&mut ärende, id).await?;
    }
    let förutsättningar = läs_förutsättningar(ärende.conn(), företag).await?;
    let u = formulär
        .som_utkast(&förutsättningar)
        .map_err(VerifikatFel::Spärrat)?;
    let år_id = match u.räkenskapsår {
        Some(år) => Some(räkenskapsår_id(&mut ärende, år).await?),
        None => None,
    };
    let innehåll = Innehåll {
        år_id,
        datum: u.datum,
        beskrivning: &u.beskrivning,
        rader: u
            .rader
            .iter()
            .map(|r| (r.konto, r.debet, r.kredit, r.text.as_deref()))
            .collect(),
        källa: Källa::Manuell,
        rättar: None,
        kopia_av: None,
    };
    let id = match utkast {
        Some(id) => {
            skriv_om(&mut ärende, id, &innehåll).await?;
            id
        }
        None => skapa(&mut ärende, &innehåll).await?,
    };
    if let Some(underlag) = underlag {
        koppla(&mut ärende, id, underlag).await?;
    }
    ärende.slutför().await?;
    Ok(id)
}

/// Ger utkastet exakt underlagen i `underlag`. Kopplingar till ett utkast loggas inte; de blir
/// en del av bokföringen när utkastet bokförs.
async fn koppla(
    ärende: &mut Ärende,
    utkast: VerifikatId,
    underlag: &[UnderlagId],
) -> Result<(), VerifikatFel> {
    let företag = ärende.företag;
    let mut önskade: Vec<i64> = Vec::new();
    for u in underlag {
        if !önskade.contains(&u.0) {
            önskade.push(u.0);
        }
    }
    let finns: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM underlag
         WHERE foretag_id = ? AND id IN (SELECT value FROM json_each(?))",
    )
    .bind(företag.0)
    .bind(serde_json::json!(önskade).to_string())
    .fetch_one(ärende.conn())
    .await?;
    if finns != önskade.len() as i64 {
        return Err(VerifikatFel::OkäntUnderlag);
    }
    sqlx::query(
        "DELETE FROM underlagskoppling
         WHERE verifikat_id = ? AND underlag_id NOT IN (SELECT value FROM json_each(?))",
    )
    .bind(utkast.0)
    .bind(serde_json::json!(önskade).to_string())
    .execute(ärende.conn())
    .await?;
    let (aktör, nu) = (ärende.aktör, ärende.nu);
    for u in önskade {
        sqlx::query(
            "INSERT INTO underlagskoppling
                 (underlag_id, verifikat_id, kopplad, kopplad_kanal, kopplad_av, komplettering)
             VALUES (?, ?, ?, ?, ?, 0)
             ON CONFLICT DO NOTHING",
        )
        .bind(u)
        .bind(utkast.0)
        .bind(nu.to_string())
        .bind(aktör.kanal.namn())
        .bind(aktör.användare.map(|a| a.0))
        .execute(ärende.conn())
        .await?;
    }
    Ok(())
}

/// Raderar ett utkast och loggar raderingen med en sammanfattning: datum, beskrivning, summa
/// och antal rader. Ett bokfört verifikat raderas aldrig (L1).
pub async fn radera_utkast(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    utkast: VerifikatId,
) -> Result<(), VerifikatFel> {
    let mut ärende = Ärende::börja(db, klocka, aktör, företag, Handling::HanteraUtkast).await?;
    kräv_utkast(&mut ärende, utkast).await?;
    let (datum_, beskrivning, summa, antal): (Option<String>, String, i64, i64) = sqlx::query_as(
        "SELECT v.datum, v.beskrivning,
                    coalesce((SELECT sum(debet) FROM verifikatrad WHERE verifikat_id = v.id), 0),
                    (SELECT count(*) FROM verifikatrad WHERE verifikat_id = v.id)
             FROM verifikat v WHERE v.id = ?",
    )
    .bind(utkast.0)
    .fetch_one(ärende.conn())
    .await?;
    // Utkastets underlag blir okopplade; själva underlagen finns kvar.
    let kopplade = underlag::verifikatets(ärende.conn(), utkast).await?;
    sqlx::query("DELETE FROM underlagskoppling WHERE verifikat_id = ?")
        .bind(utkast.0)
        .execute(ärende.conn())
        .await?;
    sqlx::query("DELETE FROM verifikatrad WHERE verifikat_id = ?")
        .bind(utkast.0)
        .execute(ärende.conn())
        .await?;
    sqlx::query("DELETE FROM verifikat WHERE id = ? AND bokford IS NULL")
        .bind(utkast.0)
        .execute(ärende.conn())
        .await?;
    // Nyckeln är "utkast" och inte "verifikat": SQLite kan ge ett senare verifikat samma id,
    // och dess historik ska inte visa raderingen.
    ärende
        .logga(
            Händelse::UtkastRaderat,
            if beskrivning.trim().is_empty() {
                "utkast utan beskrivning"
            } else {
                &beskrivning
            },
            serde_json::json!({
                "utkast": utkast.0,
                "datum": datum_,
                "beskrivning": beskrivning,
                "summa_öre": summa,
                "antal_rader": antal,
                "underlag": underlag::referenser(&kopplade),
            }),
        )
        .await?;
    ärende.slutför().await?;
    Ok(())
}

/// Att `id` är ett utkast i ärendets företag.
async fn kräv_utkast(ärende: &mut Ärende, id: VerifikatId) -> Result<(), VerifikatFel> {
    let företag = ärende.företag;
    let finns: bool = sqlx::query_scalar(
        "SELECT EXISTS (
             SELECT 1 FROM verifikat WHERE id = ? AND foretag_id = ? AND bokford IS NULL)",
    )
    .bind(id.0)
    .bind(företag.0)
    .fetch_one(ärende.conn())
    .await?;
    if finns {
        Ok(())
    } else {
        Err(VerifikatFel::IngetUtkast)
    }
}

/// Räkenskapsårets id, efter att året har skapats om det är nytt.
async fn räkenskapsår_id(ärende: &mut Ärende, årsval: Årsval) -> Result<i64, VerifikatFel> {
    let företag = ärende.företag;
    match årsval {
        Årsval::Befintligt(år) => Ok(sqlx::query_scalar(
            "SELECT id FROM rakenskapsar WHERE foretag_id = ? AND start = ?",
        )
        .bind(företag.0)
        .bind(år.start().to_string())
        .fetch_one(ärende.conn())
        .await?),
        Årsval::Nytt(år) => {
            let id = foretag::lägg_till_räkenskapsår(
                ärende.conn(),
                företag,
                år.start(),
                år.slut(),
                &år.perioder(),
            )
            .await?;
            let (start, slut) = (år.start().to_string(), år.slut().to_string());
            ärende
                .logga(
                    Händelse::RäkenskapsårSkapat,
                    &format!("{start} – {slut}"),
                    serde_json::json!({
                        "start": start,
                        "slut": slut,
                        "skäl": "första verifikatet dateras i året",
                    }),
                )
                .await?;
            Ok(id)
        }
    }
}

/// Det som lagras om ett verifikat, för ett utkast eller ett verifikat som ska bokföras.
struct Innehåll<'a> {
    år_id: Option<i64>,
    datum: Option<Date>,
    beskrivning: &'a str,
    /// Konto, debet, kredit och text.
    rader: Vec<(Option<Kontonummer>, Belopp, Belopp, Option<&'a str>)>,
    /// Sätts när verifikatet skapas och ändras inte.
    källa: Källa,
    rättar: Option<VerifikatId>,
    kopia_av: Option<VerifikatId>,
}

impl<'a> Innehåll<'a> {
    /// Ett manuellt verifikat som klarar spärrarna, i räkenskapsåret `år_id`.
    fn bokförbart(år_id: i64, v: &'a BokförbartVerifikat) -> Self {
        Innehåll {
            år_id: Some(år_id),
            datum: Some(v.datum),
            beskrivning: &v.beskrivning,
            rader: v
                .rader
                .iter()
                .map(|r| (Some(r.konto), r.debet, r.kredit, r.text.as_deref()))
                .collect(),
            källa: Källa::Manuell,
            rättar: None,
            kopia_av: None,
        }
    }
}

/// Skapar verifikatet som utkast med sina rader.
async fn skapa(
    ärende: &mut Ärende, innehåll: &Innehåll<'_>
) -> Result<VerifikatId, sqlx::Error> {
    let (företag, aktör, nu) = (ärende.företag, ärende.aktör, ärende.nu);
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO verifikat
             (foretag_id, rakenskapsar_id, datum, beskrivning, kalla, skapad, skapad_kanal,
              skapad_av, rattar, kopia_av)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(företag.0)
    .bind(innehåll.år_id)
    .bind(innehåll.datum.map(|d| d.to_string()))
    .bind(innehåll.beskrivning)
    .bind(match innehåll.källa {
        Källa::Manuell => "manuell",
        Källa::Rättelse => "rattelse",
    })
    .bind(nu.to_string())
    .bind(aktör.kanal.namn())
    .bind(aktör.användare.map(|a| a.0))
    .bind(innehåll.rättar.map(|v| v.0))
    .bind(innehåll.kopia_av.map(|v| v.0))
    .fetch_one(ärende.conn())
    .await?;
    skriv_rader(ärende, id, innehåll).await?;
    Ok(VerifikatId(id))
}

/// Ger ett utkast nytt innehåll: datum, beskrivning och rader.
async fn skriv_om(
    ärende: &mut Ärende,
    id: VerifikatId,
    innehåll: &Innehåll<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE verifikat SET rakenskapsar_id = ?, datum = ?, beskrivning = ?
         WHERE id = ? AND bokford IS NULL",
    )
    .bind(innehåll.år_id)
    .bind(innehåll.datum.map(|d| d.to_string()))
    .bind(innehåll.beskrivning)
    .bind(id.0)
    .execute(ärende.conn())
    .await?;
    sqlx::query("DELETE FROM verifikatrad WHERE verifikat_id = ?")
        .bind(id.0)
        .execute(ärende.conn())
        .await?;
    skriv_rader(ärende, id.0, innehåll).await
}

async fn skriv_rader(
    ärende: &mut Ärende,
    id: i64,
    innehåll: &Innehåll<'_>,
) -> Result<(), sqlx::Error> {
    for (i, (konto, debet, kredit, text)) in innehåll.rader.iter().enumerate() {
        sqlx::query(
            "INSERT INTO verifikatrad (verifikat_id, radnr, konto, debet, kredit, text)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(i as i64 + 1)
        .bind(konto.map(|k| i64::from(k.get())))
        .bind(debet.öre())
        .bind(kredit.öre())
        .bind(text)
        .execute(ärende.conn())
        .await?;
    }
    Ok(())
}

/// Ger ett utkast nästa nummer i årets serie och bokför det, inom ärendets transaktion.
async fn tilldela_nummer(
    ärende: &mut Ärende,
    id: VerifikatId,
    år_id: i64,
) -> Result<Verifikationsnummer, sqlx::Error> {
    let (företag, aktör, nu) = (ärende.företag, ärende.aktör, ärende.nu);
    let nummer: i64 = sqlx::query_scalar(
        "SELECT coalesce(max(nummer), 0) + 1 FROM verifikat
         WHERE foretag_id = ? AND rakenskapsar_id = ?",
    )
    .bind(företag.0)
    .bind(år_id)
    .fetch_one(ärende.conn())
    .await?;
    sqlx::query(
        "UPDATE verifikat SET nummer = ?, bokford = ?, bokford_kanal = ?, bokford_av = ?
         WHERE id = ? AND bokford IS NULL",
    )
    .bind(nummer)
    .bind(nu.to_string())
    .bind(aktör.kanal.namn())
    .bind(aktör.användare.map(|a| a.0))
    .bind(id.0)
    .execute(ärende.conn())
    .await?;
    Ok(Verifikationsnummer(
        u32::try_from(nummer).expect("ett verifikationsnummer som ryms i u32"),
    ))
}

#[derive(sqlx::FromRow)]
struct VerifikatRad {
    id: i64,
    nummer: Option<i64>,
    datum: Option<String>,
    beskrivning: String,
    ar_start: Option<String>,
    ar_slut: Option<String>,
    skapad: String,
    skapad_kanal: String,
    skapad_av: Option<i64>,
    skapad_namn: Option<String>,
    bokford: Option<String>,
    bokford_kanal: Option<String>,
    bokford_av: Option<i64>,
    bokford_namn: Option<String>,
    kalla: String,
    rattar: Option<i64>,
    kopia_av: Option<i64>,
}

/// Ett verifikat i företaget, eller `None` om det inte finns där.
pub async fn verifikatet(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    id: VerifikatId,
) -> Result<Option<Verifikat>, ÅtkomstFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    Ok(läs_verifikat(&mut tx, företag, id).await?)
}

/// Ett verifikat i företaget med rader, underlag, länkar och historik, eller `None`.
pub(crate) async fn läs_verifikat(
    tx: &mut SqliteConnection,
    företag: FöretagsId,
    id: VerifikatId,
) -> Result<Option<Verifikat>, DatabasFel> {
    let Some(v): Option<VerifikatRad> = sqlx::query_as(
        "SELECT v.id, v.nummer, v.datum, v.beskrivning, r.start AS ar_start, r.slut AS ar_slut,
                v.skapad, v.skapad_kanal, v.skapad_av, s.visningsnamn AS skapad_namn,
                v.bokford, v.bokford_kanal, v.bokford_av, b.visningsnamn AS bokford_namn,
                v.kalla, v.rattar, v.kopia_av
         FROM verifikat v
         LEFT JOIN rakenskapsar r ON r.id = v.rakenskapsar_id
         LEFT JOIN anvandare s ON s.id = v.skapad_av
         LEFT JOIN anvandare b ON b.id = v.bokford_av
         WHERE v.foretag_id = ? AND v.id = ?",
    )
    .bind(företag.0)
    .bind(id.0)
    .fetch_optional(&mut *tx)
    .await?
    else {
        return Ok(None);
    };
    let källa = match v.kalla.as_str() {
        "manuell" => Källa::Manuell,
        "rattelse" => Källa::Rättelse,
        _ => return Err(DatabasFel::Korrupt("verifikat.kalla".into())),
    };
    let rättad_av: Option<i64> = sqlx::query_scalar("SELECT id FROM verifikat WHERE rattar = ?")
        .bind(id.0)
        .fetch_optional(&mut *tx)
        .await?;
    let rättar = länk(tx, v.rattar).await?;
    let rättad_av = länk(tx, rättad_av).await?;
    let kopia_av = länk(tx, v.kopia_av).await?;
    let rader: Vec<(Option<i64>, i64, i64, Option<String>)> = sqlx::query_as(
        "SELECT konto, debet, kredit, text FROM verifikatrad WHERE verifikat_id = ? ORDER BY radnr",
    )
    .bind(id.0)
    .fetch_all(&mut *tx)
    .await?;
    let rader = rader
        .into_iter()
        .map(|(konto, debet, kredit, text)| {
            Ok(Rad {
                konto: konto.map(kontonummer).transpose()?,
                debet: Belopp::från_öre(debet),
                kredit: Belopp::från_öre(kredit),
                text,
            })
        })
        .collect::<Result<Vec<_>, DatabasFel>>()?;
    let underlag = underlag::verifikatets(&mut *tx, id).await?;
    let flöde = flödeslänk(&mut *tx, id).await?;
    let mut historik = Vec::new();
    for post in behandlingshistorik::läs_om_verifikat(&mut *tx, företag, id.0).await? {
        let namn = namn(&mut *tx, &post.aktör).await?;
        historik.push(Historikpost { post, namn });
    }

    let datum_ = v.datum.as_deref().map(datum).transpose()?;
    let räkenskapsår = match (&v.ar_start, &v.ar_slut) {
        (Some(start), Some(slut)) => Some(Räkenskapsår::befintligt(datum(start)?, datum(slut)?)),
        _ => None,
    };
    let period = match (räkenskapsår, datum_) {
        (Some(år), Some(d)) => år
            .perioder()
            .into_iter()
            .find(|p| p.start <= d && d <= p.slut),
        _ => None,
    };
    let bokförd = match (v.bokford, v.bokford_kanal) {
        (Some(tid), Some(kanal)) => Some(signatur(&tid, &kanal, v.bokford_av, v.bokford_namn)?),
        _ => None,
    };
    Ok(Some(Verifikat {
        id: VerifikatId(v.id),
        nummer: v.nummer.map(verifikationsnummer).transpose()?,
        källa,
        rättar,
        rättad_av,
        kopia_av,
        datum: datum_,
        beskrivning: v.beskrivning,
        räkenskapsår,
        period,
        summa: Belopp::från_öre(rader.iter().map(|r| r.debet.öre()).sum()),
        rader,
        skapad: signatur(&v.skapad, &v.skapad_kanal, v.skapad_av, v.skapad_namn)?,
        bokförd,
        underlag,
        historik,
        flöde,
    }))
}

/// Fakturan som verifikatet bokfördes från, om det kommer från ett av fakturaflödena. Varje
/// flöde som bokför lägger till sin tabell här.
async fn flödeslänk(
    conn: &mut SqliteConnection,
    id: VerifikatId,
) -> Result<Option<Flödeslänk>, DatabasFel> {
    // #145 Leverantörsfakturor
    let leverantörsfaktura: Option<(i64, String, String)> = sqlx::query_as(
        "SELECT f.id, f.fakturanummer, l.namn FROM leverantorsfaktura f
         JOIN leverantor l ON l.id = f.leverantor_id WHERE f.verifikat_id = ?",
    )
    .bind(id.0)
    .fetch_optional(&mut *conn)
    .await?;
    if let Some((id, nummer, namn)) = leverantörsfaktura {
        return Ok(Some(Flödeslänk {
            flöde: Flöde::Leverantörsfaktura,
            id,
            text: format!("leverantörsfaktura {nummer} från {namn}"),
        }));
    }
    // #135 Kundfakturor
    Ok(crate::kundfaktura::fakturan_för_verifikat(conn, id)
        .await?
        .map(|(id, text)| Flödeslänk {
            flöde: Flöde::Kundfaktura,
            id,
            text,
        }))
}

/// Länken till verifikatet `id`, med vem som bokförde det och när.
pub(crate) async fn länk(
    conn: &mut SqliteConnection,
    id: Option<i64>,
) -> Result<Option<Länk>, DatabasFel> {
    let Some(id) = id else {
        return Ok(None);
    };
    let (nummer, bokford, kanal, av, namn): LänkRad = sqlx::query_as(
        "SELECT v.nummer, v.bokford, v.bokford_kanal, v.bokford_av, b.visningsnamn
         FROM verifikat v LEFT JOIN anvandare b ON b.id = v.bokford_av WHERE v.id = ?",
    )
    .bind(id)
    .fetch_one(conn)
    .await?;
    let bokförd = match (bokford, kanal) {
        (Some(tid), Some(kanal)) => Some(signatur(&tid, &kanal, av, namn)?),
        _ => None,
    };
    Ok(Some(Länk {
        id: VerifikatId(id),
        nummer: nummer.map(verifikationsnummer).transpose()?,
        bokförd,
    }))
}

type LänkRad = (
    Option<i64>,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<String>,
);

/// Företagets verifikat i urvalet: bokförda i nummerordning per räkenskapsår, nyast först, och
/// utkast före dem.
pub async fn lista(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    urval: Urval,
) -> Result<Vec<Verifikatrad>, ÅtkomstFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    type ListRad = (i64, Option<i64>, Option<String>, String, i64);
    let rader: Vec<ListRad> = sqlx::query_as(
        "SELECT v.id, v.nummer, v.datum, v.beskrivning,
                coalesce((SELECT sum(debet) FROM verifikatrad WHERE verifikat_id = v.id), 0)
         FROM verifikat v
         LEFT JOIN rakenskapsar r ON r.id = v.rakenskapsar_id
         WHERE v.foretag_id = ?1
           AND (?2 = 'alla' OR (?2 = 'utkast') = (v.bokford IS NULL))
         ORDER BY v.bokford IS NOT NULL, r.start DESC, v.nummer DESC, v.id DESC",
    )
    .bind(företag.0)
    .bind(urval.kod())
    .fetch_all(&mut *tx)
    .await?;
    let rader = rader
        .into_iter()
        .map(|(id, nummer, datum_, beskrivning, summa)| {
            Ok(Verifikatrad {
                id: VerifikatId(id),
                nummer: nummer.map(verifikationsnummer).transpose()?,
                datum: datum_.as_deref().map(datum).transpose()?,
                beskrivning,
                summa: Belopp::från_öre(summa),
            })
        })
        .collect::<Result<_, DatabasFel>>()?;
    Ok(rader)
}

/// Verifikaten i företaget som frågan träffar på nummer, text (beskrivning och radtexter),
/// belopp (summan eller en rad) eller datum, i samma ordning som [`lista`]. Andra företags
/// verifikat söks aldrig. En tom fråga träffar inget.
pub async fn sök(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    fråga: &str,
) -> Result<Vec<Verifikatrad>, ÅtkomstFel> {
    let alla = lista(db, aktör, företag, Urval::Alla).await?;
    let Some(fråga) = Fråga::tolka(fråga) else {
        return Ok(Vec::new());
    };
    let rader: Vec<(i64, i64, i64, Option<String>)> = sqlx::query_as(
        "SELECT r.verifikat_id, r.debet, r.kredit, r.text
         FROM verifikatrad r JOIN verifikat v ON v.id = r.verifikat_id
         WHERE v.foretag_id = ?",
    )
    .bind(företag.0)
    .fetch_all(db.pool())
    .await?;
    Ok(alla
        .into_iter()
        .filter(|v| {
            let mut sökbart = Sökbart {
                nummer: v.nummer.map(Verifikationsnummer::get),
                datum: v.datum,
                texter: vec![v.beskrivning.as_str()],
                belopp: vec![v.summa],
            };
            for (_, debet, kredit, text) in rader.iter().filter(|r| r.0 == v.id.0) {
                sökbart.belopp.extend(
                    [*debet, *kredit]
                        .into_iter()
                        .filter(|b| *b != 0)
                        .map(Belopp::från_öre),
                );
                sökbart.texter.extend(text.as_deref());
            }
            fråga.träffar_verifikat(&sökbart)
        })
        .collect())
}

/// Namnet som visas för en aktör.
pub(crate) async fn namn(
    conn: &mut SqliteConnection, aktör: &Aktör
) -> Result<String, DatabasFel> {
    let namn: Option<String> = match aktör.användare {
        Some(a) => {
            sqlx::query_scalar("SELECT visningsnamn FROM anvandare WHERE id = ?")
                .bind(a.0)
                .fetch_optional(conn)
                .await?
        }
        None => None,
    };
    Ok(namn.unwrap_or_else(|| kanalens_namn(aktör.kanal)))
}

pub(crate) fn kanalens_namn(kanal: Kanal) -> String {
    match kanal {
        Kanal::Maya => "Maya".into(),
        Kanal::Cli => "CLI".into(),
        Kanal::Webb => "webb".into(),
    }
}

pub(crate) fn signatur(
    tid: &str,
    kanal: &str,
    användare: Option<i64>,
    namn: Option<String>,
) -> Result<Signatur, DatabasFel> {
    let korrupt = |vad: &str| DatabasFel::Korrupt(format!("verifikat.{vad}"));
    let kanal = Kanal::från_namn(kanal).ok_or_else(|| korrupt("kanal"))?;
    Ok(Signatur {
        tid: tid.parse().map_err(|_| korrupt("tid"))?,
        aktör: Aktör {
            kanal,
            användare: användare.map(AnvändarId),
        },
        namn: namn.unwrap_or_else(|| kanalens_namn(kanal)),
    })
}

fn kontonummer(nummer: i64) -> Result<Kontonummer, DatabasFel> {
    u16::try_from(nummer)
        .ok()
        .and_then(Kontonummer::new)
        .ok_or_else(|| DatabasFel::Korrupt(format!("ett kontonummer: {nummer}")))
}

pub(crate) fn verifikationsnummer(nummer: i64) -> Result<Verifikationsnummer, DatabasFel> {
    u32::try_from(nummer)
        .map(Verifikationsnummer)
        .map_err(|_| DatabasFel::Korrupt(format!("ett verifikationsnummer: {nummer}")))
}
