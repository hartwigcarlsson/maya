//! Det som CLI-kommandona delar: konfigurationen, en runtime och databasen.

use std::process::ExitCode;

use maya_core::Databas;
use maya_core::config::Config;
use maya_core::installation::{self, RelyingParty};

/// Konfigurationen ur miljön, eller felkoden efter att problemen skrivits ut.
pub(crate) fn config() -> Result<Config, ExitCode> {
    Config::from_env(std::env::vars()).map_err(|e| {
        eprint!("{e}");
        ExitCode::FAILURE
    })
}

pub(crate) type Resultat = Result<(), Box<dyn std::error::Error>>;

pub(crate) fn kör_med_databas<F, Fut>(f: F) -> ExitCode
where
    F: FnOnce(Config, Databas) -> Fut,
    Fut: Future<Output = Resultat>,
{
    kör(false, f)
}

/// Som [`kör_med_databas`], och domänen i `MAYA_PUBLIC_URL` prövas mot databasen innan den
/// öppnas (och migreras och loggar starten).
pub(crate) fn kör_med_databas_för_domänen<F, Fut>(f: F) -> ExitCode
where
    F: FnOnce(Config, Databas) -> Fut,
    Fut: Future<Output = Resultat>,
{
    kör(true, f)
}

fn kör<F, Fut>(pröva_domänen: bool, f: F) -> ExitCode
where
    F: FnOnce(Config, Databas) -> Fut,
    Fut: Future<Output = Resultat>,
{
    let config = match config() {
        Ok(config) => config,
        Err(kod) => return kod,
    };
    let runtime = tokio::runtime::Runtime::new().expect("tokio-runtime");
    let resultat = runtime.block_on(async {
        std::fs::create_dir_all(&config.data_dir)
            .map_err(|e| format!("kan inte skapa {}: {e}", config.data_dir.display()))?;
        if pröva_domänen {
            installation::kontrollera_rp_i_fil(
                &config.database_path(),
                &RelyingParty::från_publik_url(&config.public_url),
            )
            .await?;
        }
        let db = Databas::öppna_med_backup(
            &config.database_path(),
            &maya_core::backup::Backupmål::från(&config.backup),
        )
        .await?;
        f(config, db).await
    });
    match resultat {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
