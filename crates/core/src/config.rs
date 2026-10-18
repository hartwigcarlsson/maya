//! Installationens konfiguration, läst ur miljön (och `.env`, som binären läser in först).
//!
//! Hemligheter kan anges med en `_FILE`-variant som pekar på en fil med värdet. Utskriften av
//! konfigurationen (`Display` och `Debug`) visar aldrig en hemlighet.

use std::collections::HashMap;
use std::fmt;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;

use ipnet::IpNet;
use jiff::civil::Time;
use url::Url;

#[derive(Debug, Clone)]
pub struct Config {
    /// Mayas publika adress, ett origin. Härleds aldrig ur `Host` eller `X-Forwarded-*`.
    pub public_url: Url,
    /// `MAYA_LISTEN`, som standard `0.0.0.0:8080`.
    pub listen: SocketAddr,
    /// `MAYA_DATA_DIR`, som standard `/data`. Databasen ligger här.
    pub data_dir: PathBuf,
    /// `MAYA_TRUSTED_PROXY`: adresser eller nätblock vars `X-Forwarded-For` godtas.
    pub trusted_proxies: Vec<IpNet>,
    pub backup: BackupTarget,
    /// `MAYA_BACKUP_TIME`, klockslag i `Europe/Stockholm`, som standard 03:00.
    pub backup_time: Time,
    pub smtp: Option<Smtp>,
    /// `MAYA_DRIFT_EPOST`: dit larm till driften skickas.
    pub drift_epost: Option<String>,
    pub arkivplan: Arkivplan,
}

#[derive(Debug, Clone)]
pub enum BackupTarget {
    /// `MAYA_BACKUP_DIR`: en monterad katalog, i containern `/backup`.
    Dir(PathBuf),
    /// `MAYA_BACKUP_S3_…`: S3-kompatibel lagring.
    S3(S3Target),
}

#[derive(Debug, Clone)]
pub struct S3Target {
    pub endpoint: Url,
    pub bucket: String,
    pub region: String,
    pub access_key_id: String,
    pub secret_access_key: Secret,
}

#[derive(Debug, Clone)]
pub struct Smtp {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<Secret>,
    pub from: Option<String>,
}

/// Arkivplanens fritext: det Maya inte kan veta själv.
#[derive(Debug, Clone, Default)]
pub struct Arkivplan {
    /// `MAYA_ARKIVPLAN_PLATS`: fysisk plats och land.
    pub plats: Option<String>,
    /// `MAYA_ARKIVPLAN_OFFSITE`: hur kopian flyttas offsite.
    pub offsite: Option<String>,
    /// `MAYA_ARKIVPLAN_NYCKEL`: var en eventuell krypteringsnyckel förvaras.
    pub nyckel: Option<String>,
    /// `MAYA_ARKIVPLAN_ANSVARIG`: vem som ansvarar för driften.
    pub ansvarig: Option<String>,
}

/// Ett hemligt värde. Skrivs aldrig ut; läs det med [`Secret::expose`].
#[derive(Clone)]
pub struct Secret(String);

impl Secret {
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<dold>)")
    }
}

/// Allt som är fel i konfigurationen, så att driften ser alla problem på en gång.
#[derive(Debug)]
pub struct ConfigError {
    pub problems: Vec<String>,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Maya kan inte starta, konfigurationen är ofullständig:")?;
        for problem in &self.problems {
            writeln!(f, "  - {problem}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    /// Tolkar konfigurationen ur miljövariablerna `vars`. Tomma värden räknas som saknade.
    pub fn from_env(vars: impl IntoIterator<Item = (String, String)>) -> Result<Self, ConfigError> {
        let mut env = Env {
            vars: vars
                .into_iter()
                .filter(|(_, v)| !v.trim().is_empty())
                .collect(),
            problems: Vec::new(),
        };

        let public_url = env.public_url();
        let listen = env
            .parsed("MAYA_LISTEN", "en adress och port, t.ex. 0.0.0.0:8080")
            .unwrap_or_else(|| SocketAddr::from(([0, 0, 0, 0], 8080)));
        let data_dir = env
            .get("MAYA_DATA_DIR")
            .map_or_else(|| "/data".into(), PathBuf::from);
        let trusted_proxies = env.trusted_proxies();
        let backup = env.backup();
        let backup_time = env
            .parsed("MAYA_BACKUP_TIME", "ett klockslag, t.ex. 03:00")
            .unwrap_or(Time::constant(3, 0, 0, 0));
        let smtp = env.smtp();
        let drift_epost = env.get("MAYA_DRIFT_EPOST");
        let arkivplan = Arkivplan {
            plats: env.get("MAYA_ARKIVPLAN_PLATS"),
            offsite: env.get("MAYA_ARKIVPLAN_OFFSITE"),
            nyckel: env.get("MAYA_ARKIVPLAN_NYCKEL"),
            ansvarig: env.get("MAYA_ARKIVPLAN_ANSVARIG"),
        };

        match (public_url, backup) {
            (Some(public_url), Some(backup)) if env.problems.is_empty() => Ok(Config {
                public_url,
                listen,
                data_dir,
                trusted_proxies,
                backup,
                backup_time,
                smtp,
                drift_epost,
                arkivplan,
            }),
            _ => Err(ConfigError {
                problems: env.problems,
            }),
        }
    }

    /// Värden och, om den inte är standard, porten ur den publika URL:en: det enda värde som
    /// `Host` får ha.
    pub fn public_host(&self) -> String {
        let host = self.public_url.host_str().unwrap_or_default();
        match self.public_url.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_string(),
        }
    }

    pub fn is_trusted_proxy(&self, ip: IpAddr) -> bool {
        self.trusted_proxies.iter().any(|net| net.contains(&ip))
    }

    /// Sökvägen till databasen.
    pub fn database_path(&self) -> PathBuf {
        self.data_dir.join("maya.db")
    }
}

struct Env {
    vars: HashMap<String, String>,
    problems: Vec<String>,
}

impl Env {
    fn get(&self, name: &str) -> Option<String> {
        self.vars.get(name).map(|v| v.trim().to_string())
    }

