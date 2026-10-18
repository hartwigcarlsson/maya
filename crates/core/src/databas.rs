//! Databasen: en SQLite-fil som öppnas med migreringarna körda i ordning.
//!
//! En schemamigrering ändrar aldrig bokföringen i tysthet (A3): före migreringarna tas en
//! ögonblicksbild (och, med ett backupmål, en backup dit) med kontrollsummorna, efteråt räknas
//! de igen, och vid avvikelse eller fel läggs ögonblicksbilden tillbaka och databasen öppnas
//! inte. Resultatet, också en misslyckad migrering, och varje byte av programversion loggas i
//! varje företags behandlingshistorik. Schemaepoken hindrar en `1.x` från att öppna en databas från `0.x` (A4).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

use jiff::Timestamp;
use sqlx::migrate::{MigrateError, Migrator};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{SqliteConnection, SqlitePool};

use crate::Klocka;
use crate::aktor::Aktör;
use crate::backup::{self, Backupmål};
use crate::behandlingshistorik::{self, Händelse};
use crate::foretag::FöretagsId;
use crate::kontrollsumma::Kontrollsummor;

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// Schemaepoken som den här binären hör till: huvudversionen. Se [`schemaepok_för`].
pub const SCHEMAEPOK: &str = env!("CARGO_PKG_VERSION_MAJOR");

/// Den här binärens version, som loggas när den byts.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Schemaepoken för Maya-versionen `version`: huvudversionen, `0` under hela 0.x-tiden.
/// Migreringsfilerna får slås ihop före `1.0`, och den sammanslagna migreringen sätter epok `1`
/// (se migreringen `schemaepok`). En binär vägrar en databas från en annan epok (A4).
pub fn schemaepok_för(version: &str) -> &str {
    version.split('.').next().unwrap_or(version)
}

#[derive(Debug, thiserror::Error)]
pub enum DatabasFel {
    #[error("kan inte öppna databasen {path}: {source}")]
    Öppna { path: String, source: sqlx::Error },
    #[error(
        "databasen har ett nyare schema än den här Maya-versionen (migrering {0} är okänd). \
         Starta en nyare Maya, eller återställ en äldre backup med `maya restore`."
    )]
    NyareSchema(i64),
    #[error(
        "databasen bär schemaepok {funnen}, men den här Maya-versionen kräver schemaepok \
         {binärens}. Data från en annan epok följer inte med."
    )]
    AnnanEpok { funnen: String, binärens: String },
    #[error("migreringen av databasen misslyckades och databasen är som före migreringen: {0}")]
    Migrering(MigrateError),
    #[error(
        "migreringen av databasen ändrade bokföringen och har rullats tillbaka; Maya startar \
         inte. Kontrollsummorna skilde sig: {}. Starta den tidigare Maya-versionen igen.",
        avvikelser.join("; ")
    )]
    MigreringÄndradeBokföringen {
        avvikelser: Vec<String>,
        /// Backupen som togs före migreringen, när ett backupmål fanns.
        backup: Option<String>,
    },
    #[error("Maya migrerar inte databasen utan en backup, och backupen misslyckades: {0}")]
    BackupFöreMigrering(String),
    #[error(
        "migreringen rullades inte tillbaka: databasen kunde inte läggas tillbaka från {fil}: \
         {fel}. Lägg tillbaka filen som maya.db för hand innan Maya startas igen."
    )]
    Återställning { fil: String, fel: String },
    #[error("databasen innehåller ett oväntat värde i {0}")]
    Korrupt(String),
    #[error("databasfel: {0}")]
    Sql(#[from] sqlx::Error),
}

/// Handtaget till databasen. Billigt att klona.
#[derive(Debug, Clone)]
pub struct Databas {
    pool: SqlitePool,
}

/// Binären som öppnar databasen: dess version och migreringar.
#[derive(Clone, Copy)]
struct Binär<'a> {
    version: &'a str,
    migrator: &'a Migrator,
}

const DENNA: Binär<'static> = Binär {
    version: VERSION,
    migrator: &MIGRATOR,
};

/// Migreringar som kördes vid starten, för behandlingshistoriken.
#[derive(Debug, Clone)]
pub(crate) struct Migreringsresultat {
    /// Version och namn per migrering, i ordning.
    pub migreringar: Vec<(i64, String)>,
    /// Backupen före migreringen, när ett backupmål fanns.
    pub backup: Option<String>,
}

