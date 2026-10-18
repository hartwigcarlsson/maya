//! Backup av hela installationen (beslut i #13, A5): en ögonblicksbild med `VACUUM INTO` och ett
//! manifest med kontrollsummorna, till en monterad katalog eller S3-kompatibel lagring. Varje
//! backup verifieras direkt ur målet, och gallringen behåller 7 dagliga och 12 månatliga.
//! Körningarna sparas så att Att göra och Installation kan visa dem.
//!
//! En backup är två filer i målet: `maya-<UTC-tid>.db` och `maya-<UTC-tid>.json` (manifestet).
//! Manifestet skrivs sist, så en backup finns bara om dess manifest finns.

mod mal;
mod s3;

use std::path::{Path, PathBuf};

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::aktor::{self, Aktör, Handling, Kanal};
use crate::config::BackupTarget;
use crate::kontrollsumma::{self, Kontrollsummor};
use crate::{Databas, DatabasFel, Klocka, SCHEMAEPOK, installationslogg};
pub use mal::Backupmål;
use mal::MålFel;

const PREFIX: &str = "maya-";
/// Manifestets format. Höj när fälten ändras så att en äldre Maya inte läser fel.
const FORMAT: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum BackupFel {
    #[error("kanalen får inte göra det här")]
    KanalenFårInte,
    #[error("det finns ingen sådan backup i målet")]
    IngenBackup,
    #[error("backupmålet: {0}")]
    Mål(String),
    #[error("backupen kunde inte tas: {0}")]
    Ögonblicksbild(String),
    #[error(transparent)]
    Databas(#[from] DatabasFel),
}

impl From<sqlx::Error> for BackupFel {
    fn from(e: sqlx::Error) -> Self {
        BackupFel::Databas(e.into())
    }
}

impl From<MålFel> for BackupFel {
    fn from(e: MålFel) -> Self {
        match e {
            MålFel::FinnsInte => BackupFel::IngenBackup,
            MålFel::Annat(text) => BackupFel::Mål(text),
        }
    }
}

/// En backup i målet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backup {
    /// T.ex. `maya-20261018T010000Z`.
    pub namn: String,
    pub tid: Timestamp,
}

impl Backup {
    fn ny(tid: Timestamp) -> Self {
        Backup {
            namn: format!("{PREFIX}{}", tid.strftime("%Y%m%dT%H%M%SZ")),
            tid,
        }
    }

    /// Backupen med namnet `namn`, om det är ett backupnamn.
    fn tolka(namn: &str) -> Option<Self> {
        let s = namn.strip_prefix(PREFIX)?;
        let b = s.as_bytes();
        if b.len() != 16 || b[8] != b'T' || b[15] != b'Z' {
            return None;
        }
        let iso = format!(
            "{}-{}-{}T{}:{}:{}Z",
            &s[0..4],
            &s[4..6],
            &s[6..8],
            &s[9..11],
            &s[11..13],
            &s[13..15]
        );
        let tid: Timestamp = iso.parse().ok()?;
        (Backup::ny(tid).namn == namn).then(|| Backup::ny(tid))
    }

    fn databasfil(&self) -> String {
        format!("{}.db", self.namn)
    }

    fn manifestfil(&self) -> String {
        format!("{}.json", self.namn)
    }
}

/// Manifestet som ligger bredvid varje backup.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub format: u32,
    /// Maya-versionen som tog backupen.
    pub maya: String,
    pub schemaepok: String,
    pub tid: Timestamp,
    pub databas: Databasfil,
    pub kontrollsummor: Kontrollsummor,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Databasfil {
    pub fil: String,
    pub byte: u64,
    /// SHA-256 (hex) av hela filen.
    pub sha256: String,
}

/// Resultatet av en verifiering.
#[derive(Debug, Clone)]
pub struct Verifiering {
    pub backup: Backup,
    /// Det som är fel, på svenska. Tomt när backupen är hel.
    pub problem: Vec<String>,
}

impl Verifiering {
    pub fn lyckades(&self) -> bool {
        self.problem.is_empty()
    }
}

/// En tagen och verifierad backup.
#[derive(Debug, Clone)]
pub struct Backupresultat {
    pub backup: Backup,
    pub verifiering: Verifiering,
    /// Backuper som gallrades.
    pub gallrade: Vec<Backup>,
}

