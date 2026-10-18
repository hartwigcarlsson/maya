//! Rapporter → Bokföring: grundboken och huvudboken (L3, L4).
//!
//! **Grundboken** är alla bokförda verifikat i registreringsordning, alltså i den ordning de
//! bokfördes (bokförd-tidpunkten), per period efter verifikatdatum. **Huvudboken** är samma
//! rader per konto, också i registreringsordning, med ingående balans, löpande saldo och
//! utgående balans. Båda räknas bara ur bokförda verifikat; utkast syns aldrig (L2).
//!
//! Urvalet är ett räkenskapsår och en följd av dess perioder. Huvudbokens ingående balans för
//! en period mitt i året är årets ingående balans (inskriven eller preliminär, se
//! [`crate::saldo`]) plus årets transaktioner före den första perioden i urvalet.

use std::collections::BTreeMap;

use jiff::Timestamp;
use jiff::civil::Date;
use maya_domain::belopp::Belopp;
use maya_domain::kontoplan::Kontonummer;
use maya_domain::rakenskapsar::{Period, Räkenskapsår};
use sqlx::SqliteConnection;

use crate::aktor::{Aktör, Handling, Kanal};
use crate::atkomst::{self, ÅtkomstFel};
use crate::foretag::{FöretagsId, datum};
use crate::verifikat::{VerifikatId, Verifikationsnummer, kanalens_namn, verifikationsnummer};
use crate::{Databas, DatabasFel};

/// Vilka perioder en grundbok eller huvudbok gäller: ett räkenskapsår och perioderna `från`
/// t.o.m. `till` i det. Ett datum anger perioden det ligger i; `None` är årets första
/// respektive sista period.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Periodurval {
    /// Räkenskapsårets startdatum.
    pub år: Date,
    pub från: Option<Date>,
    pub till: Option<Date>,
}

#[derive(Debug, thiserror::Error)]
pub enum RapportFel {
    #[error("företaget har inget räkenskapsår som börjar det datumet")]
    OkäntRäkenskapsår,
    /// Perioderna ligger inte i året, eller `från` är efter `till`.
    #[error("perioderna finns inte i räkenskapsåret")]
    OkändPeriod,
    #[error(transparent)]
    Åtkomst(#[from] ÅtkomstFel),
}

impl From<sqlx::Error> for RapportFel {
    fn from(e: sqlx::Error) -> Self {
        RapportFel::Åtkomst(e.into())
    }
}

impl From<DatabasFel> for RapportFel {
    fn from(e: DatabasFel) -> Self {
        RapportFel::Åtkomst(e.into())
    }
}

/// Vem som bokförde och när: registreringstidpunkten (H1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bokföring {
    pub tid: Timestamp,
    /// Användarens visningsnamn.
    pub namn: String,
}

/// En rad i ett bokfört verifikat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bokföringsrad {
    pub konto: Kontonummer,
    pub kontonamn: String,
    pub text: Option<String>,
    pub debet: Belopp,
    pub kredit: Belopp,
}

/// Ett bokfört verifikat i grundboken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grundbokspost {
    pub id: VerifikatId,
    pub nummer: Verifikationsnummer,
    pub datum: Date,
    pub beskrivning: String,
    pub bokförd: Bokföring,
    pub rader: Vec<Bokföringsrad>,
    /// Summan av debet.
    pub summa: Belopp,
}

/// En period i grundboken med dess verifikat i registreringsordning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grundboksperiod {
    pub period: Period,
    pub verifikat: Vec<Grundbokspost>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grundbok {
    pub år: Räkenskapsår,
    /// Urvalets perioder i ordning, också de utan verifikat.
    pub perioder: Vec<Grundboksperiod>,
}

/// Grundboken: de bokförda verifikaten i urvalets perioder, per period och i
/// registreringsordning inom perioden.
pub async fn grundbok(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    urval: &Periodurval,
) -> Result<Grundbok, RapportFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    let (år_id, år, perioder) = välj(&mut tx, företag, urval).await?;
    let poster = bokförda(&mut tx, företag, år_id).await?;
    let perioder = perioder
        .into_iter()
        .map(|period| Grundboksperiod {
            period,
            verifikat: poster
                .iter()
                .filter(|p| period.start <= p.datum && p.datum <= period.slut)
                .cloned()
                .collect(),
        })
        .collect();
    Ok(Grundbok { år, perioder })
}

