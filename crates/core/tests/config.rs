use maya_core::config::Config;

fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
fn utan_publik_url_vagrar_maya_starta() {
    let err = Config::from_env(env(&[("MAYA_BACKUP_DIR", "/backup")])).unwrap_err();

    assert!(err.to_string().contains("MAYA_PUBLIC_URL saknas"), "{err}");
}

#[test]
fn utan_backupmal_vagrar_maya_starta() {
    let err =
        Config::from_env(env(&[("MAYA_PUBLIC_URL", "https://bokforing.example.se")])).unwrap_err();

    assert!(err.to_string().contains("Backupmål saknas"), "{err}");
}

#[test]
fn en_hemlighet_lases_ur_filen_i_file_varianten_och_skrivs_aldrig_ut() {
    let dir = tempfile::tempdir().unwrap();
    let fil = dir.path().join("s3-nyckel");
    std::fs::write(&fil, "hemlig-nyckel-123\n").unwrap();

    let config = Config::from_env(env(&[
        ("MAYA_PUBLIC_URL", "https://bokforing.example.se"),
        ("MAYA_BACKUP_S3_ENDPOINT", "https://s3.example.se"),
        ("MAYA_BACKUP_S3_BUCKET", "maya"),
        ("MAYA_BACKUP_S3_ACCESS_KEY_ID", "maya-backup"),
        (
            "MAYA_BACKUP_S3_SECRET_ACCESS_KEY_FILE",
            fil.to_str().unwrap(),
        ),
    ]))
    .unwrap();

    let maya_core::config::BackupTarget::S3(s3) = &config.backup else {
        panic!("väntade ett S3-mål: {config}");
    };
    assert_eq!(s3.secret_access_key.expose(), "hemlig-nyckel-123");
    let utskrift = format!("{config} {config:?}");
    assert!(!utskrift.contains("hemlig-nyckel-123"), "{utskrift}");
    assert!(
        utskrift.contains("https://bokforing.example.se"),
        "{utskrift}"
    );
    assert!(utskrift.contains("maya-backup"), "{utskrift}");
}

#[test]
fn en_fil_som_inte_gar_att_lasa_ger_ett_fel_som_namnger_variabeln() {
    let err = Config::from_env(env(&[
        ("MAYA_PUBLIC_URL", "https://bokforing.example.se"),
        ("MAYA_BACKUP_DIR", "/backup"),
        ("MAYA_SMTP_HOST", "smtp.example.se"),
        ("MAYA_SMTP_PASSWORD_FILE", "/finns/inte"),
    ]))
    .unwrap_err();

    assert!(err.to_string().contains("MAYA_SMTP_PASSWORD_FILE"), "{err}");
}

#[test]
fn utan_ovriga_variabler_lyssnar_maya_pa_8080_med_databasen_i_data() {
    let config = Config::from_env(env(&[
        ("MAYA_PUBLIC_URL", "https://bokforing.example.se"),
        ("MAYA_BACKUP_DIR", "/backup"),
    ]))
    .unwrap();

    assert_eq!(config.listen.to_string(), "0.0.0.0:8080");
    assert_eq!(config.data_dir, std::path::Path::new("/data"));
    assert!(config.trusted_proxies.is_empty());
}

#[test]
fn den_publika_url_en_ar_ett_origin_over_https() {
    for fel in [
        "http://bokforing.example.se",
        "https://bokforing.example.se/maya",
        "inte en adress",
    ] {
        let err = Config::from_env(env(&[
            ("MAYA_PUBLIC_URL", fel),
            ("MAYA_BACKUP_DIR", "/backup"),
        ]))
        .unwrap_err();
        assert!(err.to_string().contains("MAYA_PUBLIC_URL"), "{fel}: {err}");
    }
}

#[test]
fn den_publika_url_en_ger_varden_som_host_maste_ha() {
    let config = Config::from_env(env(&[
        ("MAYA_PUBLIC_URL", "https://bokforing.example.se:8443/"),
        ("MAYA_BACKUP_DIR", "/backup"),
    ]))
    .unwrap();

    assert_eq!(config.public_host(), "bokforing.example.se:8443");
}

#[test]
fn betrodda_proxyer_anges_som_adresser_eller_natblock() {
    let config = Config::from_env(env(&[
        ("MAYA_PUBLIC_URL", "https://bokforing.example.se"),
        ("MAYA_BACKUP_DIR", "/backup"),
        ("MAYA_TRUSTED_PROXY", "172.31.80.2, 10.0.0.0/8"),
    ]))
    .unwrap();

    assert!(config.is_trusted_proxy("172.31.80.2".parse().unwrap()));
    assert!(config.is_trusted_proxy("10.1.2.3".parse().unwrap()));
    assert!(!config.is_trusted_proxy("172.31.80.3".parse().unwrap()));
}
