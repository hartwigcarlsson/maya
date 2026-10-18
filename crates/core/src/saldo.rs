//! Saldon per konto och räkenskapsår: ingående balans, årets förändring ur bokförda verifikat
//! och utgående balans. Rapporterna, SIE4-exporten och Att göra läser saldona härifrån.
//!
//! Den **ingående balansen** för företagets första räkenskapsår skrivs in i ett formulär över
//! balanskontona, där summan måste vara noll (Å2). Den kan ändras tills årets första period är
//! låst (periodlåsningen kommer i etapp 3, så i etapp 1 går det alltid), och varje ändring loggas
//! med aktör och tid (H1, H2). För ett senare år, vars föregående år inte är stängt, räknas en
//! **preliminär ingående balans** fram vid varje läsning enligt tabellen över automatkonteringar
//! och sparas inte:
//!
//! - IB(k, n) = UB(k, n−1) för varje konto k i klass 1–2, och
//! - IB(2099, n) = UB(2099, n−1) + Σ UB(k, n−1) över klass 3–8.
//!
//! Utkast räknas aldrig, bara bokförda verifikat.

use std::collections::BTreeMap;

use jiff::civil::Date;
use maya_domain::belopp::Saldo;
use maya_domain::ingaende_balans::{self as regler, NyIngåendeBalans, Spärr};
use maya_domain::kontoplan::Kontonummer;
use maya_domain::rakenskapsar::Räkenskapsår;
use maya_domain::rapport::Rörelse;
use sqlx::SqliteConnection;

use crate::aktor::{Aktör, Handling};
use crate::atkomst::{self, Ärende, ÅtkomstFel};
use crate::behandlingshistorik::Händelse;
use crate::foretag::{FöretagsId, datum};
use crate::{Databas, DatabasFel, Klocka};

/// Varifrån den ingående balansen kommer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Inskriven för företagets första räkenskapsår i Maya (noll tills någon skriver in den).
    Inskriven,
    /// Framräknad ur föregående års utgående balans, eftersom det året inte är stängt. Sparas
    /// inte, och ska visas som preliminär där den läses.
    Preliminär,
}

#[derive(Debug, thiserror::Error)]
pub enum SaldoFel {
    #[error("företaget har inget räkenskapsår som börjar det datumet")]
    OkäntRäkenskapsår,
    #[error(transparent)]
    Åtkomst(#[from] ÅtkomstFel),
}

impl From<sqlx::Error> for SaldoFel {
    fn from(e: sqlx::Error) -> Self {
        SaldoFel::Åtkomst(e.into())
    }
}

impl From<DatabasFel> for SaldoFel {
    fn from(e: DatabasFel) -> Self {
        SaldoFel::Åtkomst(e.into())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum IngåendeBalansFel {
    /// Kontrollen i `domain`, med samma koder som formuläret visar.
    #[error("den ingående balansen kan inte sparas")]
    Spärrat(Vec<Spärr>),
    #[error("företaget har inget räkenskapsår som börjar det datumet")]
    OkäntRäkenskapsår,
    /// Bara det första årets ingående balans skrivs in; senare års är preliminär tills
    /// föregående år stängs.
    #[error(
        "Den ingående balansen skrivs bara in för det första räkenskapsåret. Senare år får sin \
         från föregående år."
    )]
    InteFörstaÅret,
    #[error(transparent)]
    Åtkomst(#[from] ÅtkomstFel),
}

impl From<sqlx::Error> for IngåendeBalansFel {
    fn from(e: sqlx::Error) -> Self {
        IngåendeBalansFel::Åtkomst(e.into())
    }
}

impl From<DatabasFel> for IngåendeBalansFel {
    fn from(e: DatabasFel) -> Self {
        IngåendeBalansFel::Åtkomst(e.into())
    }
}

/// Ett räkenskapsårs ingående balans: saldon skilda från noll, i kontoordning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngåendeBalans {
    pub år: Räkenskapsår,
    pub status: Status,
    pub saldon: Vec<(Kontonummer, Saldo)>,
}

/// Ett kontos saldon under ett räkenskapsår. Ingående balans finns bara på balanskonton.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Kontosaldo {
    pub konto: Kontonummer,
    pub ingående: Saldo,
    /// Summan av bokförda verifikat i året.
    pub förändring: Saldo,
    pub utgående: Saldo,
}

/// Saldona för ett räkenskapsår.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saldobalans {
    pub år: Räkenskapsår,
    /// Den ingående balansens status.
    pub status: Status,
    /// Konton med en ingående balans eller bokförda rader i året, i kontoordning.
    pub konton: Vec<Kontosaldo>,
}

/// Den ingående balansen för räkenskapsåret som börjar `år`.
pub async fn ingående_balans(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    år: Date,
) -> Result<IngåendeBalans, SaldoFel> {
    let sb = saldobalans(db, aktör, företag, år).await?;
    Ok(IngåendeBalans {
        år: sb.år,
        status: sb.status,
        saldon: sb
            .konton
            .iter()
            .filter(|k| k.ingående != Saldo::NOLL)
            .map(|k| (k.konto, k.ingående))
            .collect(),
    })
}

