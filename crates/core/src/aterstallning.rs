//! Återställning av hela installationen ur en backup med `maya restore` (beslut i #13, A5).
//!
//! Backupen hämtas och granskas mot sitt manifest innan något ändras, och en backup vars
//! `rp_id` skiljer sig från konfigurationen vägras, eftersom passkeys är bundna till domänen.
//! Den nuvarande databasen flyttas undan (den raderas aldrig), backupen läggs på dess plats och
//! öppnas med migreringarna körda, och kontrollsummorna jämförs med manifestet igen. Stämmer de
//! inte läggs den gamla databasen tillbaka. Återställningen loggas i varje företags
//! behandlingshistorik och i den installationsövergripande loggen, och varje användare får en
//! varning under Att göra som hen måste kvittera.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use jiff::Timestamp;
use sqlx::Connection;
use sqlx::sqlite::{SqliteConnectOptions, SqliteConnection};

use crate::aktor::{self, Aktör, Handling};
use crate::backup::{self, Backup, BackupFel, Backupmål, Manifest};
use crate::behandlingshistorik;
use crate::foretag::FöretagsId;
use crate::installation::RelyingParty;
use crate::installationslogg;
use crate::kontrollsumma::{self, Kontrollsummor};
use crate::{Databas, DatabasFel, Klocka, SCHEMAEPOK};