    fn require(&mut self, name: &str, why: &str) -> Option<String> {
        let value = self.get(name);
        if value.is_none() {
            self.problems.push(format!("{name} saknas: {why}"));
        }
        value
    }

    fn parsed<T: std::str::FromStr>(&mut self, name: &str, expected: &str) -> Option<T> {
        let raw = self.get(name)?;
        match raw.parse() {
            Ok(value) => Some(value),
            Err(_) => {
                self.problems
                    .push(format!("{name} är \"{raw}\" men ska vara {expected}"));
                None
            }
        }
    }

    /// En hemlighet: ur `name` eller ur filen som `name_FILE` pekar på, men inte båda.
    fn secret(&mut self, name: &str) -> Option<Secret> {
        let file_var = format!("{name}_FILE");
        match (self.get(name), self.get(&file_var)) {
            (Some(_), Some(_)) => {
                self.problems
                    .push(format!("ange antingen {name} eller {file_var}, inte båda"));
                None
            }
            (Some(value), None) => Some(Secret(value)),
            (None, Some(path)) => match std::fs::read_to_string(&path) {
                Ok(value) => Some(Secret(value.trim_end_matches(['\r', '\n']).to_string())),
                Err(e) => {
                    self.problems
                        .push(format!("{file_var}: kan inte läsa filen {path}: {e}"));
                    None
                }
            },
            (None, None) => None,
        }
    }

    fn public_url(&mut self) -> Option<Url> {
        let raw = self.require(
            "MAYA_PUBLIC_URL",
            "ange Mayas publika adress, t.ex. https://bokforing.example.se",
        )?;
        let problem = |why: &str| {
            format!(
                "MAYA_PUBLIC_URL är \"{raw}\" men ska vara {why}, t.ex. https://bokforing.example.se"
            )
        };
        let url = match Url::parse(&raw) {
            Ok(url) => url,
            Err(_) => {
                self.problems.push(problem("en adress"));
                return None;
            }
        };
        let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
        if url.scheme() != "https" && !(url.scheme() == "http" && local) {
            self.problems.push(problem("en https-adress"));
            return None;
        }
        if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
            self.problems
                .push(problem("bara schema och värd, utan sökväg"));
            return None;
        }
        Some(url)
    }

