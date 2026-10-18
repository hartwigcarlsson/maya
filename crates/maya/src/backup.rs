//! `maya backup verify` och det nattliga backupjobbet i `maya serve`. Båda är tunna adaptrar
//! runt `maya_core::backup`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use clap::Subcommand;
use jiff::Timestamp;
use maya_core::aktor::Aktör;
use maya_core::backup::{self, Backupmål};
use maya_core::config::Config;
use maya_core::{Databas, Klocka};

#[derive(Subcommand)]
pub enum BackupCommand {
    /// Verifierar en backup i backupmålet, t.ex. vid den årliga provåterställningen.
    Verify {
        /// Backupens namn, t.ex. maya-20261018T010000Z. Utan namn verifieras den senaste.
        name: Option<String>,
    },
}

pub fn run(command: BackupCommand) -> ExitCode {
    crate::user::kör_med_databas(|config, db| async move {
        let resultat = match command {
            BackupCommand::Verify { name } => verify(&config, &db, name.as_deref()).await,
        };
        db.stäng().await;
        resultat
    })
}

/// Där ögonblicksbilden och en hämtad backup ligger under arbetet.
fn arbetskatalog(config: &Config) -> PathBuf {
    config.data_dir.join("backup-arbete")
}

async fn verify(config: &Config, db: &Databas, namn: Option<&str>) -> crate::user::Resultat {
    let mål = Backupmål::från(&config.backup);
    let verifiering = backup::verifiera(
        db,
        &Klocka::system(),
        &Aktör::cli(),
        &mål,
        namn,
        &arbetskatalog(config),
    )
    .await
    .map_err(|e| {
        format!(
            "Maya kan inte verifiera backupen i {}: {e}",
            mål.beskrivning()
        )
    })?;

    let tid = verifiering
        .backup
        .tid
        .to_zoned(maya_domain::tid::stockholm())
        .strftime("%Y-%m-%d %H:%M");
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
    let arbete = arbetskatalog(config);
    loop {
        let nästa = maya_domain::backup::nästa_körning(Timestamp::now(), config.backup_time);
        tracing::info!(
            "nästa backup {} till {}",
            nästa
                .to_zoned(maya_domain::tid::stockholm())
                .strftime("%Y-%m-%d %H:%M"),
            mål.beskrivning()
        );
        vänta_till(nästa).await;
        kör(&db, &mål, &arbete).await;
    }
}

async fn kör(db: &Databas, mål: &Backupmål, arbete: &Path) {
    match backup::ta_backup(db, &Klocka::system(), &Aktör::maya(), mål, arbete).await {
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