#[derive(Debug, thiserror::Error)]
pub enum ÅterställningsFel {
    #[error("kanalen får inte göra det här")]
    KanalenFårInte,
    #[error(
        "backupen gjordes för rp_id {backupens}, men konfigurationen ger rp_id \
         {konfigurationens}. Passkeys är bundna till domänen, så ingen skulle kunna logga in. \
         Sätt MAYA_PUBLIC_URL till backupens domän och återställ igen; byt sedan domän med \
         `maya domain change` om det behövs."
    )]
    AnnatRpId {
        backupens: String,
        konfigurationens: String,
    },
    #[error(
        "backupen bär schemaepok {backupens}, men den här Maya-versionen kräver schemaepok \
         {SCHEMAEPOK}. Data från en annan epok följer inte med."
    )]
    AnnanEpok { backupens: String },
    #[error("backupen är inte hel: {}", .0.join("; "))]
    BackupenÄrInteHel(Vec<String>),
    #[error(
        "den återställda installationen stämmer inte med backupen, och den tidigare databasen \
         är tillbaka: {}",
        .0.join("; ")
    )]
    VerifieringenMisslyckades(Vec<String>),
    #[error("{0}")]
    Fil(String),
    #[error(transparent)]
    Backup(#[from] BackupFel),
    #[error(transparent)]
    Databas(#[from] DatabasFel),
}

impl From<sqlx::Error> for ÅterställningsFel {
    fn from(e: sqlx::Error) -> Self {
        ÅterställningsFel::Databas(e.into())
    }
}

/// Backupen som ska återställas, med manifestet, att visa innan driften bekräftar.
#[derive(Debug, Clone)]
pub struct Vald {
    pub backup: Backup,
    pub manifest: Manifest,
}

/// En gjord återställning.
#[derive(Debug, Clone)]
pub struct Återställd {
    pub backup: Backup,
    /// Antalet företag i den återställda installationen; varje fick en post i sin
    /// behandlingshistorik.
    pub företag: usize,
    /// Där den tidigare databasen ligger kvar, med det som registrerades efter backupen.
    pub undanflyttad: PathBuf,
}

/// Backupen `namn` i `mål`, eller den senaste.
pub async fn välj(mål: &Backupmål, namn: Option<&str>) -> Result<Vald, ÅterställningsFel> {
    let backuper = backup::lista(mål).await?;
    let backup = match namn {
        Some(namn) => backuper.into_iter().find(|b| b.namn == namn),
        None => backuper.into_iter().last(),
    }
    .ok_or(BackupFel::IngenBackup)?;
    let manifest = backup::manifest(mål, &backup.namn).await?;
    Ok(Vald { backup, manifest })
}

/// Återställer hela installationen i `databasfil` ur backupen `namn` i `mål`. Bara CLI:t, och
/// servern ska vara stoppad. `arbetskatalog` håller den hämtade backupen under arbetet och ska
/// ligga på samma filsystem som databasen.
pub async fn återställ(
    databasfil: &Path,
    mål: &Backupmål,
    namn: &str,
    rp: &RelyingParty,
    klocka: &Klocka,
    aktör: &Aktör,
    arbetskatalog: &Path,
) -> Result<Återställd, ÅterställningsFel> {
    if !aktor::tillåter(aktör.kanal, Handling::Återställa) {
        return Err(ÅterställningsFel::KanalenFårInte);
    }
    let Vald { backup, manifest } = välj(mål, Some(namn)).await?;
    if manifest.schemaepok != SCHEMAEPOK {
        return Err(ÅterställningsFel::AnnanEpok {
            backupens: manifest.schemaepok,
        });
    }

    let katalog = arbetskatalog.join("aterstallning");
    std::fs::create_dir_all(&katalog).map_err(|e| filfel(&katalog, e))?;
    let hämtad = backup::hämta(mål, &backup.namn, &katalog).await?;
    let resultat = async {
        granska(&hämtad, &manifest.kontrollsummor).await?;
        kontrollera_rp(&hämtad, rp).await?;
        let tid = klocka.nu();
        let undanflyttad = byt_databas(databasfil, &hämtad, tid)?;
        match öppna_och_logga(databasfil, &backup, &manifest, mål, tid, aktör).await {
            Ok(företag) => Ok(Återställd {
                backup: backup.clone(),
                företag,
                undanflyttad,
            }),
            Err(fel) => {
                lägg_tillbaka(databasfil, &undanflyttad)?;
                Err(fel)
            }
        }
    }
    .await;
    let _ = std::fs::remove_file(&hämtad);
    resultat
}

/// Backupens integritet, underlag och kontrollsummor mot manifestet, innan något ändras.
async fn granska(fil: &Path, väntade: &Kontrollsummor) -> Result<(), ÅterställningsFel> {
    let granskning = kontrollsumma::granska_fil(fil).await?;
    let mut problem = granskning.problem;
    problem.extend(väntade.avvikelser(&granskning.kontrollsummor));
    if problem.is_empty() {
        Ok(())
    } else {
        Err(ÅterställningsFel::BackupenÄrInteHel(problem))
    }
}

/// Vägrar en backup vars `rp_id` skiljer sig från konfigurationens. En backup utan sparad
/// relying party (tagen innan servern startade första gången) godtas.
async fn kontrollera_rp(fil: &Path, rp: &RelyingParty) -> Result<(), ÅterställningsFel> {
    let options = SqliteConnectOptions::from_str("sqlite://")?
        .filename(fil)
        .read_only(true);
    let mut conn = SqliteConnection::connect_with(&options).await?;
    let sparat: Option<String> = sqlx::query_scalar("SELECT rp_id FROM installation WHERE id = 1")
        .fetch_optional(&mut conn)
        .await?;
    conn.close().await?;
    match sparat {
        Some(backupens) if backupens != rp.rp_id => Err(ÅterställningsFel::AnnatRpId {
            backupens,
            konfigurationens: rp.rp_id.clone(),
        }),
        _ => Ok(()),
    }
}

/// `fil` med `ändelse` tillagd sist i namnet.
fn med_ändelse(fil: &Path, ändelse: &str) -> PathBuf {
    let mut namn = OsString::from(fil.as_os_str());
    namn.push(ändelse);
    PathBuf::from(namn)
}

/// SQLite:s filer för databasen: själva filen, loggen och det delade minnet.
const ÄNDELSER: [&str; 3] = ["", "-wal", "-shm"];

/// Flyttar den nuvarande databasen undan till `maya.db.fore-aterstallning-<UTC-tid>` (med sin
/// logg, så att inget som registrerats går förlorat) och lägger `hämtad` på dess plats. Ger den
/// undanflyttade filen.
fn byt_databas(
    databasfil: &Path,
    hämtad: &Path,
    tid: Timestamp,
) -> Result<PathBuf, ÅterställningsFel> {
    let undan = med_ändelse(
        databasfil,
        &format!(".fore-aterstallning-{}", tid.strftime("%Y%m%dT%H%M%SZ")),
    );
    for ändelse in ÄNDELSER {
        let från = med_ändelse(databasfil, ändelse);
        if från.exists() {
            let till = med_ändelse(&undan, ändelse);
            std::fs::rename(&från, &till).map_err(|e| filfel(&från, e))?;
        }
    }
    if let Err(e) = flytta(hämtad, databasfil) {
        lägg_tillbaka(databasfil, &undan)?;
        return Err(e);
    }
    Ok(undan)
}

/// Flyttar `från` till `till`, med kopiering om de ligger på olika filsystem.
fn flytta(från: &Path, till: &Path) -> Result<(), ÅterställningsFel> {
    if std::fs::rename(från, till).is_ok() {
        return Ok(());
    }
    let tillfällig = med_ändelse(till, "-aterstalls");
    std::fs::copy(från, &tillfällig).map_err(|e| filfel(från, e))?;
    std::fs::rename(&tillfällig, till).map_err(|e| filfel(till, e))
}

/// Tar bort den återställda databasen och lägger den undanflyttade tillbaka.
fn lägg_tillbaka(databasfil: &Path, undan: &Path) -> Result<(), ÅterställningsFel> {
    for ändelse in ÄNDELSER {
        let fil = med_ändelse(databasfil, ändelse);
        match std::fs::remove_file(&fil) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(filfel(&fil, e)),
            _ => {}
        }
    }
    for ändelse in ÄNDELSER {
        let från = med_ändelse(undan, ändelse);
        if från.exists() {
            let till = med_ändelse(databasfil, ändelse);
            std::fs::rename(&från, &till).map_err(|e| filfel(&från, e))?;
        }
    }
    Ok(())
}