    fn trusted_proxies(&mut self) -> Vec<IpNet> {
        let Some(raw) = self.get("MAYA_TRUSTED_PROXY") else {
            return Vec::new();
        };
        let mut nets = Vec::new();
        for part in raw.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            match part
                .parse::<IpNet>()
                .or_else(|_| part.parse::<IpAddr>().map(IpNet::from))
            {
                Ok(net) => nets.push(net),
                Err(_) => self.problems.push(format!(
                    "MAYA_TRUSTED_PROXY: \"{part}\" är varken en IP-adress eller ett nätblock"
                )),
            }
        }
        nets
    }

    fn backup(&mut self) -> Option<BackupTarget> {
        let dir = self.get("MAYA_BACKUP_DIR");
        let s3 = self.vars.keys().any(|k| k.starts_with("MAYA_BACKUP_S3_"));
        match (dir, s3) {
            (Some(_), true) => {
                self.problems.push(
                    "ange ett backupmål, antingen MAYA_BACKUP_DIR eller MAYA_BACKUP_S3_…, inte båda"
                        .to_string(),
                );
                None
            }
            (Some(dir), false) => Some(BackupTarget::Dir(dir.into())),
            (None, true) => self.s3().map(BackupTarget::S3),
            (None, false) => {
                self.problems.push(
                    "Backupmål saknas: ange MAYA_BACKUP_DIR (en monterad katalog, t.ex. /backup) \
                     eller MAYA_BACKUP_S3_ENDPOINT med bucket och nycklar. Maya kör aldrig utan \
                     säkerhetskopia."
                        .to_string(),
                );
                None
            }
        }
    }

    fn s3(&mut self) -> Option<S3Target> {
        let endpoint = self.require("MAYA_BACKUP_S3_ENDPOINT", "S3-tjänstens adress");
        let endpoint = endpoint.and_then(|raw| match Url::parse(&raw) {
            Ok(url) => Some(url),
            Err(_) => {
                self.problems.push(format!(
                    "MAYA_BACKUP_S3_ENDPOINT är \"{raw}\" men ska vara en adress"
                ));
                None
            }
        });
        let bucket = self.require("MAYA_BACKUP_S3_BUCKET", "bucketen som backuperna sparas i");
        let region = self
            .get("MAYA_BACKUP_S3_REGION")
            .unwrap_or_else(|| "us-east-1".to_string());
        let access_key_id = self.require("MAYA_BACKUP_S3_ACCESS_KEY_ID", "nyckel-id för S3");
        let given = self.get("MAYA_BACKUP_S3_SECRET_ACCESS_KEY").is_some()
            || self.get("MAYA_BACKUP_S3_SECRET_ACCESS_KEY_FILE").is_some();
        let secret_access_key = self.secret("MAYA_BACKUP_S3_SECRET_ACCESS_KEY");
        if !given {
            self.problems.push(
                "MAYA_BACKUP_S3_SECRET_ACCESS_KEY saknas: ange nyckeln eller en fil med \
                 MAYA_BACKUP_S3_SECRET_ACCESS_KEY_FILE"
                    .to_string(),
            );
        }
        Some(S3Target {
            endpoint: endpoint?,
            bucket: bucket?,
            region,
            access_key_id: access_key_id?,
            secret_access_key: secret_access_key?,
        })
    }

    fn smtp(&mut self) -> Option<Smtp> {
        let password = self.secret("MAYA_SMTP_PASSWORD");
        let host = self.get("MAYA_SMTP_HOST")?;
        Some(Smtp {
            host,
            port: self
                .parsed("MAYA_SMTP_PORT", "ett portnummer, t.ex. 587")
                .unwrap_or(587),
            username: self.get("MAYA_SMTP_USERNAME"),
            password,
            from: self.get("MAYA_SMTP_FROM"),
        })
    }
}

/// Den tolkade konfigurationen som den skrivs ut vid start, utan hemligheter.
impl fmt::Display for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn line(f: &mut fmt::Formatter<'_>, name: &str, value: &dyn fmt::Display) -> fmt::Result {
            writeln!(f, "  {name:<36} {value}")
        }
        fn optional(value: &Option<String>) -> &dyn fmt::Display {
            match value {
                Some(v) => v,
                None => &"ej angivet",
            }
        }
        const SECRET: &str = "(hemlig, angiven)";

        writeln!(f, "Mayas konfiguration:")?;
        line(f, "MAYA_PUBLIC_URL", &self.public_url)?;
        line(f, "MAYA_LISTEN", &self.listen)?;
        line(f, "MAYA_DATA_DIR", &self.data_dir.display())?;
        let proxies = self
            .trusted_proxies
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        line(
            f,
            "MAYA_TRUSTED_PROXY",
            &if proxies.is_empty() {
                "ingen".to_string()
            } else {
                proxies
            },
        )?;
        match &self.backup {
            BackupTarget::Dir(dir) => line(f, "MAYA_BACKUP_DIR", &dir.display())?,
            BackupTarget::S3(s3) => {
                line(f, "MAYA_BACKUP_S3_ENDPOINT", &s3.endpoint)?;
                line(f, "MAYA_BACKUP_S3_BUCKET", &s3.bucket)?;
                line(f, "MAYA_BACKUP_S3_REGION", &s3.region)?;
                line(f, "MAYA_BACKUP_S3_ACCESS_KEY_ID", &s3.access_key_id)?;
                line(f, "MAYA_BACKUP_S3_SECRET_ACCESS_KEY", &SECRET)?;
            }
        }
        line(
            f,
            "MAYA_BACKUP_TIME",
            &format!("{} (Europe/Stockholm)", self.backup_time.strftime("%H:%M")),
        )?;
        match &self.smtp {
            None => line(f, "MAYA_SMTP_HOST", &"ej angivet (inga mejl skickas)")?,
            Some(smtp) => {
                line(f, "MAYA_SMTP_HOST", &smtp.host)?;
                line(f, "MAYA_SMTP_PORT", &smtp.port)?;
                line(f, "MAYA_SMTP_USERNAME", optional(&smtp.username))?;
                let password: &dyn fmt::Display = match smtp.password {
                    Some(_) => &SECRET,
                    None => &"ej angivet",
                };
                line(f, "MAYA_SMTP_PASSWORD", password)?;
                line(f, "MAYA_SMTP_FROM", optional(&smtp.from))?;
            }
        }
        line(f, "MAYA_DRIFT_EPOST", optional(&self.drift_epost))?;
        line(f, "MAYA_ARKIVPLAN_PLATS", optional(&self.arkivplan.plats))?;
        line(
            f,
            "MAYA_ARKIVPLAN_OFFSITE",
            optional(&self.arkivplan.offsite),
        )?;
        line(f, "MAYA_ARKIVPLAN_NYCKEL", optional(&self.arkivplan.nyckel))?;
        line(
            f,
            "MAYA_ARKIVPLAN_ANSVARIG",
            optional(&self.arkivplan.ansvarig),
        )
    }
}
