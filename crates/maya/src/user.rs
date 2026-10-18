//! `maya user …` och `maya domain …`: det som rör en användare som helhet görs bara här, på
//! servern (ADR 0005).

use std::process::ExitCode;

use clap::Subcommand;

use crate::cli;
use maya_core::aktor::Aktör;
use maya_core::config::Config;
use maya_core::inloggning::{self, Registreringslänk};
use maya_core::installation::{self, RelyingParty};
use maya_core::{Databas, Klocka};

#[derive(Subcommand)]
pub enum UserCommand {
    /// Skapar den första användaren och skriver ut en länk för att registrera hens passkey.
    Create {
        /// Användarens e-postadress.
        email: String,
    },
    /// Skriver ut en återställningslänk för en användare som har tappat alla sina passkeys.
    Reset {
        /// Användarens e-postadress.
        email: String,
    },
    /// Avaktiverar en användare. Hen kan inte logga in, och hens sessioner avslutas.
    Deactivate {
        /// Användarens e-postadress.
        email: String,
    },
    /// Återaktiverar en avaktiverad användare.
    Reactivate {
        /// Användarens e-postadress.
        email: String,
    },
}

#[derive(Subcommand)]
pub enum DomainCommand {
    /// Byter till domänen i MAYA_PUBLIC_URL och tömmer alla passkeys.
    Change,
}

pub fn run_user(command: UserCommand) -> ExitCode {
    cli::kör_med_databas_för_domänen(|config, db| async move {
        installation::kontrollera_rp(&db, &RelyingParty::från_publik_url(&config.public_url))
            .await?;
        let resultat = user(&config, &db, command).await;
        db.stäng().await;
        resultat
    })
}

pub fn run_domain(command: DomainCommand) -> ExitCode {
    cli::kör_med_databas(|config, db| async move {
        let resultat = domain(&config, &db, command).await;
        db.stäng().await;
        resultat
    })
}

async fn user(config: &Config, db: &Databas, command: UserCommand) -> cli::Resultat {
    let klocka = Klocka::system();
    let aktör = Aktör::cli();
    match command {
        UserCommand::Create { email } => {
            let länk = inloggning::skapa_första_användare(db, &klocka, &aktör, &email)
                .await
                .map_err(|e| format!("Maya kan inte skapa användaren: {e}"))?;
            println!("Användaren {} är skapad.", normaliserad(&email));
            skriv_länk(config, &länk, "registrera en passkey")?;
        }
        UserCommand::Reset { email } => {
            let länk = inloggning::återställ_användare(db, &klocka, &aktör, &email)
                .await
                .map_err(|e| format!("Maya kan inte återställa användaren: {e}"))?;
            println!(
                "Användaren {} kan registrera en ny passkey.",
                normaliserad(&email)
            );
            skriv_länk(config, &länk, "registrera en ny passkey")?;
        }
        UserCommand::Deactivate { email } => {
            inloggning::avaktivera_användare(db, &klocka, &aktör, &email)
                .await
                .map_err(|e| format!("Maya kan inte avaktivera användaren: {e}"))?;
            println!(
                "Användaren {} är avaktiverad. Hen kan inte logga in, och hens sessioner är avslutade.",
                normaliserad(&email)
            );
        }
        UserCommand::Reactivate { email } => {
            inloggning::återaktivera_användare(db, &klocka, &aktör, &email)
                .await
                .map_err(|e| format!("Maya kan inte återaktivera användaren: {e}"))?;
            println!(
                "Användaren {} är återaktiverad och kan logga in med sina passkeys igen.",
                normaliserad(&email)
            );
        }
    }
    Ok(())
}

async fn domain(config: &Config, db: &Databas, command: DomainCommand) -> cli::Resultat {
    match command {
        DomainCommand::Change => {
            let rp = RelyingParty::från_publik_url(&config.public_url);
            installation::byt_domän(db, &Klocka::system(), &Aktör::cli(), &rp)
                .await
                .map_err(|e| format!("Maya kan inte byta domän: {e}"))?;
            println!(
                "Installationen har nu domänen {} (origin {}).",
                rp.rp_id, rp.origin
            );
            println!(
                "Alla passkeys och sessioner är borttagna, eftersom passkeys är bundna till domänen."
            );
            println!(
                "Ge varje användare en ny passkey med `maya user reset <e-post>`, och starta om Maya."
            );
        }
    }
    Ok(())
}

fn normaliserad(epost: &str) -> String {
    maya_domain::epost::normalisera(epost).unwrap_or_else(|_| epost.trim().to_string())
}

fn skriv_länk(config: &Config, länk: &Registreringslänk, för_att: &str) -> cli::Resultat {
    let url = config
        .public_url
        .join(&format!("registrera/{}", länk.token()))?;
    println!("Öppna länken för att {för_att}. Den gäller i 24 timmar och kan användas en gång:");
    println!();
    println!("  {url}");
    Ok(())
}