/// Ingående balans, förändring och utgående balans per konto för räkenskapsåret som börjar `år`.
pub async fn saldobalans(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    år: Date,
) -> Result<Saldobalans, SaldoFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    saldobalanser(&mut tx, företag)
        .await?
        .into_iter()
        .find(|sb| sb.år.start() == år)
        .ok_or(SaldoFel::OkäntRäkenskapsår)
}

/// Saldobalansen för vart och ett av företagets räkenskapsår, i ordning. Varje års ingående
/// balans bygger på föregående års utgående.
pub(crate) async fn saldobalanser(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
) -> Result<Vec<Saldobalans>, DatabasFel> {
    let åren = räkenskapsåren(conn, företag).await?;
    let Some(första) = åren.first() else {
        return Ok(Vec::new());
    };
    let förändringar = förändringar(conn, företag).await?;
    let tom = BTreeMap::new();
    let förändring = |id: i64| förändringar.get(&id).unwrap_or(&tom);
    let mut ingående = inskriven(conn, första.id).await?;
    let mut saldobalanser = Vec::new();
    for (i, år) in åren.iter().enumerate() {
        if i > 0 {
            ingående = regler::preliminär(summa(&ingående, förändring(åren[i - 1].id)))
                .into_iter()
                .collect();
        }
        let årets = förändring(år.id);
        saldobalanser.push(Saldobalans {
            år: år.år,
            status: if i == 0 {
                Status::Inskriven
            } else {
                Status::Preliminär
            },
            konton: summa(&ingående, årets)
                .into_iter()
                .map(|(konto, utgående)| Kontosaldo {
                    konto,
                    ingående: ingående.get(&konto).copied().unwrap_or_default(),
                    förändring: årets.get(&konto).copied().unwrap_or_default(),
                    utgående,
                })
                .collect(),
        });
    }
    Ok(saldobalanser)
}

/// De bokförda rörelserna per räkenskapsår (efter startdatum), konto och månad, för
/// rapporternas kolumner.
pub(crate) async fn månadsrörelser(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
) -> Result<BTreeMap<Date, Vec<Rörelse>>, DatabasFel> {
    let rader: Vec<(String, String, i64, i64)> = sqlx::query_as(
        "SELECT a.start, substr(v.datum, 1, 7) || '-01', r.konto, sum(r.debet - r.kredit)
         FROM verifikatrad r
         JOIN verifikat v ON v.id = r.verifikat_id
         JOIN rakenskapsar a ON a.id = v.rakenskapsar_id
         WHERE v.foretag_id = ? AND v.bokford IS NOT NULL
         GROUP BY a.start, substr(v.datum, 1, 7), r.konto
         ORDER BY a.start, 2, r.konto",
    )
    .bind(företag.0)
    .fetch_all(conn)
    .await?;
    let mut per_år: BTreeMap<Date, Vec<Rörelse>> = BTreeMap::new();
    for (år, månad, konto, saldo) in rader {
        per_år.entry(datum(&år)?).or_default().push(Rörelse {
            konto: kontonummer(konto)?,
            månad: datum(&månad)?,
            saldo: Saldo::från_öre(saldo),
        });
    }
    Ok(per_år)
}

/// Balanskontona (klass 1–2) i företagets kontoplan och om de är aktiva, för formuläret.
pub async fn balanskonton(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
) -> Result<Vec<(Kontonummer, bool)>, ÅtkomstFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    Ok(läs_balanskonton(&mut tx, företag).await?)
}