/// En sparad körning av backupen eller verifieringen.
#[derive(Debug, Clone, PartialEq)]
pub struct Körning {
    pub tid: Timestamp,
    pub kanal: Kanal,
    pub backup: Option<String>,
    pub lyckades: bool,
    /// Felen, en per rad; tom när körningen lyckades.
    pub meddelande: String,
}

/// Det Att göra och Installation visar.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Backupstatus {
    pub senaste_backup: Option<Körning>,
    pub senaste_lyckade_backup: Option<Körning>,
    pub senaste_verifiering: Option<Körning>,
}

/// Tar en backup av hela installationen till `mål`, verifierar den ur målet och gallrar äldre.
/// `arbetskatalog` håller ögonblicksbilden medan den laddas upp och töms efteråt. Bara Maya
/// självt (det nattliga jobbet). Körningen sparas också när den misslyckas.
pub async fn ta_backup(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    mål: &Backupmål,
    arbetskatalog: &Path,
) -> Result<Backupresultat, BackupFel> {
    if !aktor::tillåter(aktör.kanal, Handling::TaBackup) {
        return Err(BackupFel::KanalenFårInte);
    }
    let backup = Backup::ny(klocka.nu());
    let resultat = ta(db, &backup, mål, arbetskatalog).await;
    let meddelande = match &resultat {
        Ok(()) => String::new(),
        Err(e) => e.to_string(),
    };
    spara_körning(
        db,
        "backup",
        backup.tid,
        aktör,
        resultat.is_ok().then_some(backup.namn.as_str()),
        &meddelande,
    )
    .await?;
    resultat?;

    let verifiering =
        verifiera_utan_kontroll(db, klocka, aktör, mål, &backup, arbetskatalog).await?;
    let gallrade = gallra(mål).await?;
    Ok(Backupresultat {
        backup,
        verifiering,
        gallrade,
    })
}

async fn ta(
    db: &Databas,
    backup: &Backup,
    mål: &Backupmål,
    arbetskatalog: &Path,
) -> Result<(), BackupFel> {
    std::fs::create_dir_all(arbetskatalog)
        .map_err(|e| BackupFel::Ögonblicksbild(format!("{}: {e}", arbetskatalog.display())))?;
    let fil = arbetskatalog.join(backup.databasfil());
    let resultat = ta_till(db, backup, mål, &fil).await;
    let _ = std::fs::remove_file(&fil);
    resultat
}

async fn ta_till(
    db: &Databas,
    backup: &Backup,
    mål: &Backupmål,
    fil: &Path,
) -> Result<(), BackupFel> {
    let kontrollsummor = ögonblicksbild(db, fil).await?;
    lägg_i_mål(mål, backup, fil, kontrollsummor).await
}

/// Tar en ögonblicksbild av databasen till `fil` med `VACUUM INTO` och granskar den. Ger
/// kontrollsummorna ur ögonblicksbilden, så att de beskriver exakt det som sparas.
async fn ögonblicksbild(db: &Databas, fil: &Path) -> Result<Kontrollsummor, BackupFel> {
    let _ = std::fs::remove_file(fil);
    let sökväg = fil.to_str().ok_or_else(|| {
        BackupFel::Ögonblicksbild(format!("{} är ingen UTF-8-sökväg", fil.display()))
    })?;
    sqlx::query("VACUUM INTO ?")
        .bind(sökväg)
        .execute(db.pool())
        .await
        .map_err(|e| BackupFel::Ögonblicksbild(e.to_string()))?;
    let granskning = kontrollsumma::granska_fil(fil).await?;
    if !granskning.problem.is_empty() {
        return Err(BackupFel::Ögonblicksbild(granskning.problem.join("; ")));
    }
    Ok(granskning.kontrollsummor)
}

