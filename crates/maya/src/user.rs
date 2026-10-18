//! `maya user …`: det som rör en användare som helhet görs bara här, på servern (ADR 0005).

use std::process::ExitCode;

use clap::Subcommand;
use maya_core::aktor::Aktör;
use maya_core::config::Config;
use maya_core::installation::{self, RelyingParty};
use maya_core::{Databas, Klocka, inloggning};

#[derive(Subcommand)]
pub enum UserCommand {
    /// Skapar den första användaren och skriver ut en länk för att registrera hens passkey.
    Create {
        /// Användarens e-postadress.
        email: String,
    },
}

pub fn run(command: UserCommand) -> ExitCode {
    let config = match Config::from_env(std::env::vars()) {
        Ok(config) => config,
        Err(e) => {
            eprint!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let runtime = tokio::runtime::Runtime::new().expect("tokio-runtime");
    match runtime.block_on(kör(&config, command)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

async fn kör(config: &Config, command: UserCommand) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(&config.data_dir)
        .map_err(|e| format!("kan inte skapa {}: {e}", config.data_dir.display()))?;
    let db = Databas::öppna(&config.database_path()).await?;
    installation::kontrollera_rp(&db, &RelyingParty::från_publik_url(&config.public_url)).await?;
    match command {
        UserCommand::Create { email } => {
            let länk =
                inloggning::skapa_första_användare(&db, &Klocka::system(), &Aktör::cli(), &email)
                    .await
                    .map_err(|e| format!("Maya kan inte skapa användaren: {e}"))?;
            let url = config
                .public_url
                .join(&format!("registrera/{}", länk.token()))?;
            println!("Användaren {} är skapad.", email.trim().to_lowercase());
            println!(
                "Öppna länken för att registrera en passkey. Den gäller i 24 timmar och kan användas en gång:"
            );
            println!();
            println!("  {url}");
        }
    }
    db.stäng().await;
    Ok(())
}
