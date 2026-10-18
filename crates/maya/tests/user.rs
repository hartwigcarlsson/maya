//! `maya user create` som driften kör den på servern.

use std::path::Path;
use std::process::{Command, Output};

const MAYA: &str = env!("CARGO_BIN_EXE_maya");

fn maya(dir: &Path, args: &[&str]) -> Output {
    std::fs::write(
        dir.join(".env"),
        format!(
            "MAYA_PUBLIC_URL=https://bokforing.example.se\nMAYA_BACKUP_DIR={backup}\n\
             MAYA_DATA_DIR={data}\n",
            backup = dir.display(),
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

#[test]
fn maya_user_create_skriver_ut_en_registreringslank_for_forsta_anvandaren() {
    let dir = tempfile::tempdir().unwrap();

    let output = maya(dir.path(), &["user", "create", "anna@example.se"]);

    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let länk = stdout
        .split_whitespace()
        .find(|ord| ord.starts_with("https://bokforing.example.se/registrera/"))
        .unwrap_or_else(|| panic!("ingen länk i {stdout}"));
    let token = länk.rsplit('/').next().unwrap();
    assert_eq!(token.len(), 64, "{länk}");
    assert!(stdout.contains("anna@example.se"), "{stdout}");
    assert!(stdout.contains("24 timmar"), "{stdout}");
}

#[test]
fn maya_user_create_vagrar_nar_det_redan_finns_en_anvandare() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        maya(dir.path(), &["user", "create", "anna@example.se"])
            .status
            .success()
    );

    let output = maya(dir.path(), &["user", "create", "bo@example.se"]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("finns redan en användare"), "{stderr}");
}

fn länk(stdout: &[u8]) -> String {
    let stdout = String::from_utf8_lossy(stdout);
    stdout
        .split_whitespace()
        .find(|ord| ord.starts_with("https://bokforing.example.se/registrera/"))
        .unwrap_or_else(|| panic!("ingen länk i {stdout}"))
        .to_string()
}

#[test]
fn maya_user_reset_skriver_ut_en_engangslank_som_galler_i_24_timmar() {
    let dir = tempfile::tempdir().unwrap();
    let skapad = maya(dir.path(), &["user", "create", "anna@example.se"]);

    let output = maya(dir.path(), &["user", "reset", "anna@example.se"]);

    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("24 timmar"), "{stdout}");
    assert!(stdout.contains("en gång"), "{stdout}");
    assert_ne!(länk(&output.stdout), länk(&skapad.stdout));
}

#[test]
fn maya_user_reset_for_en_okand_anvandare_misslyckas() {
    let dir = tempfile::tempdir().unwrap();
    maya(dir.path(), &["user", "create", "anna@example.se"]);

    let output = maya(dir.path(), &["user", "reset", "bo@example.se"]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ingen användare med den e-postadressen"),
        "{stderr}"
    );
}

#[test]
fn maya_user_deactivate_och_reactivate() {
    let dir = tempfile::tempdir().unwrap();
    maya(dir.path(), &["user", "create", "anna@example.se"]);

    let output = maya(dir.path(), &["user", "deactivate", "anna@example.se"]);
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("anna@example.se är avaktiverad"),
        "{stdout}"
    );

    let output = maya(dir.path(), &["user", "reset", "anna@example.se"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("avaktiverad"), "{stderr}");

    let output = maya(dir.path(), &["user", "reactivate", "anna@example.se"]);
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("anna@example.se är återaktiverad"),
        "{stdout}"
    );
    assert!(
        maya(dir.path(), &["user", "reset", "anna@example.se"])
            .status
            .success()
    );
}
