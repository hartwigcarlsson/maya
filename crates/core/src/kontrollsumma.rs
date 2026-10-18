//! Kontrollsummor över räkenskapsinformationen (beslut i #13): antal bokförda verifikat per
//! serie och räkenskapsår, debet och kredit per konto och räkenskapsår, en hash över de bokförda
//! raderna och underlagens hashar. Backupen, schemamigreringarna och återställningen räknar dem
//! före och efter och jämför (A4, A5).
//!
//! Företag och räkenskapsår anges med organisationsnummer och startdatum, inte med radernas
//! id, så att summorna går att jämföra också efter en migrering som bygger om tabeller.
//! Behandlingshistoriken ingår inte: en migrering och en återställning skriver själva poster där.
//!
//! [`Kontrollsummor::räkna`] läser en öppen databas, [`granska_fil`] en databasfil som inte ska
//! migreras (en backup), och `räkna_i` (inom `core`) en enskild anslutning, t.ex. runt en
//! migrering.

use std::path::Path;
use std::str::FromStr;

use jiff::civil::Date;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Connection, SqliteConnection};

use crate::{Databas, DatabasFel};

/// Serien som alla verifikat bokförs i under etapp 1.
const SERIE: &str = "V";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kontrollsummor {
    /// Antal bokförda verifikat per företag, räkenskapsår och serie, i den ordningen.
    pub verifikat: Vec<Verifikatantal>,
    /// Summa debet och kredit i öre per företag, räkenskapsår och konto, i den ordningen.
    pub konton: Vec<Kontosumma>,
    /// SHA-256 (hex) över alla bokförda verifikat med rader och underlagskopplingar, i
    /// företagets, årets och numrets ordning.
    pub bokförda_rader: String,
    /// Varje underlags SHA-256 (hex), räknad ur innehållet, i id-ordning.
    pub underlag: Vec<Underlagshash>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verifikatantal {
    pub orgnr: String,
    /// Räkenskapsårets startdatum.
    pub räkenskapsår: Date,
    pub serie: String,
    pub antal: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kontosumma {
    pub orgnr: String,
    pub räkenskapsår: Date,
    pub konto: u16,
    /// Öre.
    pub debet: i64,
    /// Öre.
    pub kredit: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Underlagshash {
    pub orgnr: String,
    pub id: i64,
    pub sha256: String,
}

/// En databasfil som har granskats: integritetskontrollen, underlagens lagrade hashar mot
/// innehållet, och kontrollsummorna.
#[derive(Debug, Clone)]
pub struct Granskning {
    pub kontrollsummor: Kontrollsummor,
    /// Det som är fel, på svenska. Tomt när filen är hel.
    pub problem: Vec<String>,
}

impl Kontrollsummor {
    /// Kontrollsummorna för databasen just nu.
    pub async fn räkna(db: &Databas) -> Result<Self, DatabasFel> {
        let mut conn = db.pool().acquire().await?;
        // En lästransaktion: alla summor ur samma ögonblicksbild.
        let mut tx = conn.begin().await?;
        let summor = räkna_i(&mut tx).await?;
        tx.commit().await?;
        Ok(summor)
    }

    /// Skillnaderna från `self` (före) till `efter`, på svenska. Tomt när summorna stämmer.
    pub fn avvikelser(&self, efter: &Kontrollsummor) -> Vec<String> {
        let mut avvikelser = Vec::new();

        let antal = |l: &[Verifikatantal]| {
            l.iter()
                .map(|v| ((v.orgnr.clone(), v.räkenskapsår, v.serie.clone()), v.antal))
                .collect::<std::collections::BTreeMap<_, _>>()
        };
        jämför(
            &antal(&self.verifikat),
            &antal(&efter.verifikat),
            |(orgnr, år, serie), f, e| {
                avvikelser.push(format!(
                "antal verifikat i serie {serie} för {orgnr} räkenskapsåret som börjar {år}: {} före, {} efter",
                f.copied().unwrap_or(0),
                e.copied().unwrap_or(0),
            ))
            },
        );

        let summor = |l: &[Kontosumma]| {
            l.iter()
                .map(|k| {
                    (
                        (k.orgnr.clone(), k.räkenskapsår, k.konto),
                        (k.debet, k.kredit),
                    )
                })
                .collect::<std::collections::BTreeMap<_, _>>()
        };
        jämför(
            &summor(&self.konton),
            &summor(&efter.konton),
            |(orgnr, år, konto), f, e| {
                let text = |s: Option<&(i64, i64)>| {
                    let (d, k) = s.copied().unwrap_or((0, 0));
                    format!("debet {d}, kredit {k}")
                };
                avvikelser.push(format!(
                "konto {konto} för {orgnr} räkenskapsåret som börjar {år}: {} öre före, {} öre efter",
                text(f),
                text(e),
            ))
            },
        );

        if self.bokförda_rader != efter.bokförda_rader {
            avvikelser.push("hashen över bokförda rader skiljer sig".to_string());
        }

        let underlag = |l: &[Underlagshash]| {
            l.iter()
                .map(|u| (u.id, (u.orgnr.clone(), u.sha256.clone())))
                .collect::<std::collections::BTreeMap<_, _>>()
        };
        jämför(
            &underlag(&self.underlag),
            &underlag(&efter.underlag),
            |id, f, e| {
                avvikelser.push(match (f, e) {
                    (Some(_), None) => format!("underlag {id} saknas efter"),
                    (None, Some(_)) => format!("underlag {id} finns bara efter"),
                    _ => format!("underlag {id} har ändrats (SHA-256 skiljer sig)"),
                })
            },
        );
        avvikelser
    }
}

/// Anropar `skillnad(nyckel, före, efter)` för varje nyckel vars värde skiljer sig.
fn jämför<K: Ord, V: PartialEq>(
    före: &std::collections::BTreeMap<K, V>,
    efter: &std::collections::BTreeMap<K, V>,
    mut skillnad: impl FnMut(&K, Option<&V>, Option<&V>),
) {
    let nycklar: std::collections::BTreeSet<&K> = före.keys().chain(efter.keys()).collect();
    for nyckel in nycklar {
        let (f, e) = (före.get(nyckel), efter.get(nyckel));
        if f != e {
            skillnad(nyckel, f, e);
        }
    }
}

/// Granskar databasfilen i `path` utan att ändra den: `PRAGMA integrity_check`, varje underlags
/// innehåll mot dess lagrade SHA-256, och kontrollsummorna. Filen migreras inte.
pub async fn granska_fil(path: &Path) -> Result<Granskning, DatabasFel> {
    let öppna_fel = |source| DatabasFel::Öppna {
        path: path.display().to_string(),
        source,
    };
    let options = SqliteConnectOptions::from_str("sqlite://")
        .map_err(öppna_fel)?
        .filename(path)
        .read_only(true);
    let mut conn = SqliteConnection::connect_with(&options)
        .await
        .map_err(öppna_fel)?;
    let mut problem = Vec::new();

    let integritet: Vec<String> = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_all(&mut conn)
        .await?;
    if integritet != ["ok"] {
        problem.extend(
            integritet
                .into_iter()
                .map(|p| format!("integritetskontrollen: {p}")),
        );
    }

    let lagrade: Vec<(i64, String)> = sqlx::query_as("SELECT id, sha256 FROM underlag ORDER BY id")
        .fetch_all(&mut conn)
        .await?;
    let kontrollsummor = räkna_i(&mut conn).await?;
    for ((id, lagrad), räknad) in lagrade.iter().zip(&kontrollsummor.underlag) {
        if *lagrad != räknad.sha256 {
            problem.push(format!(
                "underlag {id}: innehållet stämmer inte med underlagets SHA-256"
            ));
        }
    }
    conn.close().await?;
    Ok(Granskning {
        kontrollsummor,
        problem,
    })
}

/// Kontrollsummorna i anslutningen `conn`. Kör i en transaktion för en enhetlig bild.
pub(crate) async fn räkna_i(conn: &mut SqliteConnection) -> Result<Kontrollsummor, DatabasFel> {
    let datum = |text: String| {
        text.parse::<Date>()
            .map_err(|_| DatabasFel::Korrupt(format!("räkenskapsårets start {text}")))
    };

    let rader: Vec<(String, String, i64)> = sqlx::query_as(
        "SELECT f.orgnr, r.start, count(*)
         FROM verifikat v JOIN foretag f ON f.id = v.foretag_id
              JOIN rakenskapsar r ON r.id = v.rakenskapsar_id
         WHERE v.bokford IS NOT NULL
         GROUP BY f.orgnr, r.start ORDER BY f.orgnr, r.start",
    )
    .fetch_all(&mut *conn)
    .await?;
    let verifikat = rader
        .into_iter()
        .map(|(orgnr, start, antal)| {
            Ok(Verifikatantal {
                orgnr,
                räkenskapsår: datum(start)?,
                serie: SERIE.to_string(),
                antal: antal as u64,
            })
        })
        .collect::<Result<_, DatabasFel>>()?;

    let rader: Vec<(String, String, i64, i64, i64)> = sqlx::query_as(
        "SELECT f.orgnr, r.start, vr.konto, sum(vr.debet), sum(vr.kredit)
         FROM verifikatrad vr JOIN verifikat v ON v.id = vr.verifikat_id
              JOIN foretag f ON f.id = v.foretag_id
              JOIN rakenskapsar r ON r.id = v.rakenskapsar_id
         WHERE v.bokford IS NOT NULL
         GROUP BY f.orgnr, r.start, vr.konto ORDER BY f.orgnr, r.start, vr.konto",
    )
    .fetch_all(&mut *conn)
    .await?;
    let konton = rader
        .into_iter()
        .map(|(orgnr, start, konto, debet, kredit)| {
            Ok(Kontosumma {
                orgnr,
                räkenskapsår: datum(start)?,
                konto: u16::try_from(konto)
                    .map_err(|_| DatabasFel::Korrupt(format!("konto {konto}")))?,
                debet,
                kredit,
            })
        })
        .collect::<Result<_, DatabasFel>>()?;

    let bokförda_rader = hash_över_bokförda_rader(conn).await?;

    let mut underlag = Vec::new();
    let ider: Vec<(i64, String)> = sqlx::query_as(
        "SELECT u.id, f.orgnr FROM underlag u JOIN foretag f ON f.id = u.foretag_id ORDER BY u.id",
    )
    .fetch_all(&mut *conn)
    .await?;
    // Ett underlag i taget: innehållet kan vara 25 MB.
    for (id, orgnr) in ider {
        let innehåll: Vec<u8> = sqlx::query_scalar("SELECT innehall FROM underlag WHERE id = ?")
            .bind(id)
            .fetch_one(&mut *conn)
            .await?;
        underlag.push(Underlagshash {
            orgnr,
            id,
            sha256: hex::encode(Sha256::digest(&innehåll)),
        });
    }

    Ok(Kontrollsummor {
        verifikat,
        konton,
        bokförda_rader,
        underlag,
    })
}

/// SHA-256 över en rad JSON per bokfört verifikat, verifikatrad och underlagskoppling. JSON gör
/// fälten entydiga oavsett vad texterna innehåller.
async fn hash_över_bokförda_rader(conn: &mut SqliteConnection) -> Result<String, DatabasFel> {
    let mut hash = Sha256::new();
    let mut rad = |värde: serde_json::Value| {
        hash.update(värde.to_string());
        hash.update(b"\n");
    };

    type Rad = (i64, Option<i64>, i64, i64, Option<String>);
    type Verifikatrad = (
        i64,
        String,
        String,
        i64,
        Option<String>,
        String,
        String,
        Option<String>,
        Option<i64>,
    );
    let verifikat: Vec<Verifikatrad> = sqlx::query_as(
        "SELECT v.id, f.orgnr, r.start, v.nummer, v.datum, v.beskrivning, v.bokford,
                v.bokford_kanal, v.bokford_av
         FROM verifikat v JOIN foretag f ON f.id = v.foretag_id
              JOIN rakenskapsar r ON r.id = v.rakenskapsar_id
         WHERE v.bokford IS NOT NULL
         ORDER BY f.orgnr, r.start, v.nummer",
    )
    .fetch_all(&mut *conn)
    .await?;
    for (id, orgnr, start, nummer, datum, beskrivning, bokförd, kanal, av) in verifikat {
        rad(serde_json::json!([
            "verifikat",
            orgnr,
            start,
            SERIE,
            nummer,
            datum,
            beskrivning,
            bokförd,
            kanal,
            av
        ]));
        let rader: Vec<Rad> = sqlx::query_as(
            "SELECT radnr, konto, debet, kredit, text FROM verifikatrad
             WHERE verifikat_id = ? ORDER BY radnr",
        )
        .bind(id)
        .fetch_all(&mut *conn)
        .await?;
        for (radnr, konto, debet, kredit, text) in rader {
            rad(serde_json::json!([
                "rad", radnr, konto, debet, kredit, text
            ]));
        }
        let kopplingar: Vec<(String, String, i64)> = sqlx::query_as(
            "SELECT u.sha256, k.kopplad, k.komplettering
             FROM underlagskoppling k JOIN underlag u ON u.id = k.underlag_id
             WHERE k.verifikat_id = ? ORDER BY u.sha256, k.kopplad",
        )
        .bind(id)
        .fetch_all(&mut *conn)
        .await?;
        for (sha256, kopplad, komplettering) in kopplingar {
            rad(serde_json::json!([
                "underlag",
                sha256,
                kopplad,
                komplettering
            ]));
        }
    }
    Ok(hex::encode(hash.finalize()))
}
