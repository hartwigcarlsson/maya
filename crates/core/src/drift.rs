//! Driftvarningar (beslut 11 i #70): det som är fel i driften och som alla användare ser överst
//! i Att göra, i alla företag, bara medan det är fel. Hela installationens räkenskapsinformation
//! ligger i samma databas, så varningarna gäller alla företag.

use jiff::Timestamp;
use maya_domain::drift::backupen_för_gammal;

use crate::aktor::{self, Aktör, Handling};
use crate::backup::{Backupmål, Backupstatus};
use crate::config::{Arkivplan, Config};
use crate::inloggning::tidpunkt;
use crate::{Databas, DatabasFel, Klocka, backup};

/// Något som är fel i driften.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Driftvarning {
    /// Den senaste lyckade backupen är äldre än 36 timmar, eller ingen backup har lyckats.
    BackupGammal { senaste: Option<Timestamp> },
    /// Den senaste verifieringen av en backup misslyckades.
    VerifieringMisslyckades {
        tid: Timestamp,
        backup: Option<String>,
        /// Felen, en per rad.
        meddelande: String,
    },
    /// Backupmålet ligger på samma enhet som databasen eller finns inte.
    Backupmål(String),
    /// Arkivplanen saknar fysisk plats och land (`MAYA_ARKIVPLAN_PLATS`).
    ArkivplanUtanLand,
}

impl Driftvarning {
    /// Varningen i en mening.
    pub fn rubrik(&self) -> &'static str {
        match self {
            Driftvarning::BackupGammal { .. } => "Backupen är äldre än 36 timmar",
            Driftvarning::VerifieringMisslyckades { .. } => {
                "Verifieringen av backupen misslyckades"
            }
            Driftvarning::Backupmål(_) => "Backupen är inte åtskild från databasen",
            Driftvarning::ArkivplanUtanLand => "Arkivplanen saknar land",
        }
    }

    /// Vad som är fel och vad driften kan göra åt det.
    pub fn text(&self) -> String {
        match self {
            Driftvarning::BackupGammal { senaste: Some(t) } => format!(
                "Den senaste lyckade backupen togs {}. Kontrollera backupmålet och Mayas logg.",
                tid(*t)
            ),
            Driftvarning::BackupGammal { senaste: None } => {
                "Ingen backup har lyckats än. Kontrollera backupmålet och Mayas logg.".into()
            }
            Driftvarning::VerifieringMisslyckades {
                tid: t,
                backup,
                meddelande,
            } => {
                let vilken = backup
                    .as_deref()
                    .map(|b| format!(" av {b}"))
                    .unwrap_or_default();
                format!(
                    "Verifieringen{vilken} {} hittade fel: {}",
                    tid(*t),
                    meddelande.lines().collect::<Vec<_>>().join("; ")
                )
            }
            Driftvarning::Backupmål(text) => text.clone(),
            Driftvarning::ArkivplanUtanLand => "Ange var servern står, med land, i \
                 MAYA_ARKIVPLAN_PLATS. Bokföringen ska förvaras i Sverige, och förvaring i ett \
                 annat land kräver en anmälan eller ett tillstånd (BFL 7:3 a)."
                .into(),
        }
    }
}

/// En tidpunkt i Stockholm, till minuten.
fn tid(t: Timestamp) -> String {
    t.to_zoned(maya_domain::tid::stockholm())
        .strftime("%Y-%m-%d %H:%M")
        .to_string()
}

/// Driftvarningarna just nu, i en fast ordning.
pub async fn driftvarningar(
    db: &Databas,
    klocka: &Klocka,
    config: &Config,
) -> Result<Vec<Driftvarning>, DatabasFel> {
    let status = backup::status(db).await?;
    let mut varningar = Vec::new();
    let senaste = status.senaste_lyckade_backup.as_ref().map(|k| k.tid);
    if backupen_för_gammal(klocka.nu(), senaste, installerad(db).await?) {
        varningar.push(Driftvarning::BackupGammal { senaste });
    }
    if let Some(v) = status.senaste_verifiering.filter(|v| !v.lyckades) {
        varningar.push(Driftvarning::VerifieringMisslyckades {
            tid: v.tid,
            backup: v.backup,
            meddelande: v.meddelande,
        });
    }
    varningar.extend(
        backup::varningar(&config.data_dir, &config.backup)
            .into_iter()
            .map(Driftvarning::Backupmål),
    );
    if config.arkivplan.plats.is_none() {
        varningar.push(Driftvarning::ArkivplanUtanLand);
    }
    Ok(varningar)
}

#[derive(Debug, thiserror::Error)]
pub enum DriftFel {
    #[error("kanalen får inte göra det här")]
    KanalenFårInte,
    #[error(transparent)]
    Databas(#[from] DatabasFel),
}

/// Det som sidan Installation visar. Bara läsning: konfigurationen ändras bara på servern.
#[derive(Debug, Clone)]
pub struct Installation {
    /// Mayas version.
    pub version: &'static str,
    pub publik_url: String,
    /// Backupmålet i text, t.ex. "katalogen /backup".
    pub backupmål: String,
    pub backup: Backupstatus,
    pub arkivplan: Arkivplan,
    /// `MAYA_DRIFT_EPOST`: dit larmen går.
    pub larmadress: Option<String>,
    /// Larm skickas: både SMTP och larmadressen är angivna.
    pub larm: bool,
    pub driftvarningar: Vec<Driftvarning>,
}

/// Installationen för sidan Installation (beslut 12 i #70).
pub async fn installation(
    db: &Databas,
    klocka: &Klocka,
    config: &Config,
    aktör: &Aktör,
) -> Result<Installation, DriftFel> {
    if !aktor::tillåter(aktör.kanal, Handling::LäsaInstallation) {
        return Err(DriftFel::KanalenFårInte);
    }
    Ok(Installation {
        version: env!("CARGO_PKG_VERSION"),
        publik_url: config.public_url.to_string(),
        backupmål: Backupmål::från(&config.backup).beskrivning(),
        backup: backup::status(db).await?,
        arkivplan: config.arkivplan.clone(),
        larmadress: config.drift_epost.clone(),
        larm: config.smtp.is_some() && config.drift_epost.is_some(),
        driftvarningar: driftvarningar(db, klocka, config).await?,
    })
}

/// När installationen började användas: när den första användaren skapades.
async fn installerad(db: &Databas) -> Result<Option<Timestamp>, DatabasFel> {
    let skapad: Option<String> = sqlx::query_scalar("SELECT min(skapad) FROM anvandare")
        .fetch_one(db.pool())
        .await?;
    skapad.as_deref().map(tidpunkt).transpose()
}
