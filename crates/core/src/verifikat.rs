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
use maya_domain::sok::{Fråga, Sökbart};
use maya_domain::verifikat::{Förutsättningar, NyttVerifikat, Spärr, Årsval};
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
pub struct Verifikationsnummer(u32);

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

/// Ett verifikat med rader och historik.
#[derive(Debug, Clone, PartialEq)]
pub struct Verifikat {
    pub id: VerifikatId,
    /// `None` för ett utkast.
    pub nummer: Option<Verifikationsnummer>,
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

async fn läs_förutsättningar(
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

/// Bokför i ärendet. `underlag` är `None` när kopplingarna ska lämnas som de är.
async fn bokför_i(
    mut ärende: Ärende,
    utkast: Option<VerifikatId>,
    formulär: &NyttVerifikat,
    underlag: Option<&[UnderlagId]>,
) -> Result<Bokfört, VerifikatFel> {
    let företag = ärende.företag;
    let förutsättningar = läs_förutsättningar(ärende.conn(), företag).await?;
    let v = formulär
        .kontrollera(&förutsättningar)
        .map_err(VerifikatFel::Spärrat)?;
    let år_id = räkenskapsår_id(&mut ärende, v.räkenskapsår).await?;
    let innehåll = Innehåll {
        år_id: Some(år_id),
        datum: Some(v.datum),
        beskrivning: &v.beskrivning,
        rader: v
            .rader
            .iter()
            .map(|r| (Some(r.konto), r.debet, r.kredit, r.text.as_deref()))
            .collect(),
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
    let nummer = tilldela_nummer(&mut ärende, id, år_id).await?;
    let kopplade = underlag::verifikatets(ärende.conn(), id).await?;

    let år = v.räkenskapsår.år();
    ärende
        .logga(
            Händelse::VerifikatBokfört,
            &nummer.to_string(),
            serde_json::json!({
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
            }),
        )
        .await?;
    ärende.slutför().await?;
    Ok(Bokfört { id, nummer })
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
}

/// Skapar verifikatet som utkast med sina rader.
async fn skapa(
    ärende: &mut Ärende, innehåll: &Innehåll<'_>
) -> Result<VerifikatId, sqlx::Error> {
    let (företag, aktör, nu) = (ärende.företag, ärende.aktör, ärende.nu);
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO verifikat
             (foretag_id, rakenskapsar_id, datum, beskrivning, kalla, skapad, skapad_kanal,
              skapad_av)
         VALUES (?, ?, ?, ?, 'manuell', ?, ?, ?) RETURNING id",
    )
    .bind(företag.0)
    .bind(innehåll.år_id)
    .bind(innehåll.datum.map(|d| d.to_string()))
    .bind(innehåll.beskrivning)
    .bind(nu.to_string())
    .bind(aktör.kanal.namn())
    .bind(aktör.användare.map(|a| a.0))
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
    let Some(v): Option<VerifikatRad> = sqlx::query_as(
        "SELECT v.id, v.nummer, v.datum, v.beskrivning, r.start AS ar_start, r.slut AS ar_slut,
                v.skapad, v.skapad_kanal, v.skapad_av, s.visningsnamn AS skapad_namn,
                v.bokford, v.bokford_kanal, v.bokford_av, b.visningsnamn AS bokford_namn
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
    let underlag = underlag::verifikatets(&mut tx, id).await?;
    let mut historik = Vec::new();
    for post in behandlingshistorik::läs_om_verifikat(&mut tx, företag, id.0).await? {
        let namn = namn(&mut tx, &post.aktör).await?;
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
    }))
}

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

fn kanalens_namn(kanal: Kanal) -> String {
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