/// Skriver in eller ändrar den ingående balansen för företagets första räkenskapsår, som börjar
/// `år`. Saldona ersätter de tidigare, och ändringen loggas med vad som ändrades, i samma
/// transaktion. Samma saldon som förut är ingen ändring och loggas inte.
pub async fn ändra_ingående_balans(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    år: Date,
    formulär: &NyIngåendeBalans,
) -> Result<(), IngåendeBalansFel> {
    let mut ärende =
        Ärende::börja(db, klocka, aktör, företag, Handling::ÄndraIngåendeBalans).await?;
    let åren = räkenskapsåren(ärende.conn(), företag).await?;
    let i = åren
        .iter()
        .position(|å| å.år.start() == år)
        .ok_or(IngåendeBalansFel::OkäntRäkenskapsår)?;
    if i != 0 {
        return Err(IngåendeBalansFel::InteFörstaÅret);
    }
    let Årsrad { id: år_id, år } = åren[0];
    let konton = läs_balanskonton(ärende.conn(), företag).await?;
    let ny = formulär
        .kontrollera(&konton)
        .map_err(IngåendeBalansFel::Spärrat)?;
    let före = inskriven(ärende.conn(), år_id).await?;
    let efter: BTreeMap<_, _> = ny.saldon.iter().copied().collect();
    let ändringar: Vec<_> = före
        .keys()
        .chain(efter.keys())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .filter_map(|konto| {
            let (f, e) = (
                före.get(konto).copied().unwrap_or_default(),
                efter.get(konto).copied().unwrap_or_default(),
            );
            (f != e).then(|| {
                serde_json::json!({ "konto": konto.get(), "före_öre": f.öre(), "efter_öre": e.öre() })
            })
        })
        .collect();
    if ändringar.is_empty() {
        return Ok(());
    }

    sqlx::query("DELETE FROM ingaende_balans WHERE rakenskapsar_id = ?")
        .bind(år_id)
        .execute(ärende.conn())
        .await?;
    for (konto, saldo) in &ny.saldon {
        sqlx::query("INSERT INTO ingaende_balans (rakenskapsar_id, konto, saldo) VALUES (?, ?, ?)")
            .bind(år_id)
            .bind(i64::from(konto.get()))
            .bind(saldo.öre())
            .execute(ärende.conn())
            .await?;
    }
    let (start, slut) = (år.start().to_string(), år.slut().to_string());
    ärende
        .logga(
            Händelse::IngåendeBalansÄndrad,
            &format!("{start} – {slut}"),
            serde_json::json!({
                "räkenskapsår": { "start": start, "slut": slut },
                "ändringar": ändringar,
            }),
        )
        .await?;
    ärende.slutför().await?;
    Ok(())
}

#[derive(Clone, Copy)]
struct Årsrad {
    id: i64,
    år: Räkenskapsår,
}

/// Företagets räkenskapsår i ordning.
async fn räkenskapsåren(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
) -> Result<Vec<Årsrad>, DatabasFel> {
    let rader: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT id, start, slut FROM rakenskapsar WHERE foretag_id = ? ORDER BY start",
    )
    .bind(företag.0)
    .fetch_all(conn)
    .await?;
    rader
        .iter()
        .map(|(id, start, slut)| {
            Ok(Årsrad {
                id: *id,
                år: Räkenskapsår::befintligt(datum(start)?, datum(slut)?),
            })
        })
        .collect()
}

/// Den inskrivna ingående balansen för ett år.
async fn inskriven(
    conn: &mut SqliteConnection,
    år_id: i64,
) -> Result<BTreeMap<Kontonummer, Saldo>, DatabasFel> {
    let rader: Vec<(i64, i64)> =
        sqlx::query_as("SELECT konto, saldo FROM ingaende_balans WHERE rakenskapsar_id = ?")
            .bind(år_id)
            .fetch_all(conn)
            .await?;
    rader
        .into_iter()
        .map(|(konto, saldo)| Ok((kontonummer(konto)?, Saldo::från_öre(saldo))))
        .collect()
}

/// Summan av de bokförda raderna per räkenskapsår och konto.
async fn förändringar(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
) -> Result<BTreeMap<i64, BTreeMap<Kontonummer, Saldo>>, DatabasFel> {
    let rader: Vec<(i64, i64, i64)> = sqlx::query_as(
        "SELECT v.rakenskapsar_id, r.konto, sum(r.debet - r.kredit)
         FROM verifikatrad r JOIN verifikat v ON v.id = r.verifikat_id
         WHERE v.foretag_id = ? AND v.bokford IS NOT NULL
         GROUP BY v.rakenskapsar_id, r.konto",
    )
    .bind(företag.0)
    .fetch_all(conn)
    .await?;
    let mut per_år: BTreeMap<i64, BTreeMap<Kontonummer, Saldo>> = BTreeMap::new();
    for (år, konto, saldo) in rader {
        per_år
            .entry(år)
            .or_default()
            .insert(kontonummer(konto)?, Saldo::från_öre(saldo));
    }
    Ok(per_år)
}

async fn läs_balanskonton(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
) -> Result<Vec<(Kontonummer, bool)>, DatabasFel> {
    let rader: Vec<(i64, bool)> = sqlx::query_as(
        "SELECT nummer, aktiv FROM konto WHERE foretag_id = ? AND nummer < 3000 ORDER BY nummer",
    )
    .bind(företag.0)
    .fetch_all(conn)
    .await?;
    rader
        .into_iter()
        .map(|(nummer, aktiv)| Ok((kontonummer(nummer)?, aktiv)))
        .collect()
}

fn summa(
    a: &BTreeMap<Kontonummer, Saldo>,
    b: &BTreeMap<Kontonummer, Saldo>,
) -> BTreeMap<Kontonummer, Saldo> {
    let mut summa = a.clone();
    for (konto, saldo) in b {
        *summa.entry(*konto).or_default() += *saldo;
    }
    summa
}

pub(crate) fn kontonummer(nummer: i64) -> Result<Kontonummer, DatabasFel> {
    u16::try_from(nummer)
        .ok()
        .and_then(Kontonummer::new)
        .ok_or_else(|| DatabasFel::Korrupt(format!("ett kontonummer: {nummer}")))
}