impl Databas {
    /// Öppnar databasen i `path`, skapar den om den saknas och kör migreringarna i ordning,
    /// med en ögonblicksbild före och kontrollsummor före och efter. Vägrar en databas med
    /// nyare schema än binären eller från en annan schemaepok.
    pub async fn öppna(path: &Path) -> Result<Self, DatabasFel> {
        Self::öppna_med(path, DENNA, None).await
    }

    /// Som [`Databas::öppna`], och före en schemamigrering tas också en backup till `mål`.
    /// Servern och CLI:t öppnar databasen så.
    pub async fn öppna_med_backup(path: &Path, mål: &Backupmål) -> Result<Self, DatabasFel> {
        Self::öppna_med(path, DENNA, Some(mål)).await
    }

    async fn öppna_med(
        path: &Path,
        binär: Binär<'_>,
        mål: Option<&Backupmål>,
    ) -> Result<Self, DatabasFel> {
        let öppna_fel = |source| DatabasFel::Öppna {
            path: path.display().to_string(),
            source,
        };
        let options = SqliteConnectOptions::from_str("sqlite://")
            .map_err(öppna_fel)?
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Full)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .connect_with(options)
            .await
            .map_err(öppna_fel)?;
        let db = Databas { pool };
        let epok = schemaepok_för(binär.version);

        // Epoken kontrolleras före migreringarna, så att en databas från en annan epok aldrig
        // migreras. En ny databas har ingen epok ännu.
        db.kontrollera_epok(epok).await?;
        let tid = Klocka::system().nu();
        let körda = db
            .migrera(path, binär.migrator, binär.version, tid, mål)
            .await?;
        db.kontrollera_epok(epok).await?;

