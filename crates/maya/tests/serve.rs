//! `maya serve` som driften kör den: binären med konfigurationen i miljön och en `.env`.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

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
    let port = ledig_port();
    std::fs::write(
        dir.path().join(".env"),
        format!(
            "MAYA_PUBLIC_URL=https://bokforing.example.se\n\
             MAYA_BACKUP_DIR={backup}\n\
             MAYA_DATA_DIR={data}\n\
             MAYA_LISTEN=127.0.0.1:{port}\n\
             MAYA_SMTP_HOST=smtp.example.se\n\
             MAYA_SMTP_PASSWORD_FILE={nyckel}\n",
            backup = dir.path().display(),
            data = data.display(),
            nyckel = nyckel.display(),
        ),
    )
    .unwrap();

    let server = Server(
        maya(dir.path())
            .arg("serve")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );

    let status = vänta_på_healthz(port, &server);
    assert_eq!(status, "HTTP/1.1 200 OK");
    assert!(
        data.join("maya.db").exists(),
        "databasen skapas och migreras vid start"
    );

    let healthcheck = maya(dir.path())
        .arg("healthcheck")
        .env("MAYA_LISTEN", format!("127.0.0.1:{port}"))
        .output()
        .unwrap();
    assert!(healthcheck.status.success(), "{healthcheck:?}");

    let stdout = server.stoppa();
    assert!(stdout.contains("Mayas konfiguration"), "{stdout}");
    assert!(stdout.contains("https://bokforing.example.se"), "{stdout}");
    assert!(stdout.contains("(hemlig, angiven)"), "{stdout}");
    assert!(!stdout.contains("hemligt-losenord"), "{stdout}");
}

struct Server(Child);

impl Server {
    fn stoppa(mut self) -> String {
        self.0.kill().unwrap();
        let mut stdout = String::new();
        self.0
            .stdout
            .take()
            .unwrap()
            .read_to_string(&mut stdout)
            .unwrap();
        stdout
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
    }
}

fn ledig_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Statusraden från `/healthz`, när servern har startat.
fn vänta_på_healthz(port: u16, server: &Server) -> String {
    let start = Instant::now();
    loop {
        if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) {
            write!(
                stream,
                "GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            return response.lines().next().unwrap_or_default().to_string();
        }
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "servern startade inte (pid {})",
            server.0.id()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}
