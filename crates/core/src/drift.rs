//! Driftvarningar (beslut 11 i #70): det som är fel i driften och som alla användare ser överst
//! i Att göra, i alla företag, bara medan det är fel. Hela installationens räkenskapsinformation
//! ligger i samma databas, så varningarna gäller alla företag.

use jiff::Timestamp;
use maya_domain::drift::{Larm, Larmfel, Skickat, backupen_för_gammal};

use crate::aktor::{self, Aktör, Handling};
use crate::backup::{Backupmål, Backupstatus};
use crate::config::{Arkivplan, Config};
use crate::inloggning::tidpunkt;
use crate::{Databas, DatabasFel, Klocka, backup, installationslogg, smtp};

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
    /// Larmet kunde inte skickas; det försöks igen nästa gång.
    #[error("larmet kunde inte skickas: {0}")]
    Smtp(String),
    #[error(transparent)]
    Databas(#[from] DatabasFel),
}

impl From<sqlx::Error> for DriftFel {
    fn from(e: sqlx::Error) -> Self {
        DriftFel::Databas(e.into())
    }
}

/// Skickar larmen till driften som behövs just nu (beslut 13 i #70): när backupen eller
/// verifieringen misslyckas eller backupen är äldre än 36 timmar, högst ett per dygn och fel,
/// och ett när felet är löst. Utan `MAYA_SMTP_HOST` eller `MAYA_DRIFT_EPOST` skickas inget.
/// Bara Maya självt; ger larmen som skickades. Ett larm som inte gick att skicka försöks igen
/// vid nästa anrop.
pub async fn skicka_larm(
    db: &Databas,
    klocka: &Klocka,
    config: &Config,
    aktör: &Aktör,
) -> Result<Vec<Larm>, DriftFel> {
    if !aktor::tillåter(aktör.kanal, Handling::SkickaLarm) {
        return Err(DriftFel::KanalenFårInte);
    }
    let (Some(smtp), Some(till)) = (&config.smtp, &config.drift_epost) else {
        return Ok(Vec::new());
    };
    let nu = klocka.nu();
    let aktiva = larmfel(db, config, nu).await?;
    let rader: Vec<(String, String)> = sqlx::query_as("SELECT fel, skickat FROM driftlarm")
        .fetch_all(db.pool())
        .await?;
    let skickade = rader
        .iter()
        .filter_map(|(fel, tid)| {
            Some(Ok(Skickat {
                fel: Larmfel::från_kod(fel)?,
                tid: match tidpunkt(tid) {
                    Ok(t) => t,
                    Err(e) => return Some(Err(e)),
                },
            }))
        })
        .collect::<Result<Vec<_>, DatabasFel>>()?;
    let fel: Vec<Larmfel> = aktiva.iter().map(|(f, _)| *f).collect();

    let värd = config.public_host();
    let från = smtp
        .from
        .clone()
        .unwrap_or_else(|| format!("maya@{}", config.public_url.host_str().unwrap_or("maya")));
    let mut skickat = Vec::new();
    let mut misslyckade = Vec::new();
    for larm in maya_domain::drift::larm(nu, &fel, &skickade) {
        let (ämne, text) = match larm {
            Larm::Fel(f) => {
                let detalj = aktiva
                    .iter()
                    .find(|(a, _)| *a == f)
                    .map(|(_, d)| d.as_str())
                    .unwrap_or_default();
                (
                    format!("Maya: {}", f.namn()),
                    format!(
                        "Maya på {url} har ett fel i driften: {namn}.\n\n{detalj}\n\nMaya \
                         mejlar om felet igen tidigast om ett dygn, och en gång när det är \
                         löst. Läget finns på sidan Installation: {url}installation\n",
                        url = config.public_url,
                        namn = f.namn(),
                    ),
                )
            }
            Larm::Löst(f) => (
                format!("Maya: löst: {}", f.namn()),
                format!(
                    "Felet i driften av Maya på {url} är löst: {namn}.\n\nLäget finns på sidan \
                     Installation: {url}installation\n",
                    url = config.public_url,
                    namn = f.namn(),
                ),
            ),
        };
        let (smtp, värd, från, till) = (smtp.clone(), värd.clone(), från.clone(), till.clone());
        let utfall = tokio::task::spawn_blocking(move || {
            let brev = smtp::Brev {
                från: &från,
                till: &till,
                ämne: &ämne,
                text: &text,
            };
            smtp::skicka(&smtp, &värd, &brev, nu)
        })
        .await
        .map_err(|e| e.to_string())
        .and_then(|r| r);
        // Ett larm som inte kom fram stoppar inte de andra; det försöks igen nästa gång.
        if let Err(e) = utfall {
            tracing::error!("larmet {larm:?} kunde inte skickas: {e}");
            misslyckade.push(e);
            continue;
        }
        match larm {
            Larm::Fel(f) => {
                sqlx::query(
                    "INSERT INTO driftlarm (fel, skickat) VALUES (?1, ?2)
                     ON CONFLICT (fel) DO UPDATE SET skickat = ?2",
                )
                .bind(f.kod())
                .bind(nu.to_string())
                .execute(db.pool())
                .await?;
            }
            Larm::Löst(f) => {
                sqlx::query("DELETE FROM driftlarm WHERE fel = ?")
                    .bind(f.kod())
                    .execute(db.pool())
                    .await?;
            }
        }
        skickat.push(larm);
    }
    if misslyckade.is_empty() {
        Ok(skickat)
    } else {
        Err(DriftFel::Smtp(misslyckade.join("; ")))
    }
}

/// Felen som driften får larm om just nu, med en beskrivning för mejlet.
async fn larmfel(
    db: &Databas,
    config: &Config,
    nu: Timestamp,
) -> Result<Vec<(Larmfel, String)>, DatabasFel> {
    let status = backup::status(db).await?;
    let mut fel = Vec::new();
    if let Some(b) = status.senaste_backup.filter(|b| !b.lyckades) {
        fel.push((
            Larmfel::BackupMisslyckades,
            format!(
                "Backupen {} till {} misslyckades: {}",
                tid(b.tid),
                Backupmål::från(&config.backup).beskrivning(),
                b.meddelande.lines().collect::<Vec<_>>().join("; ")
            ),
        ));
    }
    let senaste = status.senaste_lyckade_backup.as_ref().map(|k| k.tid);
    if backupen_för_gammal(nu, senaste, installerad(db).await?) {
        fel.push((
            Larmfel::BackupGammal,
            Driftvarning::BackupGammal { senaste }.text(),
        ));
    }
    if let Some(v) = status.senaste_verifiering.filter(|v| !v.lyckades) {
        fel.push((
            Larmfel::VerifieringMisslyckades,
            Driftvarning::VerifieringMisslyckades {
                tid: v.tid,
                backup: v.backup,
                meddelande: v.meddelande,
            }
            .text(),
        ));
    }
    Ok(fel)
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
    /// Den installationsövergripande loggen, nyast först.
    pub logg: Vec<installationslogg::Loggpost>,
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
        logg: {
            let mut logg = installationslogg::läs(db).await?;
            logg.reverse();
            logg
        },
    })
}

/// När installationen började användas: när den första användaren skapades.
async fn installerad(db: &Databas) -> Result<Option<Timestamp>, DatabasFel> {
    let skapad: Option<String> = sqlx::query_scalar("SELECT min(skapad) FROM anvandare")
        .fetch_one(db.pool())
        .await?;
    skapad.as_deref().map(tidpunkt).transpose()
}
