//! Att göra, företagets startsida: dagens datum, nyckeltalen ur bokföringen och sakerna att
//! göra. I etapp 1 är sakerna driftvarningar, verifikatutkast och okopplade underlag; varje ny
//! sort läggs till som en variant i [`Sak`] och räknas i [`antal`].
//!
//! Fristerna (T1) räknas i `maya_domain::att_gora`. De varnar men spärrar inte, och Maya bokför
//! aldrig automatiskt.

use std::collections::BTreeMap;

use jiff::Timestamp;
use jiff::civil::Date;
use maya_domain::att_gora::{self as regler, Frist, Utkastläge};
use maya_domain::belopp::{Belopp, Saldo};
use maya_domain::kontoplan::Kontonummer;
use maya_domain::rakenskapsar::Räkenskapsår;
use sqlx::SqliteConnection;

use crate::aktor::{Aktör, Handling};
use crate::atkomst::{self, ÅtkomstFel};
use crate::config::Config;
use crate::drift::{self, Driftvarning};
use crate::foretag::{self, FöretagsId};
use crate::inloggning::tidpunkt;
use crate::saldo;
use crate::underlag::{self, Underlag};
use crate::verifikat::VerifikatId;
use crate::{Databas, DatabasFel, Klocka};

/// Företagskontot i BAS.
const FÖRETAGSKONTOT: u16 = 1930;
/// Årets resultat, som inte räknas in i resultatet.
const ÅRETS_RESULTAT: u16 = 8999;

/// Det som Att göra visar för ett företag.
#[derive(Debug, Clone, PartialEq)]
pub struct AttGöra {
    /// Dagens datum i Stockholm.
    pub idag: Date,
    pub nyckeltal: Nyckeltal,
    /// Driftvarningarna, utkasten i id-ordning och de okopplade underlagen äldst först.
    pub saker: Vec<Sak>,
}

/// Nyckeltalen överst, räknade ur bokförda verifikat daterade till och med i dag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Nyckeltal {
    /// Saldot på 1930 per i dag, debet positivt.
    pub företagskontot: Saldo,
    /// Klass 3–8 utan 8999 i innevarande räkenskapsår, med tecknet vänt så att vinst är
    /// positiv.
    pub resultat_i_år: Saldo,
    /// Innevarande räkenskapsår, om det finns i Maya.
    pub år: Option<Räkenskapsår>,
}

/// En sak att göra.
#[derive(Debug, Clone, PartialEq)]
pub enum Sak {
    /// Något som är fel i driften. Samma i alla företag, och visas överst.
    Driftvarning(Driftvarning),
    Utkast(Utkastsak),
    /// Ett underlag utan koppling till något verifikat: "Vad är det här?".
    Underlag(Underlag),
}

/// Ett verifikatutkast som väntar på att bokföras.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Utkastsak {
    pub id: VerifikatId,
    pub datum: Option<Date>,
    pub beskrivning: String,
    /// Summan av debet.
    pub summa: Belopp,
    pub skapad: Timestamp,
    /// Antalet kopplade underlag.
    pub underlag: usize,
    /// Minst en rad på ett konto i grupp 191.
    pub kassa: bool,
}

impl Utkastsak {
    pub fn läge(&self) -> Utkastläge {
        Utkastläge {
            datum: self.datum,
            kassa: self.kassa,
        }
    }

    pub fn frist(&self, idag: Date) -> Frist {
        regler::frist(&self.läge(), idag)
    }
}

/// Att göra för företaget i dag. Driftvarningarna räknas ur `config` och backupens körningar.
pub async fn att_göra(
    db: &Databas,
    klocka: &Klocka,
    config: &Config,
    aktör: &Aktör,
    företag: FöretagsId,
) -> Result<AttGöra, ÅtkomstFel> {
    let idag = klocka.idag();
    let mut tx = db.pool().begin().await?;
    atkomst::kräv(&mut tx, aktör, företag, Handling::LäsaFöretag).await?;
    let nyckeltal = nyckeltal(&mut tx, företag, idag).await?;
    let utkast = utkast(&mut tx, företag).await?;
    tx.commit().await?;
    let mut saker: Vec<Sak> = drift::driftvarningar(db, klocka, config)
        .await?
        .into_iter()
        .map(Sak::Driftvarning)
        .collect();
    saker.extend(utkast.into_iter().map(Sak::Utkast));
    let okopplade = underlag::lista(db, aktör, företag, underlag::Urval::Okopplade).await?;
    saker.extend(okopplade.into_iter().map(Sak::Underlag));
    Ok(AttGöra {
        idag,
        nyckeltal,
        saker,
    })
}

