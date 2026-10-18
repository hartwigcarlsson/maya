//! Databasen: en SQLite-fil som öppnas med migreringarna körda i ordning.

use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use sqlx::SqlitePool;
use sqlx::migrate::{MigrateError, Migrator};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// Schemaepoken som den här binären hör till. Se migreringen `schemaepok`.
pub const SCHEMAEPOK: &str = "0";

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
         {SCHEMAEPOK}. Data från en annan epok följer inte med."
    )]
    AnnanEpok { funnen: String },
    #[error("migreringen av databasen misslyckades: {0}")]
    Migrering(MigrateError),
    #[error("databasfel: {0}")]
    Sql(#[from] sqlx::Error),
}

/// Handtaget till databasen. Billigt att klona.
#[derive(Debug, Clone)]
pub struct Databas {
    pool: SqlitePool,
}

impl Databas {
    /// Öppnar databasen i `path`, skapar den om den saknas och kör migreringarna i ordning.
    /// Vägrar en databas med nyare schema än binären eller från en annan schemaepok.
    pub async fn öppna(path: &Path) -> Result<Self, DatabasFel> {
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

        // Epoken kontrolleras före migreringarna, så att en databas från en annan epok aldrig
        // migreras. En ny databas har ingen epok ännu.
        db.kontrollera_epok().await?;
        MIGRATOR.run(&db.pool).await.map_err(|e| match e {
            MigrateError::VersionMissing(version) => DatabasFel::NyareSchema(version),
            other => DatabasFel::Migrering(other),
        })?;
        db.kontrollera_epok().await?;
        Ok(db)
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

    /// Stänger alla anslutningar.
    pub async fn stäng(self) {
        self.pool.close().await;
    }

    async fn kontrollera_epok(&self) -> Result<(), DatabasFel> {
        let finns: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'schemaepok')",
        )
        .fetch_one(&self.pool)
        .await?;
        if !finns {
            return Ok(());
        }
        let funnen = self.schemaepok().await?;
        if funnen != SCHEMAEPOK {
            return Err(DatabasFel::AnnanEpok { funnen });
        }
        Ok(())
    }
}

#[cfg(feature = "test-support")]
mod test_support {
    use super::Databas;

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
    }
}

#[cfg(feature = "test-support")]
pub use test_support::TillfälligDatabas;
