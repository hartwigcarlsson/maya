//! `maya backup verify` som driften kör den för en provåterställning.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output};

use maya_core::aktor::Aktör;
use maya_core::backup::{self, Backupmål};
use maya_core::{Databas, Klocka, installationslogg};

const MAYA: &str = env!("CARGO_BIN_EXE_maya");

fn maya(dir: &Path, args: &[&str]) -> Output {
    std::fs::write(
        dir.join(".env"),
        format!(
            "MAYA_PUBLIC_URL=https://bokforing.example.se\nMAYA_BACKUP_DIR={backup}\n\
             MAYA_DATA_DIR={data}\n",
            backup = dir.join("backup").display(),
            data = dir.join("data").display(),
        ),
    )
    .unwrap();
    Command::new(MAYA)
        .current_dir(dir)
        .env_clear()
        .args(args)
        .output()
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn maya_backup_verify_rapporterar_på_svenska_och_loggas() {
    let dir = tempfile::tempdir().unwrap();
    let (data, backupdir) = (dir.path().join("data"), dir.path().join("backup"));
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&backupdir).unwrap();
    let db = Databas::öppna(&data.join("maya.db")).await.unwrap();
    let klocka = Klocka::fast("2026-10-18T01:00:00Z".parse().unwrap());
    let mål = Backupmål::katalog(&backupdir);
    backup::ta_backup(
        &db,
        &klocka,
        &Aktör::maya(),
        &mål,
        &dir.path().join("arbete"),
    )
    .await
    .unwrap();

    let output = maya(dir.path(), &["backup", "verify"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{output:?}");
    assert!(stdout.contains("maya-20261018T010000Z"), "{stdout}");
    assert!(
        stdout.contains("2026-10-18 03:00"),
        "tiden i Stockholm: {stdout}"
    );
    assert!(stdout.contains("Backupen är hel"), "{stdout}");

    // En ändrad fil.
    std::fs::OpenOptions::new()
        .append(true)
        .open(backupdir.join("maya-20261018T010000Z.db"))
        .unwrap()
        .write_all(b"x")
        .unwrap();
    let output = maya(dir.path(), &["backup", "verify", "maya-20261018T010000Z"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!output.status.success(), "{output:?}");
    assert!(stdout.contains("Backupen är inte hel"), "{stdout}");
    assert!(
        stdout.contains("databasfilens SHA-256 stämmer inte med manifestet"),
        "{stdout}"
    );

    let logg = installationslogg::läs(&db).await.unwrap();
    let verifieringar: Vec<_> = logg
        .iter()
        .filter(|p| p.typ == "backup verifierad")
        .map(|p| (p.aktör, p.beskrivning["lyckades"].as_bool().unwrap()))
        .collect();
    assert_eq!(verifieringar, [(Aktör::cli(), true), (Aktör::cli(), false)]);
    assert!(
        !backup::status(&db)
            .await
            .unwrap()
            .senaste_verifiering
            .unwrap()
            .lyckades
    );
}

#[test]
fn maya_backup_verify_utan_backup_säger_det() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("backup")).unwrap();
    let output = maya(dir.path(), &["backup", "verify"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ingen sådan backup"), "{stderr}");
}