/// Laddar upp ögonblicksbilden `fil` till `mål` som `backup`, med manifestet sist.
async fn lägg_i_mål(
    mål: &Backupmål,
    backup: &Backup,
    fil: &Path,
    kontrollsummor: Kontrollsummor,
) -> Result<(), BackupFel> {
    let (byte, sha256) = filhash(fil).await?;
    let manifest = Manifest {
        format: FORMAT,
        maya: env!("CARGO_PKG_VERSION").to_string(),
        schemaepok: SCHEMAEPOK.to_string(),
        tid: backup.tid,
        databas: Databasfil {
            fil: backup.databasfil(),
            byte,
            sha256: sha256.clone(),
        },
        kontrollsummor,
    };
    mål.ladda_upp(&backup.databasfil(), fil, &sha256).await?;
    let json = serde_json::to_vec_pretty(&manifest).expect("manifestet blir JSON");
    mål.lägg(&backup.manifestfil(), json).await?;
    Ok(())
}

/// Ögonblicksbilden före en schemamigrering: kvar i arbetskatalogen tills migreringen är
/// kontrollerad, så att den kan läggas tillbaka.
#[derive(Debug)]
pub(crate) struct FöreMigrering {
    pub fil: PathBuf,
    /// Backupen i målet, när ett mål fanns.
    pub backup: Option<Backup>,
    pub kontrollsummor: Kontrollsummor,
}

/// Tar en ögonblicksbild före en schemamigrering till `arbetskatalog` och, med ett mål, en
/// backup dit (A3). Körningen sparas inte i `backupkorning`: den tabellen kan saknas i det
/// gamla schemat.
pub(crate) async fn före_migrering(
    db: &Databas,
    tid: Timestamp,
    mål: Option<&Backupmål>,
    arbetskatalog: &Path,
) -> Result<FöreMigrering, BackupFel> {
    std::fs::create_dir_all(arbetskatalog)
        .map_err(|e| BackupFel::Ögonblicksbild(format!("{}: {e}", arbetskatalog.display())))?;
    let backup = Backup::ny(tid);
    let fil = arbetskatalog.join(format!("fore-migrering-{}", backup.databasfil()));
    let kontrollsummor = ögonblicksbild(db, &fil).await?;
    let backup = match mål {
        Some(mål) => {
            if let Err(e) = lägg_i_mål(mål, &backup, &fil, kontrollsummor.clone()).await {
                let _ = std::fs::remove_file(&fil);
                return Err(e);
            }
            Some(backup)
        }
        None => None,
    };
    Ok(FöreMigrering {
        fil,
        backup,
        kontrollsummor,
    })
}

/// Verifierar backupen `namn` (eller den senaste) i `mål`: databasfilens SHA-256 mot
/// manifestet, `PRAGMA integrity_check`, underlagens innehåll mot deras SHA-256 och
/// kontrollsummorna mot manifestet. CLI:t (`maya backup verify`) och Maya självt. Körningen
/// sparas; CLI:ts verifiering skrivs också i den installationsövergripande loggen.
pub async fn verifiera(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    mål: &Backupmål,
    namn: Option<&str>,
    arbetskatalog: &Path,
) -> Result<Verifiering, BackupFel> {
    if !aktor::tillåter(aktör.kanal, Handling::VerifieraBackup) {
        return Err(BackupFel::KanalenFårInte);
    }
    let backuper = lista(mål).await?;
    let backup = match namn {
        Some(namn) => backuper.into_iter().find(|b| b.namn == namn),
        None => backuper.into_iter().last(),
    }
    .ok_or(BackupFel::IngenBackup)?;
    verifiera_utan_kontroll(db, klocka, aktör, mål, &backup, arbetskatalog).await
}

async fn verifiera_utan_kontroll(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    mål: &Backupmål,
    backup: &Backup,
    arbetskatalog: &Path,
) -> Result<Verifiering, BackupFel> {
    let tid = klocka.nu();
    let problem = match granska(mål, backup, arbetskatalog).await {
        Ok(problem) => problem,
        Err(e) => vec![e.to_string()],
    };
    let verifiering = Verifiering {
        backup: backup.clone(),
        problem,
    };
    let meddelande = verifiering.problem.join("\n");
    spara_körning(
        db,
        "verifiering",
        tid,
        aktör,
        Some(&backup.namn),
        &meddelande,
    )
    .await?;
    if aktör.kanal == Kanal::Cli {
        let mut conn = db.pool().acquire().await?;
        installationslogg::skriv(
            &mut conn,
            tid,
            aktör,
            installationslogg::Händelse::BackupVerifierad,
            &backup.namn,
            serde_json::json!({
                "backup": backup.namn,
                "mål": mål.beskrivning(),
                "lyckades": verifiering.lyckades(),
                "problem": verifiering.problem,
            }),
        )
        .await?;
    }
    Ok(verifiering)
}

