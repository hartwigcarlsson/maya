//! Företag: att lägga upp ett företag med första räkenskapsåret, kontoplanen och åtkomst för
//! den som lade upp det, och att läsa företagets uppgifter.

use std::fmt;
use std::str::FromStr;

use jiff::civil::Date;
use maya_domain::foretag::{Bokföringsmetod, Fältfel, Momsperiod, NyttFöretag};
use maya_domain::kontoplan::bas;
use maya_domain::orgnr::Organisationsnummer;
use maya_domain::rakenskapsar::Period;
use sqlx::SqliteConnection;

use crate::aktor::{self, Aktör, Handling};
use crate::atkomst::{self, Ärende, ÅtkomstFel};
use crate::behandlingshistorik::Händelse;
use crate::{Databas, DatabasFel, Klocka, kontoplan};

/// Ett företags id i installationen. Vem som helst kan skriva ett id; `core` avgör om aktören
/// har åtkomst till företaget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FöretagsId(pub(crate) i64);

impl fmt::Display for FöretagsId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for FöretagsId {
    type Err = std::num::ParseIntError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse().map(FöretagsId)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FöretagsFel {
    /// Formuläret klarar inte kontrollen i `domain`; samma koder som formuläret visar.
    #[error("formuläret är inte rätt ifyllt")]
    Ogiltigt(Vec<Fältfel>),
    #[error("{0} finns redan i Maya. Be någon som har åtkomst till företaget om en inbjudan.")]
    OrgnrFinns(Organisationsnummer),
    #[error(transparent)]
    Åtkomst(#[from] ÅtkomstFel),
}

impl From<sqlx::Error> for FöretagsFel {
    fn from(e: sqlx::Error) -> Self {
        FöretagsFel::Åtkomst(e.into())
    }
}

/// Ett företag i en lista, t.ex. företagsväljaren.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Företagsrad {
    pub id: FöretagsId,
    pub orgnr: Organisationsnummer,
    pub namn: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Räkenskapsår {
    pub start: Date,
    pub slut: Date,
    pub perioder: Vec<Period>,
}

/// Ett företags uppgifter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Företag {
    pub id: FöretagsId,
    pub orgnr: Organisationsnummer,
    pub namn: String,
    /// Räkenskapsåren i ordning.
    pub räkenskapsår: Vec<Räkenskapsår>,
    /// Bokföringsmetoden med gäller-från-datum, äldst först.
    pub bokföringsmetoder: Vec<(Date, Bokföringsmetod)>,
    /// Momsperioden med gäller-från-datum, äldst först.
    pub momsperioder: Vec<(Date, Momsperiod)>,
}

/// Lägger upp ett företag ur formuläret: företaget, första räkenskapsåret med perioder,
/// bokföringsmetod och momsperiod från årets start, en kopia av BAS, åtkomst för aktören och
/// posten i behandlingshistoriken, i en transaktion. Bara webben (beslut i #61).
pub async fn lägg_upp(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    formulär: &NyttFöretag,
) -> Result<FöretagsId, FöretagsFel> {
    if !aktor::tillåter(aktör.kanal, Handling::LäggaUppFöretag) {
        return Err(ÅtkomstFel::KanalenFårInte.into());
    }
    let Some(användare) = aktör.användare else {
        return Err(ÅtkomstFel::IngenÅtkomst.into());
    };
    let nytt = formulär.kontrollera().map_err(FöretagsFel::Ogiltigt)?;
    let nu = klocka.nu();
    // `BEGIN IMMEDIATE` serialiserar uppläggningarna, så kontrollen nedan räcker; UNIQUE på
    // orgnr är skyddsnätet.
    let mut tx = db.pool().begin_with("BEGIN IMMEDIATE").await?;

    let finns: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM foretag WHERE orgnr = ?)")
        .bind(nytt.orgnr.siffror())
        .fetch_one(&mut *tx)
        .await?;
    if finns {
        return Err(FöretagsFel::OrgnrFinns(nytt.orgnr));
    }
    let id = FöretagsId(
        sqlx::query_scalar(
            "INSERT INTO foretag (orgnr, namn, skapad) VALUES (?, ?, ?) RETURNING id",
        )
        .bind(nytt.orgnr.siffror())
        .bind(&nytt.namn)
        .bind(nu.to_string())
        .fetch_one(&mut *tx)
        .await?,
    );
    sqlx::query("INSERT INTO atkomst (foretag_id, anvandare_id) VALUES (?, ?)")
        .bind(id.0)
        .bind(användare.0)
        .execute(&mut *tx)
        .await?;
    let år = &nytt.första_år;
    lägg_till_räkenskapsår(&mut tx, id, år.start(), år.slut(), &år.perioder()).await?;
    let start = år.start().to_string();
    sqlx::query("INSERT INTO bokforingsmetod (foretag_id, galler_fran, metod) VALUES (?, ?, ?)")
        .bind(id.0)
        .bind(&start)
        .bind(nytt.bokföringsmetod.kod())
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO momsperiod (foretag_id, galler_fran, momsperiod) VALUES (?, ?, ?)")
        .bind(id.0)
        .bind(&start)
        .bind(nytt.momsperiod.kod())
        .execute(&mut *tx)
        .await?;
    kontoplan::kopiera_bas(&mut tx, id).await?;