fn filfel(fil: &Path, e: std::io::Error) -> ÅterställningsFel {
    ÅterställningsFel::Fil(format!("{}: {e}", fil.display()))
}

/// Öppnar den återställda databasen (migreringarna körs), verifierar kontrollsummorna mot
/// manifestet och loggar återställningen. Ger antalet företag.
async fn öppna_och_logga(
    databasfil: &Path,
    backup: &Backup,
    manifest: &Manifest,
    mål: &Backupmål,
    tid: Timestamp,
    aktör: &Aktör,
) -> Result<usize, ÅterställningsFel> {
    let db = Databas::öppna(databasfil).await?;
    let resultat = verifiera_och_logga(&db, backup, manifest, mål, tid, aktör).await;
    db.stäng().await;
    resultat
}

async fn verifiera_och_logga(
    db: &Databas,
    backup: &Backup,
    manifest: &Manifest,
    mål: &Backupmål,
    tid: Timestamp,
    aktör: &Aktör,
) -> Result<usize, ÅterställningsFel> {
    let avvikelser = manifest
        .kontrollsummor
        .avvikelser(&Kontrollsummor::räkna(db).await?);
    if !avvikelser.is_empty() {
        return Err(ÅterställningsFel::VerifieringenMisslyckades(avvikelser));
    }

    logga(db, backup, &manifest.maya, &mål.beskrivning(), tid, aktör).await
}

/// Loggar återställningen i varje företags behandlingshistorik och i installationsloggen, och
/// ger varje användare en varning att kvittera under Att göra. Ger antalet företag.
pub(crate) async fn logga(
    db: &Databas,
    backup: &Backup,
    maya: &str,
    mål: &str,
    tid: Timestamp,
    aktör: &Aktör,
) -> Result<usize, ÅterställningsFel> {
    let mut tx = db.pool().begin_with("BEGIN IMMEDIATE").await?;
    let företag: Vec<i64> = sqlx::query_scalar("SELECT id FROM foretag ORDER BY id")
        .fetch_all(&mut *tx)
        .await?;
    let beskrivning = serde_json::json!({
        "backup": backup.namn,
        "backup_tid": backup.tid.to_string(),
        "mål": mål,
        "maya": maya,
        "programversion": env!("CARGO_PKG_VERSION"),
        "företag": företag.len(),
    });
    for id in &företag {
        behandlingshistorik::skriv(
            &mut tx,
            FöretagsId(*id),
            tid,
            aktör,
            behandlingshistorik::Händelse::ÅterställningGjord,
            &backup.namn,
            beskrivning.clone(),
        )
        .await?;
    }
    installationslogg::skriv(
        &mut tx,
        tid,
        aktör,
        installationslogg::Händelse::ÅterställningGjord,
        &backup.namn,
        beskrivning,
    )
    .await?;
    let återställning: i64 = sqlx::query_scalar(
        "INSERT INTO aterstallning (tid, backup, backup_tid) VALUES (?, ?, ?) RETURNING id",
    )
    .bind(tid.to_string())
    .bind(&backup.namn)
    .bind(backup.tid.to_string())
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO aterstallningsvarning (aterstallning_id, anvandare_id)
         SELECT ?, id FROM anvandare",
    )
    .bind(återställning)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(företag.len())
}