/// Problemen med backupen i målet.
async fn granska(
    mål: &Backupmål,
    backup: &Backup,
    arbetskatalog: &Path,
) -> Result<Vec<String>, BackupFel> {
    let manifest = manifest(mål, &backup.namn).await?;
    // En katalog granskas på plats; från S3 hämtas filen först.
    let (fil, tillfällig) = match mål.lokal_katalog() {
        Some(dir) => (dir.join(backup.databasfil()), false),
        None => {
            std::fs::create_dir_all(arbetskatalog)
                .map_err(|e| BackupFel::Mål(format!("{}: {e}", arbetskatalog.display())))?;
            let fil = arbetskatalog.join(format!("verifiera-{}", backup.databasfil()));
            mål.hämta(&backup.databasfil(), &fil).await?;
            (fil, true)
        }
    };
    let resultat = granska_fil(&fil, &manifest).await;
    if tillfällig {
        let _ = std::fs::remove_file(&fil);
    }
    resultat
}

async fn granska_fil(fil: &Path, manifest: &Manifest) -> Result<Vec<String>, BackupFel> {
    let mut problem = Vec::new();
    if !fil.is_file() {
        return Err(BackupFel::IngenBackup);
    }
    let (byte, sha256) = filhash(fil).await?;
    if byte != manifest.databas.byte || sha256 != manifest.databas.sha256 {
        problem.push("databasfilens SHA-256 stämmer inte med manifestet".to_string());
    }
    match kontrollsumma::granska_fil(fil).await {
        Ok(granskning) => {
            problem.extend(granskning.problem);
            problem.extend(
                manifest
                    .kontrollsummor
                    .avvikelser(&granskning.kontrollsummor),
            );
        }
        Err(e) => problem.push(format!("databasfilen går inte att läsa: {e}")),
    }
    Ok(problem)
}

/// Backuperna i målet, äldst först.
pub async fn lista(mål: &Backupmål) -> Result<Vec<Backup>, BackupFel> {
    Ok(mål
        .lista(PREFIX)
        .await?
        .iter()
        .filter_map(|namn| Backup::tolka(namn.strip_suffix(".json")?))
        .collect())
}

/// Manifestet för backupen `namn`.
pub async fn manifest(mål: &Backupmål, namn: &str) -> Result<Manifest, BackupFel> {
    let backup = Backup::tolka(namn).ok_or(BackupFel::IngenBackup)?;
    let json = mål.läs(&backup.manifestfil()).await?;
    serde_json::from_slice(&json).map_err(|e| {
        BackupFel::Mål(format!(
            "manifestet {} går inte att läsa: {e}",
            backup.manifestfil()
        ))
    })
}

/// Hämtar backupen `namn` till `katalog/maya.db`, där Maya kan öppna den, och kontrollerar att
/// filen är den som manifestet beskriver. Grunden för återställningen.
pub async fn hämta(mål: &Backupmål, namn: &str, katalog: &Path) -> Result<PathBuf, BackupFel> {
    let manifest = manifest(mål, namn).await?;
    let fil = katalog.join("maya.db");
    mål.hämta(&manifest.databas.fil, &fil).await?;
    let (byte, sha256) = filhash(&fil).await?;
    if byte != manifest.databas.byte || sha256 != manifest.databas.sha256 {
        let _ = std::fs::remove_file(&fil);
        return Err(BackupFel::Mål(
            "databasfilens SHA-256 stämmer inte med manifestet".to_string(),
        ));
    }
    Ok(fil)
}