    let mut ärende = Ärende::för_nytt_företag(tx, aktör, id, nu);
    ärende
        .logga(
            Händelse::FöretagSkapat,
            &nytt.orgnr.to_string(),
            serde_json::json!({
                "orgnr": nytt.orgnr.to_string(),
                "namn": nytt.namn,
                "första_räkenskapsår": { "start": start, "slut": år.slut().to_string() },
                "bokföringsmetod": nytt.bokföringsmetod.kod(),
                "momsperiod": nytt.momsperiod.kod(),
                "kontoplan": bas::VERSION,
            }),
        )
        .await?;
    ärende.slutför().await?;
    Ok(id)
}

async fn lägg_till_räkenskapsår(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    start: Date,
    slut: Date,
    perioder: &[Period],
) -> Result<(), sqlx::Error> {
    let år: i64 = sqlx::query_scalar(
        "INSERT INTO rakenskapsar (foretag_id, start, slut) VALUES (?, ?, ?) RETURNING id",
    )
    .bind(företag.0)
    .bind(start.to_string())
    .bind(slut.to_string())
    .fetch_one(&mut *conn)
    .await?;
    for period in perioder {
        sqlx::query("INSERT INTO period (rakenskapsar_id, start, slut) VALUES (?, ?, ?)")
            .bind(år)
            .bind(period.start.to_string())
            .bind(period.slut.to_string())
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

/// Företagen som aktören har åtkomst till, i namnordning.
pub async fn mina_företag(db: &Databas, aktör: &Aktör) -> Result<Vec<Företagsrad>, DatabasFel> {
    let Some(användare) = aktör.användare else {
        return Ok(Vec::new());
    };
    if !aktor::tillåter(aktör.kanal, Handling::LäsaFöretag) {
        return Ok(Vec::new());
    }
    let rader: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT f.id, f.orgnr, f.namn FROM foretag f
         JOIN atkomst a ON a.foretag_id = f.id
         JOIN anvandare u ON u.id = a.anvandare_id
         WHERE a.anvandare_id = ? AND u.avaktiverad IS NULL
         ORDER BY f.namn COLLATE NOCASE, f.id",
    )
    .bind(användare.0)
    .fetch_all(db.pool())
    .await?;
    rader
        .into_iter()
        .map(|(id, orgnr, namn)| {
            Ok(Företagsrad {
                id: FöretagsId(id),
                orgnr: tolka_orgnr(&orgnr)?,
                namn,
            })
        })
        .collect()
}

/// Företagets uppgifter, räkenskapsår och historiken för bokföringsmetod och momsperiod.
pub async fn företaget(
    db: &Databas,
    aktör: &Aktör,
    id: FöretagsId,
) -> Result<Företag, ÅtkomstFel> {
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, id, Handling::LäsaFöretag).await?;
    let (orgnr, namn): (String, String) =
        sqlx::query_as("SELECT orgnr, namn FROM foretag WHERE id = ?")
            .bind(id.0)
            .fetch_one(&mut *tx)
            .await?;
    let år: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT id, start, slut FROM rakenskapsar WHERE foretag_id = ? ORDER BY start",
    )
    .bind(id.0)
    .fetch_all(&mut *tx)
    .await?;
    let mut räkenskapsår = Vec::new();
    for (år_id, start, slut) in år {
        let perioder: Vec<(String, String)> = sqlx::query_as(
            "SELECT start, slut FROM period WHERE rakenskapsar_id = ? ORDER BY start",
        )
        .bind(år_id)
        .fetch_all(&mut *tx)
        .await?;
        räkenskapsår.push(Räkenskapsår {
            start: datum(&start)?,
            slut: datum(&slut)?,
            perioder: perioder
                .iter()
                .map(|(start, slut)| {
                    Ok(Period {
                        start: datum(start)?,
                        slut: datum(slut)?,
                    })
                })
                .collect::<Result<_, DatabasFel>>()?,
        });
    }
    let metoder: Vec<(String, String)> = sqlx::query_as(
        "SELECT galler_fran, metod FROM bokforingsmetod WHERE foretag_id = ? ORDER BY galler_fran",
    )
    .bind(id.0)
    .fetch_all(&mut *tx)
    .await?;
    let momsperioder: Vec<(String, String)> = sqlx::query_as(
        "SELECT galler_fran, momsperiod FROM momsperiod WHERE foretag_id = ? ORDER BY galler_fran",
    )
    .bind(id.0)
    .fetch_all(&mut *tx)
    .await?;
    let korrupt = |vad: &str| DatabasFel::Korrupt(vad.to_string());
    Ok(Företag {
        id,
        orgnr: tolka_orgnr(&orgnr)?,
        namn,
        räkenskapsår,
        bokföringsmetoder: metoder
            .iter()
            .map(|(från, kod)| {
                Ok((
                    datum(från)?,
                    Bokföringsmetod::från_kod(kod).ok_or_else(|| korrupt("bokforingsmetod"))?,
                ))
            })
            .collect::<Result<_, DatabasFel>>()?,
        momsperioder: momsperioder
            .iter()
            .map(|(från, kod)| {
                Ok((
                    datum(från)?,
                    Momsperiod::från_kod(kod).ok_or_else(|| korrupt("momsperiod"))?,
                ))
            })
            .collect::<Result<_, DatabasFel>>()?,
    })
}

fn tolka_orgnr(siffror: &str) -> Result<Organisationsnummer, DatabasFel> {
    Organisationsnummer::tolka(siffror).map_err(|_| DatabasFel::Korrupt("foretag.orgnr".into()))
}

fn datum(text: &str) -> Result<Date, DatabasFel> {
    text.parse()
        .map_err(|_| DatabasFel::Korrupt(format!("ett datum: {text}")))
}