/// Antalet saker att göra i vart och ett av företagen som aktören har åtkomst till, för
/// toppmenyn och företagsväljaren.
pub async fn antal(
    db: &Databas,
    klocka: &Klocka,
    config: &Config,
    aktör: &Aktör,
) -> Result<BTreeMap<FöretagsId, usize>, DatabasFel> {
    let företagen = foretag::mina_företag(db, aktör).await?;
    if företagen.is_empty() {
        return Ok(BTreeMap::new());
    }
    let varningar = drift::driftvarningar(db, klocka, config).await?.len();
    let mut antal: BTreeMap<FöretagsId, usize> =
        företagen.iter().map(|f| (f.id, varningar)).collect();
    let rader: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT foretag_id, count(*) FROM verifikat WHERE bokford IS NULL GROUP BY foretag_id
         UNION ALL
         SELECT u.foretag_id, count(*) FROM underlag u
         WHERE NOT EXISTS (SELECT 1 FROM underlagskoppling k WHERE k.underlag_id = u.id)
         GROUP BY u.foretag_id",
    )
    .fetch_all(db.pool())
    .await?;
    for (företag, n) in rader {
        if let Some(a) = antal.get_mut(&FöretagsId(företag)) {
            *a += usize::try_from(n).unwrap_or_default();
        }
    }
    Ok(antal)
}

async fn nyckeltal(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
    idag: Date,
) -> Result<Nyckeltal, DatabasFel> {
    let saldobalanser = saldo::saldobalanser_till(conn, företag, Some(idag)).await?;
    // Det senaste året som har börjat: dess utgående balans är saldot per i dag.
    let senaste = saldobalanser.iter().rev().find(|sb| sb.år.start() <= idag);
    let företagskontot = senaste
        .and_then(|sb| {
            sb.konton
                .iter()
                .find(|k| k.konto.get() == FÖRETAGSKONTOT)
                .map(|k| k.utgående)
        })
        .unwrap_or_default();
    let innevarande = senaste.filter(|sb| sb.år.innehåller(idag));
    let resultat_i_år = innevarande
        .map(|sb| {
            sb.konton
                .iter()
                .filter(|k| (3..=8).contains(&k.konto.klass()) && k.konto.get() != ÅRETS_RESULTAT)
                .map(|k| k.förändring)
                .sum::<Saldo>()
        })
        .map(|s| Saldo::från_öre(-s.öre()))
        .unwrap_or_default();
    Ok(Nyckeltal {
        företagskontot,
        resultat_i_år,
        år: innevarande.map(|sb| sb.år),
    })
}

async fn utkast(
    conn: &mut SqliteConnection,
    företag: FöretagsId,
) -> Result<Vec<Utkastsak>, DatabasFel> {
    type Rad = (i64, Option<String>, String, i64, String, i64);
    let rader: Vec<Rad> = sqlx::query_as(
        "SELECT v.id, v.datum, v.beskrivning,
                coalesce((SELECT sum(debet) FROM verifikatrad WHERE verifikat_id = v.id), 0),
                v.skapad,
                (SELECT count(*) FROM underlagskoppling WHERE verifikat_id = v.id)
         FROM verifikat v
         WHERE v.foretag_id = ? AND v.bokford IS NULL
         ORDER BY v.id",
    )
    .bind(företag.0)
    .fetch_all(&mut *conn)
    .await?;
    let konton: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT r.verifikat_id, r.konto FROM verifikatrad r
         JOIN verifikat v ON v.id = r.verifikat_id
         WHERE v.foretag_id = ? AND v.bokford IS NULL AND r.konto IS NOT NULL",
    )
    .bind(företag.0)
    .fetch_all(&mut *conn)
    .await?;
    rader
        .into_iter()
        .map(|(id, datum, beskrivning, summa, skapad, underlag)| {
            let kassa = regler::rör_kassan(
                konton
                    .iter()
                    .filter(|(v, _)| *v == id)
                    .filter_map(|(_, k)| u16::try_from(*k).ok().and_then(Kontonummer::new)),
            );
            Ok(Utkastsak {
                id: VerifikatId(id),
                datum: datum.as_deref().map(foretag::datum).transpose()?,
                beskrivning,
                summa: Belopp::från_öre(summa),
                skapad: tidpunkt(&skapad)?,
                underlag: usize::try_from(underlag).unwrap_or_default(),
                kassa,
            })
        })
        .collect()
}
