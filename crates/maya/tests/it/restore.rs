//! `maya restore` som driften kör det: tidpunkten visas, och inget ändras utan bekräftelse.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use maya_core::aktor::Aktör;
use maya_core::backup::{self, Backupmål};
use maya_core::installation::{self, RelyingParty};
use maya_core::{Databas, Klocka, installationslogg};

const MAYA: &str = env!("CARGO_BIN_EXE_maya");

fn maya(dir: &Path, publik_url: &str, args: &[&str], stdin: &str) -> Output {
    std::fs::write(
        dir.join(".env"),
        format!(
            "MAYA_PUBLIC_URL={publik_url}\nMAYA_BACKUP_DIR={backup}\nMAYA_DATA_DIR={data}\n",
            backup = dir.join("backup").display(),
            data = dir.join("data").display(),
        ),
    )
    .unwrap();
    let mut barn = Command::new(MAYA)
        .current_dir(dir)
        .env_clear()
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    barn.stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    barn.wait_with_output().unwrap()
}

/// En installation med en backup från 2026-10-18 03:00 i Stockholm.
async fn installation(dir: &Path) {
    let (data, backupdir) = (dir.join("data"), dir.join("backup"));
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&backupdir).unwrap();
    let db = Databas::öppna(&data.join("maya.db")).await.unwrap();
    installation::kontrollera_rp(
        &db,
        &RelyingParty {
            rp_id: "bokforing.example.se".into(),
            origin: "https://bokforing.example.se".into(),
        },
    )
    .await
    .unwrap();
    backup::ta_backup(
        &db,
        &Klocka::fast("2026-10-18T01:00:00Z".parse().unwrap()),
        &Aktör::maya(),
        &Backupmål::katalog(&backupdir),
        &dir.join("arbete"),
    )
    .await
    .unwrap();
    db.stäng().await;
}

async fn återställningar(dir: &Path) -> usize {
    let db = Databas::öppna(&dir.join("data/maya.db")).await.unwrap();
    let antal = installationslogg::läs(&db)
        .await
        .unwrap()
        .iter()
        .filter(|p| p.typ == "återställning gjord")
        .count();
    db.stäng().await;
    antal
}

#[tokio::test(flavor = "multi_thread")]
async fn maya_restore_visar_tidpunkten_och_återställer_efter_bekräftelse() {
    let dir = tempfile::tempdir().unwrap();
    installation(dir.path()).await;
    const URL: &str = "https://bokforing.example.se";

    let output = maya(dir.path(), URL, &["restore"], "nej\n");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!output.status.success(), "{output:?}");
    assert!(stdout.contains("maya-20261018T010000Z"), "{stdout}");
    assert!(
        stdout.contains("2026-10-18 03:00"),
        "tiden i Stockholm: {stdout}"
    );
    assert!(stdout.contains("Skriv ja"), "{stdout}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("avbruten"), "{stderr}");
    assert_eq!(återställningar(dir.path()).await, 0);

    let output = maya(dir.path(), URL, &["restore"], "ja\n");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{output:?}");
    assert!(stdout.contains("Installationen är återställd"), "{stdout}");
    assert!(stdout.contains("fore-aterstallning"), "{stdout}");
    assert_eq!(återställningar(dir.path()).await, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn maya_restore_vägrar_en_backup_från_en_annan_domän() {
    let dir = tempfile::tempdir().unwrap();
    installation(dir.path()).await;

    let output = maya(
        dir.path(),
        "https://annan.example.se",
        &["restore", "maya-20261018T010000Z"],
        "ja\n",
    );
    assert!(!output.status.success(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("rp_id bokforing.example.se"), "{stderr}");
    assert_eq!(återställningar(dir.path()).await, 0);
}
