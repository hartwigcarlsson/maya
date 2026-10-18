//! `maya serve` som driften kör den: binären med konfigurationen i miljön och en `.env`.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

const MAYA: &str = env!("CARGO_BIN_EXE_maya");

fn maya(cwd: &Path) -> Command {
    let mut command = Command::new(MAYA);
    command.current_dir(cwd).env_clear();
    command
}

#[test]
fn maya_serve_vagrar_starta_utan_publik_url_och_backupmal() {
    let dir = tempfile::tempdir().unwrap();

    let output = maya(dir.path()).arg("serve").output().unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("MAYA_PUBLIC_URL saknas"), "{stderr}");
    assert!(stderr.contains("Backupmål saknas"), "{stderr}");
}

#[test]
fn maya_serve_las_env_skriver_ut_konfigurationen_och_svarar_pa_healthz() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    std::fs::create_dir(&data).unwrap();
    let nyckel = dir.path().join("smtp-losenord");
    std::fs::write(&nyckel, "hemligt-losenord").unwrap();
    std::fs::write(
        dir.path().join(".env"),
        format!(
            "MAYA_PUBLIC_URL=https://bokforing.example.se\n\
             MAYA_BACKUP_DIR={backup}\n\
             MAYA_DATA_DIR={data}\n\
             MAYA_LISTEN=127.0.0.1:0\n\
             MAYA_SMTP_HOST=smtp.example.se\n\
             MAYA_SMTP_PASSWORD_FILE={nyckel}\n",
            backup = dir.path().display(),
            data = data.display(),
            nyckel = nyckel.display(),
        ),
    )
    .unwrap();

    let server = Server::starta(maya(dir.path()).arg("serve"));

    assert_eq!(server.healthz(), "HTTP/1.1 200 OK");
    assert!(
        data.join("maya.db").exists(),
        "databasen skapas och migreras vid start"
    );

    let healthcheck = maya(dir.path())
        .arg("healthcheck")
        .env("MAYA_LISTEN", server.adress.to_string())
        .output()
        .unwrap();
    assert!(healthcheck.status.success(), "{healthcheck:?}");

    let stdout = server.stoppa();
    assert!(stdout.contains("Mayas konfiguration"), "{stdout}");
    assert!(stdout.contains("https://bokforing.example.se"), "{stdout}");
    assert!(stdout.contains("(hemlig, angiven)"), "{stdout}");
    assert!(!stdout.contains("hemligt-losenord"), "{stdout}");
}

#[test]
fn maya_serve_vagrar_starta_nar_den_publika_url_en_har_bytt_domän() {
    let dir = tempfile::tempdir().unwrap();
    let konfiguration = |url: &str| {
        std::fs::write(
            dir.path().join(".env"),
            format!(
                "MAYA_PUBLIC_URL={url}\nMAYA_BACKUP_DIR={backup}\nMAYA_DATA_DIR={data}\n\
                 MAYA_LISTEN=127.0.0.1:0\n",
                backup = dir.path().display(),
                data = dir.path().join("data").display(),
            ),
        )
        .unwrap();
    };
    konfiguration("https://bokforing.example.se");
    let server = Server::starta(maya(dir.path()).arg("serve"));
    assert_eq!(server.healthz(), "HTTP/1.1 200 OK");
    server.stoppa();

    konfiguration("https://annan.example.se");
    let output = maya(dir.path()).arg("serve").output().unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("rp_id bokforing.example.se"), "{stderr}");
}

/// En körande `maya serve` på en port som operativsystemet valde (`MAYA_LISTEN=…:0`). Testerna
/// väntar på loggraden med den verkliga adressen i stället för att gissa en ledig port, så att
/// två servrar aldrig kan krocka på samma port.
struct Server {
    process: Child,
    adress: SocketAddr,
    stdout: Option<JoinHandle<String>>,
}

impl Server {
    fn starta(command: &mut Command) -> Self {
        let mut process = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = process.stdout.take().unwrap();
        let (adress_tx, adress_rx) = mpsc::channel();
        let läsare = std::thread::spawn(move || {
            let mut allt = String::new();
            for rad in BufReader::new(stdout).lines() {
                let Ok(rad) = rad else { break };
                if let Some(adress) = lyssnar_på(&rad) {
                    let _ = adress_tx.send(adress);
                }
                allt.push_str(&rad);
                allt.push('\n');
            }
            allt
        });
        let Ok(adress) = adress_rx.recv_timeout(Duration::from_secs(20)) else {
            let _ = process.kill();
            let mut stderr = String::new();
            let _ = process.stderr.take().unwrap().read_to_string(&mut stderr);
            let stdout = läsare.join().unwrap_or_default();
            panic!("servern startade inte:\n{stdout}\n{stderr}");
        };
        Server {
            process,
            adress,
            stdout: Some(läsare),
        }
    }

    /// Statusraden från `/healthz`. Servern lyssnar redan när adressen har loggats.
    fn healthz(&self) -> String {
        let mut stream = TcpStream::connect(self.adress).unwrap();
        write!(
            stream,
            "GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response.lines().next().unwrap_or_default().to_string()
    }

    fn stoppa(mut self) -> String {
        self.process.kill().unwrap();
        self.process.wait().unwrap();
        self.stdout.take().unwrap().join().unwrap()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

/// Adressen i loggraden "Maya … lyssnar på <adress>, publik adress …".
fn lyssnar_på(rad: &str) -> Option<SocketAddr> {
    let (_, efter) = rad.split_once("lyssnar på ")?;
    efter.split(',').next()?.trim().parse().ok()
}
