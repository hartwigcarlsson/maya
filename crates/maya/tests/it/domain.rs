//! `maya domain change` som driften kör den på servern efter att ha ändrat MAYA_PUBLIC_URL.

use std::path::Path;
use std::process::{Command, Output};

const MAYA: &str = env!("CARGO_BIN_EXE_maya");

fn maya(dir: &Path, url: &str, args: &[&str]) -> Output {
    std::fs::write(
        dir.join(".env"),
        format!(
            "MAYA_PUBLIC_URL={url}\nMAYA_BACKUP_DIR={backup}\nMAYA_DATA_DIR={data}\n",
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
fn maya_domain_change_byter_till_den_publika_urlens_doman() {
    let dir = tempfile::tempdir().unwrap();
    let gammal = "https://bokforing.example.se";
    let ny = "https://ny.example.se";
    let skapad = maya(dir.path(), gammal, &["user", "create", "anna@example.se"]);
    assert!(skapad.status.success(), "{skapad:?}");
    let vägrar = maya(dir.path(), ny, &["user", "reset", "anna@example.se"]);
    assert!(!vägrar.status.success(), "fel domän: {vägrar:?}");

    let output = maya(dir.path(), ny, &["domain", "change"]);

    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("ny.example.se"), "{stdout}");
    assert!(stdout.contains("maya user reset"), "{stdout}");
    let reset = maya(dir.path(), ny, &["user", "reset", "anna@example.se"]);
    assert!(reset.status.success(), "{reset:?}");
    assert!(
        String::from_utf8_lossy(&reset.stdout).contains("https://ny.example.se/registrera/"),
        "{reset:?}"
    );
}

#[test]
fn maya_domain_change_vagrar_nar_domanen_inte_har_andrats() {
    let dir = tempfile::tempdir().unwrap();
    let url = "https://bokforing.example.se";
    let skapad = maya(dir.path(), url, &["user", "create", "anna@example.se"]);
    assert!(skapad.status.success(), "{skapad:?}");

    let output = maya(dir.path(), url, &["domain", "change"]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("har redan domänen"), "{stderr}");
}
