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
use maya_domain::verifikat::{
    BokförbartVerifikat, Förutsättningar, NyttVerifikat, Spärr, Årsval
};
use sqlx::SqliteConnection;

use crate::aktor::{Aktör, AnvändarId, Handling, Kanal};
use crate::atkomst::{self, Ärende, ÅtkomstFel};
use crate::behandlingshistorik::{self, Händelse, Post};
use crate::foretag::{self, FöretagsId, datum};
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
    pub konto: Kontonummer,
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
    let mut ärende = Ärende::börja(db, klocka, aktör, företag, Handling::Bokföra).await?;
    let förutsättningar = läs_förutsättningar(ärende.conn(), företag).await?;
    let v = formulär
        .kontrollera(&förutsättningar)
        .map_err(VerifikatFel::Spärrat)?;
    let år_id = räkenskapsår_id(&mut ärende, &v).await?;
    let id = skapa(&mut ärende, år_id, &v).await?;
    let nummer = bokför_utkast(&mut ärende, id, år_id).await?;

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
            }),
        )
        .await?;
    ärende.slutför().await?;
    Ok(Bokfört { id, nummer })
}

/// Räkenskapsårets id, efter att året har skapats om det är nytt.
async fn räkenskapsår_id(
    ärende: &mut Ärende,
    v: &BokförbartVerifikat,
) -> Result<i64, VerifikatFel> {
    let företag = ärende.företag;
    match v.räkenskapsår {
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

/// Skapar verifikatet som utkast med sina rader.
async fn skapa(
    ärende: &mut Ärende,
    år_id: i64,
    v: &BokförbartVerifikat,
) -> Result<VerifikatId, sqlx::Error> {
    let (företag, aktör, nu) = (ärende.företag, ärende.aktör, ärende.nu);
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO verifikat
             (foretag_id, rakenskapsar_id, datum, beskrivning, kalla, skapad, skapad_kanal,
              skapad_av)
         VALUES (?, ?, ?, ?, 'manuell', ?, ?, ?) RETURNING id",
    )
    .bind(företag.0)
    .bind(år_id)
    .bind(v.datum.to_string())
    .bind(&v.beskrivning)
    .bind(nu.to_string())
    .bind(aktör.kanal.namn())
    .bind(aktör.användare.map(|a| a.0))
    .fetch_one(ärende.conn())
    .await?;
    for (i, rad) in v.rader.iter().enumerate() {
        sqlx::query(
            "INSERT INTO verifikatrad (verifikat_id, radnr, konto, debet, kredit, text)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(i as i64 + 1)
        .bind(i64::from(rad.konto.get()))
        .bind(rad.debet.öre())
        .bind(rad.kredit.öre())
        .bind(&rad.text)
        .execute(ärende.conn())
        .await?;
    }
    Ok(VerifikatId(id))
}

/// Bokför ett utkast: nästa nummer i årets serie, inom ärendets transaktion.
async fn bokför_utkast(
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
    let rader: Vec<(i64, i64, i64, Option<String>)> = sqlx::query_as(
        "SELECT konto, debet, kredit, text FROM verifikatrad WHERE verifikat_id = ? ORDER BY radnr",
    )
    .bind(id.0)
    .fetch_all(&mut *tx)
    .await?;
    let rader = rader
        .into_iter()
        .map(|(konto, debet, kredit, text)| {
            Ok(Rad {
                konto: kontonummer(konto)?,
                debet: Belopp::från_öre(debet),
                kredit: Belopp::från_öre(kredit),
                text,
            })
        })
        .collect::<Result<Vec<_>, DatabasFel>>()?;
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
async fn namn(conn: &mut SqliteConnection, aktör: &Aktör) -> Result<String, DatabasFel> {
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

fn signatur(
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

fn verifikationsnummer(nummer: i64) -> Result<Verifikationsnummer, DatabasFel> {
    u32::try_from(nummer)
        .map(Verifikationsnummer)
        .map_err(|_| DatabasFel::Korrupt(format!("ett verifikationsnummer: {nummer}")))
}