/// Gallrar enligt [`maya_domain::backup::att_gallra`] och tar bort databasfiler utan manifest
/// (från backuper som avbröts).
async fn gallra(mål: &Backupmål) -> Result<Vec<Backup>, BackupFel> {
    let filer = mål.lista(PREFIX).await?;
    let backuper = lista(mål).await?;
    let tider: Vec<Timestamp> = backuper.iter().map(|b| b.tid).collect();
    let gallras: Vec<Backup> = maya_domain::backup::att_gallra(&tider)
        .into_iter()
        .map(Backup::ny)
        .collect();
    for backup in &gallras {
        // Manifestet först: utan det räknas backupen inte längre.
        mål.radera(&backup.manifestfil()).await?;
        mål.radera(&backup.databasfil()).await?;
    }
    for fil in filer {
        if let Some(namn) = fil.strip_suffix(".db")
            && !backuper.iter().any(|b| b.namn == namn)
        {
            mål.radera(&fil).await?;
        }
    }
    Ok(gallras)
}

/// Senaste backup, senaste lyckade backup och senaste verifiering.
pub async fn status(db: &Databas) -> Result<Backupstatus, DatabasFel> {
    let senaste = async |villkor: &'static str| -> Result<Option<Körning>, DatabasFel> {
        let rad: Option<(String, String, Option<String>, i64, String)> =
            sqlx::query_as(sqlx::AssertSqlSafe(format!(
                "SELECT tid, kanal, backup, lyckades, meddelande FROM backupkorning
                 WHERE {villkor} ORDER BY id DESC LIMIT 1"
            )))
            .fetch_optional(db.pool())
            .await?;
        let fel = |vad: &str| DatabasFel::Korrupt(format!("backupkorning: {vad}"));
        rad.map(|(tid, kanal, backup, lyckades, meddelande)| {
            Ok(Körning {
                tid: tid.parse().map_err(|_| fel("tid"))?,
                kanal: Kanal::från_namn(&kanal).ok_or_else(|| fel("kanal"))?,
                backup,
                lyckades: lyckades == 1,
                meddelande,
            })
        })
        .transpose()
    };
    Ok(Backupstatus {
        senaste_backup: senaste("typ = 'backup'").await?,
        senaste_lyckade_backup: senaste("typ = 'backup' AND lyckades = 1").await?,
        senaste_verifiering: senaste("typ = 'verifiering'").await?,
    })
}

async fn spara_körning(
    db: &Databas,
    typ: &str,
    tid: Timestamp,
    aktör: &Aktör,
    backup: Option<&str>,
    meddelande: &str,
) -> Result<(), DatabasFel> {
    sqlx::query(
        "INSERT INTO backupkorning (typ, tid, kanal, backup, lyckades, meddelande)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(typ)
    .bind(tid.to_string())
    .bind(aktör.kanal.namn())
    .bind(backup)
    .bind(meddelande.is_empty())
    .bind(meddelande)
    .execute(db.pool())
    .await?;
    Ok(())
}

/// Storlek och SHA-256 (hex) för filen, läst i block.
async fn filhash(fil: &Path) -> Result<(u64, String), BackupFel> {
    let fil = fil.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let mut läsare = std::fs::File::open(&fil)?;
        let mut hash = Sha256::new();
        let byte = std::io::copy(&mut läsare, &mut hash)?;
        Ok::<_, std::io::Error>((byte, hex::encode(hash.finalize())))
    })
    .await
    .map_err(|e| BackupFel::Mål(e.to_string()))?
    .map_err(|e| BackupFel::Mål(e.to_string()))
}

/// Varningar om målet i konfigurationen, för loggen vid start och för Att göra: målet ligger på
/// samma enhet som databasen (kopian är inte åtskild, A5), eller katalogen finns inte.
pub fn varningar(data_dir: &Path, mål: &BackupTarget) -> Vec<String> {
    let BackupTarget::Dir(dir) = mål else {
        return Vec::new();
    };
    if !dir.is_dir() {
        return vec![format!(
            "Backupkatalogen {} finns inte. Montera den, annars misslyckas backupen.",
            dir.display()
        )];
    }
    if samma_enhet(data_dir, dir) {
        return vec![format!(
            "Backupkatalogen {} ligger på samma enhet som databasen i {}. Kopian är inte åtskild \
             från originalet: montera en annan disk eller använd S3-kompatibel lagring.",
            dir.display(),
            data_dir.display()
        )];
    }
    Vec::new()
}

#[cfg(unix)]
fn samma_enhet(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev(),
        _ => false,
    }
}

#[cfg(not(unix))]
fn samma_enhet(_: &Path, _: &Path) -> bool {
    false
}