        let mut tx = db.pool.begin().await?;
        logga_start(&mut tx, tid, binär.version, körda.as_ref()).await?;
        tx.commit().await?;
        Ok(db)
    }

    /// Kör de migreringar som inte är körda. En befintlig databas skyddas av en ögonblicksbild
    /// som läggs tillbaka om en migrering misslyckas eller ändrar kontrollsummorna.
    async fn migrera(
        &self,
        path: &Path,
        migrator: &Migrator,
        binär_version: &str,
        tid: Timestamp,
        mål: Option<&Backupmål>,
    ) -> Result<Option<Migreringsresultat>, DatabasFel> {
        // En ny databas har inget att skydda.
        let Some(körda) = self.körda_migreringar().await? else {
            self.kör(migrator).await?;
            return Ok(None);
        };
        if let Some(okänd) = körda.iter().find(|v| !migrator.version_exists(**v)) {
            return Err(DatabasFel::NyareSchema(*okänd));
        }
        let väntande: Vec<(i64, String)> = migrator
            .iter()
            .filter(|m| !m.migration_type.is_down_migration() && !körda.contains(&m.version))
            .map(|m| (m.version, m.description.to_string()))
            .collect();
        if väntande.is_empty() {
            // Kontrollerar ändå att de körda migreringarna är oförändrade.
            self.kör(migrator).await?;
            return Ok(None);
        }

        let arbetskatalog = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("backup-arbete");
        let före = backup::före_migrering(self, tid, mål, &arbetskatalog)
            .await
            .map_err(|e| DatabasFel::BackupFöreMigrering(e.to_string()))?;
        let backup = före.backup.map(|b| b.namn);
        tracing::info!(
            "{} migreringar att köra; ögonblicksbild {}{}",
            väntande.len(),
            före.fil.display(),
            backup
                .as_deref()
                .map(|b| format!(", backup {b}"))
                .unwrap_or_default()
        );

        let resultat = async {
            self.kör(migrator).await?;
            let efter = Kontrollsummor::räkna(self).await?;
            let avvikelser = före.kontrollsummor.avvikelser(&efter);
            if avvikelser.is_empty() {
                Ok(())
            } else {
                Err(DatabasFel::MigreringÄndradeBokföringen {
                    avvikelser,
                    backup: backup.clone(),
                })
            }
        }
        .await;
        match resultat {
            Ok(()) => {
                let _ = std::fs::remove_file(&före.fil);
                tracing::info!("migreringarna är körda och kontrollsummorna oförändrade");
                Ok(Some(Migreringsresultat {
                    migreringar: väntande,
                    backup,
                }))
            }
            Err(fel) => {
                tracing::error!("{fel}");
                self.lägg_tillbaka(path, &före.fil).await?;
                let försök = Migreringsresultat {
                    migreringar: väntande,
                    backup,
                };
                // Försöket ska stå kvar i den återlagda databasen. Går det inte att logga är
                // det migreringens fel som visas.
                if let Err(e) = logga_misslyckad(path, tid, binär_version, &försök, &fel).await {
                    tracing::error!("den misslyckade migreringen kunde inte loggas: {e}");
                }
                Err(fel)
            }
        }
    }

    /// De migreringar som är körda, eller `None` för en ny databas.
    async fn körda_migreringar(&self) -> Result<Option<Vec<i64>>, DatabasFel> {
        let finns: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master
                            WHERE type = 'table' AND name = '_sqlx_migrations')",
        )
        .fetch_one(&self.pool)
        .await?;
        if !finns {
            return Ok(None);
        }
        Ok(Some(
            sqlx::query_scalar("SELECT version FROM _sqlx_migrations ORDER BY version")
                .fetch_all(&self.pool)
                .await?,
        ))
    }

    /// Kör migreringarna på en egen anslutning som sedan stängs: en migrering utan transaktion
    /// kan ändra anslutningens inställningar (t.ex. `foreign_keys`).
    async fn kör(&self, migrator: &Migrator) -> Result<(), DatabasFel> {
        let mut conn = self.pool.acquire().await?;
        let resultat = migrator.run(&mut *conn).await;
        conn.close().await?;
        resultat.map_err(|e| match e {
            MigrateError::VersionMissing(version) => DatabasFel::NyareSchema(version),
            other => DatabasFel::Migrering(other),
        })
    }

    /// Stänger databasen och lägger ögonblicksbilden `fil` tillbaka som databasfilen.
    async fn lägg_tillbaka(&self, path: &Path, fil: &Path) -> Result<(), DatabasFel> {
        self.pool.close().await;
        let fel = |e: std::io::Error| DatabasFel::Återställning {
            fil: fil.display().to_string(),
            fel: e.to_string(),
        };
        let med_ändelse = |ändelse: &str| {
            let mut namn = OsString::from(path.as_os_str());
            namn.push(ändelse);
            PathBuf::from(namn)
        };
        let tillfällig = med_ändelse("-aterstallning");
        std::fs::copy(fil, &tillfällig).map_err(fel)?;
        // Loggen hör till den migrerade filen och får inte läsas in i den återlagda.
        for ändelse in ["-wal", "-shm"] {
            match std::fs::remove_file(med_ändelse(ändelse)) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(fel(e)),
                _ => {}
            }
        }
        std::fs::rename(&tillfällig, path).map_err(fel)?;
        let _ = std::fs::remove_file(fil);
        tracing::info!("databasen är som före migreringen");
        Ok(())
    }

    /// Schemaepoken som databasen bär.
    pub async fn schemaepok(&self) -> Result<String, DatabasFel> {
        Ok(sqlx::query_scalar("SELECT epok FROM schemaepok")
            .fetch_one(&self.pool)
            .await?)
    }

    /// Kontrollerar att databasen svarar, för `/healthz`.
    pub async fn kontrollera(&self) -> Result<(), DatabasFel> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }

    pub(crate) fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Stänger alla anslutningar.
    pub async fn stäng(self) {
        self.pool.close().await;
    }

    async fn kontrollera_epok(&self, epok: &str) -> Result<(), DatabasFel> {
        let finns: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'schemaepok')",
        )
        .fetch_one(&self.pool)
        .await?;
        if !finns {
            return Ok(());
        }
        let funnen = self.schemaepok().await?;
        if funnen != epok {
            return Err(DatabasFel::AnnanEpok {
                funnen,
                binärens: epok.to_string(),
            });
        }
        Ok(())
    }
}

/// Loggar en migrering som misslyckades eller ändrade bokföringen i varje företags
/// behandlingshistorik i den återlagda databasen `path`, med Maya som aktör.
async fn logga_misslyckad(
    path: &Path,
    tid: Timestamp,
    version: &str,
    försök: &Migreringsresultat,
    fel: &DatabasFel,
) -> Result<(), sqlx::Error> {
    use sqlx::Connection;
    let options = SqliteConnectOptions::new()
        .filename(path)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5));
    let mut conn = SqliteConnection::connect_with(&options).await?;
    let mut tx = conn.begin().await?;
    logga_misslyckad_i(&mut tx, tid, version, försök, &fel.to_string()).await?;
    tx.commit().await?;
    conn.close().await
}