/// Räkenskapsårets id, året och urvalets perioder.
async fn välj(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    urval: &Periodurval,
) -> Result<(i64, Räkenskapsår, Vec<Period>), RapportFel> {
    let rad: Option<(i64, String)> =
        sqlx::query_as("SELECT id, slut FROM rakenskapsar WHERE foretag_id = ? AND start = ?")
            .bind(företag.0)
            .bind(urval.år.to_string())
            .fetch_optional(&mut *conn)
            .await?;
    let (id, slut) = rad.ok_or(RapportFel::OkäntRäkenskapsår)?;
    let år = Räkenskapsår::befintligt(urval.år, datum(&slut)?);
    let alla = år.perioder();
    let index = |d: Option<Date>, standard: usize| match d {
        None => Some(standard),
        Some(d) => alla.iter().position(|p| p.start <= d && d <= p.slut),
    };
    let (Some(från), Some(till)) = (index(urval.från, 0), index(urval.till, alla.len() - 1))
    else {
        return Err(RapportFel::OkändPeriod);
    };
    if från > till {
        return Err(RapportFel::OkändPeriod);
    }
    Ok((id, år, alla[från..=till].to_vec()))
}

/// Årets bokförda verifikat med rader, i registreringsordning: bokförd-tidpunkten och sedan
/// numret. Tidpunkten jämförs som tid och inte som text, eftersom texten kan ha olika många
/// decimaler.
async fn bokförda(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    år_id: i64,
) -> Result<Vec<Grundbokspost>, DatabasFel> {
    type Rad = (
        i64,
        i64,
        String,
        String,
        String,
        String,
        Option<String>,
        i64,
        String,
        i64,
        i64,
        Option<String>,
    );
    let rader: Vec<Rad> = sqlx::query_as(
        "SELECT v.id, v.nummer, v.datum, v.beskrivning, v.bokford, v.bokford_kanal, a.visningsnamn,
                r.konto, coalesce(k.namn, ''), r.debet, r.kredit, r.text
         FROM verifikat v
         JOIN verifikatrad r ON r.verifikat_id = v.id
         LEFT JOIN anvandare a ON a.id = v.bokford_av
         LEFT JOIN konto k ON k.foretag_id = v.foretag_id AND k.nummer = r.konto
         WHERE v.foretag_id = ? AND v.rakenskapsar_id = ? AND v.bokford IS NOT NULL
         ORDER BY v.id, r.radnr",
    )
    .bind(företag.0)
    .bind(år_id)
    .fetch_all(conn)
    .await?;
    let korrupt = |vad: &str| DatabasFel::Korrupt(format!("verifikat.{vad}"));
    let mut poster: BTreeMap<i64, Grundbokspost> = BTreeMap::new();
    for (
        id,
        nummer,
        datum_,
        beskrivning,
        bokförd,
        kanal,
        namn,
        konto,
        kontonamn,
        debet,
        kredit,
        text,
    ) in rader
    {
        let post = match poster.entry(id) {
            std::collections::btree_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::btree_map::Entry::Vacant(e) => e.insert(Grundbokspost {
                id: VerifikatId(id),
                nummer: verifikationsnummer(nummer)?,
                datum: datum(&datum_)?,
                beskrivning,
                bokförd: Bokföring {
                    tid: bokförd.parse().map_err(|_| korrupt("bokford"))?,
                    namn: match namn {
                        Some(namn) => namn,
                        None => kanalens_namn(
                            Kanal::från_namn(&kanal).ok_or_else(|| korrupt("bokford_kanal"))?,
                        ),
                    },
                },
                rader: Vec::new(),
                summa: Belopp::NOLL,
            }),
        };
        post.summa = Belopp::från_öre(post.summa.öre() + debet);
        post.rader.push(Bokföringsrad {
            konto: kontonummer(konto)?,
            kontonamn,
            text,
            debet: Belopp::från_öre(debet),
            kredit: Belopp::från_öre(kredit),
        });
    }
    let mut poster: Vec<_> = poster.into_values().collect();
    poster.sort_by_key(|p| (p.bokförd.tid, p.nummer));
    Ok(poster)
}

fn kontonummer(nummer: i64) -> Result<Kontonummer, DatabasFel> {
    u16::try_from(nummer)
        .ok()
        .and_then(Kontonummer::new)
        .ok_or_else(|| DatabasFel::Korrupt(format!("ett kontonummer: {nummer}")))
}
