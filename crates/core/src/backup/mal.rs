//! Backupmålet: en monterad katalog eller S3-kompatibel lagring. Filerna heter likadant i båda.

use std::io;
use std::path::{Path, PathBuf};

use super::s3::{S3, S3Fel};
use crate::config::BackupTarget;

/// Var backuperna sparas.
#[derive(Clone)]
pub struct Backupmål(Mål);

#[derive(Clone)]
enum Mål {
    Katalog(PathBuf),
    S3(Box<S3>),
}

impl std::fmt::Debug for Backupmål {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.beskrivning())
    }
}

/// Ett fel i målet, med en text på svenska.
#[derive(Debug)]
pub(crate) enum MålFel {
    FinnsInte,
    Annat(String),
}

impl From<S3Fel> for MålFel {
    fn from(e: S3Fel) -> Self {
        match e {
            S3Fel::FinnsInte(_) => MålFel::FinnsInte,
            annat => MålFel::Annat(annat.to_string()),
        }
    }
}

impl Backupmål {
    /// Målet som konfigurationen anger.
    pub fn från(mål: &BackupTarget) -> Self {
        match mål {
            BackupTarget::Dir(dir) => Self::katalog(dir),
            BackupTarget::S3(s3) => Backupmål(Mål::S3(Box::new(S3::ny(s3)))),
        }
    }

    /// Samma mål med `klocka` för tiden i S3-begärans signatur. En katalog har ingen.
    pub fn med_klocka(self, klocka: crate::Klocka) -> Self {
        match self.0 {
            Mål::S3(s3) => Backupmål(Mål::S3(Box::new(s3.med_klocka(klocka)))),
            katalog => Backupmål(katalog),
        }
    }

    /// En monterad katalog.
    pub fn katalog(dir: &Path) -> Self {
        Backupmål(Mål::Katalog(dir.to_path_buf()))
    }

    /// Målet i text, för meddelanden.
    pub fn beskrivning(&self) -> String {
        match &self.0 {
            Mål::Katalog(dir) => format!("katalogen {}", dir.display()),
            Mål::S3(s3) => format!("S3-bucketen {}", s3.beskrivning()),
        }
    }

    /// Katalogen, om målet är en katalog.
    pub(crate) fn lokal_katalog(&self) -> Option<&Path> {
        match &self.0 {
            Mål::Katalog(dir) => Some(dir),
            Mål::S3(_) => None,
        }
    }

    /// Lägger filen `fil` i målet som `namn`. I en katalog skrivs den först under ett tillfälligt
    /// namn och byter sedan namn, så att en halv fil aldrig syns.
    pub(crate) async fn ladda_upp(
        &self,
        namn: &str,
        fil: &Path,
        sha256: &str,
    ) -> Result<(), MålFel> {
        let (mål, namn, fil, sha256) = (
            self.0.clone(),
            namn.to_string(),
            fil.to_path_buf(),
            sha256.to_string(),
        );
        blockerande(move || match mål {
            Mål::Katalog(dir) => {
                let tillfällig = dir.join(format!(".{namn}.tmp"));
                std::fs::copy(&fil, &tillfällig).map_err(|e| io_fel(&dir, e))?;
                std::fs::File::open(&tillfällig)
                    .and_then(|f| f.sync_all())
                    .map_err(|e| io_fel(&dir, e))?;
                std::fs::rename(&tillfällig, dir.join(&namn)).map_err(|e| io_fel(&dir, e))
            }
            Mål::S3(s3) => Ok(s3.ladda_upp(&namn, &fil, &sha256)?),
        })
        .await
    }

    /// Lägger `innehåll` i målet som `namn`.
    pub(crate) async fn lägg(&self, namn: &str, innehåll: Vec<u8>) -> Result<(), MålFel> {
        let (mål, namn) = (self.0.clone(), namn.to_string());
        blockerande(move || match mål {
            Mål::Katalog(dir) => {
                let tillfällig = dir.join(format!(".{namn}.tmp"));
                std::fs::write(&tillfällig, &innehåll).map_err(|e| io_fel(&dir, e))?;
                std::fs::rename(&tillfällig, dir.join(&namn)).map_err(|e| io_fel(&dir, e))
            }
            Mål::S3(s3) => Ok(s3.lägg(&namn, &innehåll)?),
        })
        .await
    }

    /// Hämtar `namn` till filen `till`.
    pub(crate) async fn hämta(&self, namn: &str, till: &Path) -> Result<(), MålFel> {
        let (mål, namn, till) = (self.0.clone(), namn.to_string(), till.to_path_buf());
        blockerande(move || match mål {
            Mål::Katalog(dir) => std::fs::copy(dir.join(&namn), &till)
                .map(|_| ())
                .map_err(|e| fil_fel(&dir, &namn, e)),
            Mål::S3(s3) => Ok(s3.hämta(&namn, &till)?),
        })
        .await
    }

    /// Läser `namn` till minnet.
    pub(crate) async fn läs(&self, namn: &str) -> Result<Vec<u8>, MålFel> {
        let (mål, namn) = (self.0.clone(), namn.to_string());
        blockerande(move || match mål {
            Mål::Katalog(dir) => {
                std::fs::read(dir.join(&namn)).map_err(|e| fil_fel(&dir, &namn, e))
            }
            Mål::S3(s3) => Ok(s3.läs(&namn)?),
        })
        .await
    }

    /// Namnen på alla filer i målet som börjar med `prefix`, sorterade.
    pub(crate) async fn lista(&self, prefix: &str) -> Result<Vec<String>, MålFel> {
        let (mål, prefix) = (self.0.clone(), prefix.to_string());
        blockerande(move || {
            let mut namn = match mål {
                Mål::Katalog(dir) => std::fs::read_dir(&dir)
                    .map_err(|e| io_fel(&dir, e))?
                    .filter_map(|post| post.ok()?.file_name().into_string().ok())
                    .filter(|namn| namn.starts_with(&prefix))
                    .collect(),
                Mål::S3(s3) => s3.lista(&prefix)?,
            };
            namn.sort();
            Ok(namn)
        })
        .await
    }

    pub(crate) async fn radera(&self, namn: &str) -> Result<(), MålFel> {
        let (mål, namn) = (self.0.clone(), namn.to_string());
        blockerande(move || match mål {
            Mål::Katalog(dir) => match std::fs::remove_file(dir.join(&namn)) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => Err(fil_fel(&dir, &namn, e)),
                _ => Ok(()),
            },
            Mål::S3(s3) => Ok(s3.radera(&namn)?),
        })
        .await
    }
}

async fn blockerande<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, MålFel> + Send + 'static,
) -> Result<T, MålFel> {
    tokio::task::spawn_blocking(f)
        .await
        .unwrap_or_else(|e| Err(MålFel::Annat(format!("backupjobbet avbröts: {e}"))))
}

fn io_fel(dir: &Path, e: io::Error) -> MålFel {
    MålFel::Annat(format!("katalogen {}: {e}", dir.display()))
}

fn fil_fel(dir: &Path, namn: &str, e: io::Error) -> MålFel {
    if e.kind() == io::ErrorKind::NotFound {
        MålFel::FinnsInte
    } else {
        MålFel::Annat(format!("{}: {e}", dir.join(namn).display()))
    }
}