pub(crate) async fn logga_misslyckad_i(
    conn: &mut SqliteConnection,
    tid: Timestamp,
    version: &str,
    försök: &Migreringsresultat,
    fel: &str,
) -> Result<(), sqlx::Error> {
    let företag: Vec<i64> = sqlx::query_scalar("SELECT id FROM foretag ORDER BY id")
        .fetch_all(&mut *conn)
        .await?;
    let senaste = försök.migreringar.last().map(|(v, _)| v.to_string());
    for id in företag {
        behandlingshistorik::skriv(
            conn,
            FöretagsId(id),
            tid,
            &Aktör::maya(),
            Händelse::MigreringMisslyckad,
            senaste.as_deref().unwrap_or_default(),
            serde_json::json!({
                "migreringar": migreringar_json(försök),
                "programversion": version,
                "fel": fel,
                "backup": försök.backup,
            }),
        )
        .await?;
    }
    Ok(())
}

fn migreringar_json(körda: &Migreringsresultat) -> Vec<serde_json::Value> {
    körda
        .migreringar
        .iter()
        .map(|(version, namn)| serde_json::json!({"version": version, "namn": namn}))
        .collect()
}

/// Loggar starten i varje företags behandlingshistorik med Maya som aktör: migreringarna som
/// kördes, och programversionen om den har bytts sedan förra starten.
pub(crate) async fn logga_start(
    conn: &mut SqliteConnection,
    tid: Timestamp,
    version: &str,
    körda: Option<&Migreringsresultat>,
) -> Result<(), sqlx::Error> {
    let tidigare: Option<String> = sqlx::query_scalar("SELECT version FROM programversion")
        .fetch_optional(&mut *conn)
        .await?;
    let bytt = tidigare.as_deref() != Some(version);
    let företag: Vec<i64> = sqlx::query_scalar("SELECT id FROM foretag ORDER BY id")
        .fetch_all(&mut *conn)
        .await?;
    let maya = Aktör::maya();
    for id in företag {
        if let Some(körda) = körda {
            let senaste = körda.migreringar.last().map(|(v, _)| v.to_string());
            behandlingshistorik::skriv(
                conn,
                FöretagsId(id),
                tid,
                &maya,
                Händelse::MigreringKörd,
                &senaste.unwrap_or_default(),
                serde_json::json!({
                    "migreringar": migreringar_json(körda),
                    "programversion": version,
                    "kontrollsummor": "oförändrade",
                    "backup": körda.backup,
                }),
            )
            .await?;
        }
        if bytt {
            behandlingshistorik::skriv(
                conn,
                FöretagsId(id),
                tid,
                &maya,
                Händelse::ProgramversionBytt,
                version,
                serde_json::json!({ "från": tidigare, "till": version }),
            )
            .await?;
        }
    }
    if bytt {
        sqlx::query(
            "INSERT INTO programversion (id, version) VALUES (1, ?)
             ON CONFLICT (id) DO UPDATE SET version = excluded.version",
        )
        .bind(version)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

#[cfg(feature = "test-support")]
mod test_support {
    use std::path::Path;

    use sqlx::migrate::Migrator;

    use super::{Binär, Databas, DatabasFel};
    use crate::backup::Backupmål;

    /// En ny databas i en tillfällig katalog med migreringarna körda. Katalogen tas bort när
    /// värdet släpps.
    pub struct TillfälligDatabas {
        db: Databas,
        _dir: tempfile::TempDir,
    }

    impl std::ops::Deref for TillfälligDatabas {
        type Target = Databas;
        fn deref(&self) -> &Databas {
            &self.db
        }
    }

    impl Databas {
        /// En ny, tom databas per test (ADR 0003: sömmen är `core` mot en tillfällig SQLite).
        pub async fn tillfällig() -> TillfälligDatabas {
            let dir = tempfile::tempdir().expect("tillfällig katalog");
            let db = Databas::öppna(&dir.path().join("maya.db"))
                .await
                .expect("tillfällig databas");
            TillfälligDatabas { db, _dir: dir }
        }

        /// Öppnar databasen som en Maya med versionen `version` och migreringarna i
        /// `migrator` skulle göra, t.ex. en senare version med en migrering till.
        pub async fn öppna_som(
            path: &Path,
            version: &str,
            migrator: &Migrator,
            mål: Option<&Backupmål>,
        ) -> Result<Databas, DatabasFel> {
            Databas::öppna_med(path, Binär { version, migrator }, mål).await
        }
    }
}

#[cfg(feature = "test-support")]
pub use test_support::TillfälligDatabas;
