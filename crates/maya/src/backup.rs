//! `maya backup verify`, det nattliga backupjobbet och larmjobbet i `maya serve`. Alla är tunna
//! adaptrar runt `maya_core::backup` och `maya_core::drift`.

use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use clap::Subcommand;
use jiff::Timestamp;
use maya_core::aktor::Aktör;
use maya_core::backup::{self, Backupmål};
use maya_core::config::Config;
use maya_core::drift;
use maya_core::{Databas, Klocka};
use maya_domain::drift::Larm;

#[derive(Subcommand)]
pub enum BackupCommand {
    /// Verifierar en backup i backupmålet, t.ex. vid den årliga provåterställningen.
    Verify {
        /// Backupens namn, t.ex. maya-20261018T010000Z. Utan namn verifieras den senaste.
        name: Option<String>,
    },
}

pub fn run(command: BackupCommand) -> ExitCode {
    crate::cli::kör_med_databas(|config, db| async move {
        let resultat = match command {
            BackupCommand::Verify { name } => verify(&config, &db, name.as_deref()).await,
        };
        db.stäng().await;
        resultat
    })
}

async fn verify(config: &Config, db: &Databas, namn: Option<&str>) -> crate::cli::Resultat {
    let mål = Backupmål::från(&config.backup);
    let verifiering = backup::verifiera(
        db,
        &Klocka::system(),
        &Aktör::cli(),
        &mål,
        namn,
        &config.arbetskatalog(),
    )
    .await
    .map_err(|e| {
        format!(
            "Maya kan inte verifiera backupen i {}: {e}",
            mål.beskrivning()
        )
    })?;

    let tid = maya_domain::tid::minut(verifiering.backup.tid);
    println!(
        "Backupen {} från {tid} i {}:",
        verifiering.backup.namn,
        mål.beskrivning()
    );
    if verifiering.lyckades() {
        println!(
            "Backupen är hel. Databasfilen stämmer med manifestet, integritetskontrollen gick \
             igenom, underlagen är oförändrade och kontrollsummorna stämmer."
        );
        Ok(())
    } else {
        println!("Backupen är inte hel:");
        for problem in &verifiering.problem {
            println!("  - {problem}");
        }
        Err("verifieringen misslyckades".into())
    }
}

/// Varningar om backupmålet vid start (A5).
pub fn varna_vid_start(config: &Config) {
    for varning in backup::varningar(&config.data_dir, &config.backup) {
        tracing::warn!("{varning}");
    }
}

/// Tar en backup varje natt på `MAYA_BACKUP_TIME` i Stockholm, som Maya självt, tills servern
/// stannar.
pub async fn nattligt_jobb(config: &Config, db: Databas) {
    let mål = Backupmål::från(&config.backup);
    let arbete = config.arbetskatalog();
    loop {
        let nästa = maya_domain::backup::nästa_körning(Timestamp::now(), config.backup_time);
        tracing::info!(
            "nästa backup {} till {}",
            maya_domain::tid::minut(nästa),
            mål.beskrivning()
        );
        vänta_till(nästa).await;
        kör(&db, &mål, &arbete).await;
    }
}

async fn kör(db: &Databas, mål: &Backupmål, arbete: &Path) {
    let resultat = backup::ta_backup(db, &Klocka::system(), &Aktör::maya(), mål, arbete).await;
    if let Ok(r) = &resultat
        && let Some(fel) = &r.gallringsfel
    {
        tracing::error!(
            "gallringen av äldre backuper efter {} misslyckades: {fel}",
            r.backup.namn
        );
    }
    match resultat {
        Ok(resultat) if resultat.verifiering.lyckades() => tracing::info!(
            "backupen {} är tagen och verifierad ({} gallrade)",
            resultat.backup.namn,
            resultat.gallrade.len()
        ),
        Ok(resultat) => tracing::error!(
            "backupen {} gick inte att verifiera: {}",
            resultat.backup.namn,
            resultat.verifiering.problem.join("; ")
        ),
        Err(e) => tracing::error!("backupen misslyckades: {e}"),
    }
}

/// Hur ofta larmjobbet ser efter fel.
const LARMINTERVALL: Duration = Duration::from_secs(10 * 60);

/// Skickar larm till driften (beslut 13 i #70) som Maya självt: ser efter fel vid start och
/// sedan var tionde minut. Utan SMTP eller larmadress loggar jobbet det en gång och slutar.
pub async fn larmjobb(config: &Config, db: Databas) {
    let saknas = match (&config.smtp, &config.drift_epost) {
        (None, _) => Some("MAYA_SMTP_HOST"),
        (Some(_), None) => Some("MAYA_DRIFT_EPOST"),
        (Some(_), Some(_)) => None,
    };
    if let Some(variabel) = saknas {
        tracing::info!("larm till driften är avstängda: {variabel} saknas");
        return;
    }
    tracing::info!(
        "larm till driften skickas till {}",
        config.drift_epost.as_deref().unwrap_or_default()
    );
    loop {
        match drift::skicka_larm(&db, &Klocka::system(), config, &Aktör::maya()).await {
            Ok(larm) => {
                for larm in larm {
                    match larm {
                        Larm::Fel(f) => tracing::warn!("larm till driften skickat: {}", f.namn()),
                        Larm::Löst(f) => {
                            tracing::info!("larm till driften skickat: löst: {}", f.namn())
                        }
                    }
                }
            }
            Err(e) => tracing::error!("{e}"),
        }
        tokio::time::sleep(LARMINTERVALL).await;
    }
}

/// Sover till `tid` i korta steg, så att en ändrad systemklocka eller en värd som har sovit inte
/// flyttar backupen.
async fn vänta_till(tid: Timestamp) {
    loop {
        let kvar = tid.duration_since(Timestamp::now());
        if kvar.is_negative() || kvar.is_zero() {
            return;
        }
        let steg = Duration::try_from(kvar)
            .unwrap_or(Duration::ZERO)
            .min(Duration::from_secs(60));
        tokio::time::sleep(steg).await;
    }
}
