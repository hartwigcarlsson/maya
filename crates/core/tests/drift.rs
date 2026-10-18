//! Driftvarningar under Att göra, genom `core` mot en tillfällig databas: varje varning visas
//! bara när villkoret gäller och försvinner när det är löst.

use std::path::Path;

use maya_core::aktor::Aktör;
use maya_core::att_gora::{self, Sak};
use maya_core::backup::{self, Backupmål};
use maya_core::config::Config;
use maya_core::drift::Driftvarning;
use maya_core::{Databas, Klocka, testhjalp};
use sqlx::Connection;

fn klocka(tid: &str) -> Klocka {
    Klocka::fast(tid.parse().unwrap())
}

/// En konfiguration där inget är fel: S3-mål.
fn hel_konfiguration(extra: &[(&str, &str)]) -> Config {
    // En variabel i `extra` ersätter standardvärdet; ett tomt värde räknas som saknat.
    let env: std::collections::HashMap<String, String> = [
        ("MAYA_PUBLIC_URL", "https://bokforing.example.se"),
        ("MAYA_BACKUP_S3_ENDPOINT", "https://s3.example.se"),
        ("MAYA_BACKUP_S3_BUCKET", "maya"),
        ("MAYA_BACKUP_S3_ACCESS_KEY_ID", "maya-backup"),
        ("MAYA_BACKUP_S3_SECRET_ACCESS_KEY", "hemlig"),
    ]
    .iter()
    .chain(extra)
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    Config::from_env(env).unwrap()
}

async fn driftvarningar(
    db: &Databas,
    k: &Klocka,
    config: &Config,
    anna: &Aktör,
    företag: maya_core::foretag::FöretagsId,
) -> Vec<Driftvarning> {
    att_gora::att_göra(db, k, config, anna, företag)
        .await
        .unwrap()
        .saker
        .into_iter()
        .filter_map(|s| match s {
            Sak::Driftvarning(v) => Some(v),
            _ => None,
        })
        .collect()
}

async fn ta_backup(db: &Databas, tid: &str, mål: &Path, arbete: &Path) -> String {
    backup::ta_backup(
        db,
        &klocka(tid),
        &Aktör::maya(),
        &Backupmål::katalog(mål),
        arbete,
    )
    .await
    .unwrap()
    .backup
    .namn
}

#[tokio::test]
async fn en_backup_äldre_än_36_timmar_ger_en_varning_som_försvinner_med_nästa_backup() {
    let db = Databas::tillfällig().await;
    let (anna, företag) = testhjalp::företag(&db).await;
    let config = hel_konfiguration(&[]);
    let (mål, arbete) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());

    // Användaren skapades 2026-01-01 och ingen backup har tagits: varning.
    let varningar = driftvarningar(
        &db,
        &klocka("2026-10-18T08:00:00Z"),
        &config,
        &anna,
        företag,
    )
    .await;
    assert_eq!(
        varningar,
        vec![Driftvarning::BackupGammal { senaste: None }]
    );

    ta_backup(&db, "2026-10-18T01:00:00Z", mål.path(), arbete.path()).await;
    // 36 timmar efter backupen är den inte för gammal än.
    let k = klocka("2026-10-19T13:00:00Z");
    assert_eq!(
        driftvarningar(&db, &k, &config, &anna, företag).await,
        vec![]
    );

    let k = klocka("2026-10-19T13:00:01Z");
    assert_eq!(
        driftvarningar(&db, &k, &config, &anna, företag).await,
        vec![Driftvarning::BackupGammal {
            senaste: Some("2026-10-18T01:00:00Z".parse().unwrap())
        }]
    );

    ta_backup(&db, "2026-10-19T13:00:00Z", mål.path(), arbete.path()).await;
    assert_eq!(
        driftvarningar(&db, &k, &config, &anna, företag).await,
        vec![]
    );
}

#[tokio::test]
async fn en_misslyckad_verifiering_ger_en_varning_tills_en_verifiering_lyckas() {
    let db = Databas::tillfällig().await;
    let (anna, företag) = testhjalp::företag(&db).await;
    let config = hel_konfiguration(&[]);
    let (mål, arbete) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let namn = ta_backup(&db, "2026-10-18T01:00:00Z", mål.path(), arbete.path()).await;

    let fil = mål.path().join(format!("{namn}.db"));
    let original = std::fs::read(&fil).unwrap();
    let mut conn = sqlx::SqliteConnection::connect(&format!("sqlite://{}", fil.display()))
        .await
        .unwrap();
    sqlx::raw_sql("CREATE TABLE manipulerad (x)")
        .execute(&mut conn)
        .await
        .unwrap();
    conn.close().await.unwrap();

    let k = klocka("2026-10-18T09:00:00Z");
    let mål_k = Backupmål::katalog(mål.path());
    backup::verifiera(&db, &k, &Aktör::cli(), &mål_k, Some(&namn), arbete.path())
        .await
        .unwrap();
    let varningar = driftvarningar(&db, &k, &config, &anna, företag).await;
    let [Driftvarning::VerifieringMisslyckades { tid, backup, .. }] = varningar.as_slice() else {
        panic!("väntade en varning om verifieringen: {varningar:?}");
    };
    assert_eq!(*tid, "2026-10-18T09:00:00Z".parse().unwrap());
    assert_eq!(backup.as_deref(), Some(namn.as_str()));

    std::fs::write(&fil, original).unwrap();
    backup::verifiera(&db, &k, &Aktör::cli(), &mål_k, Some(&namn), arbete.path())
        .await
        .unwrap();
    assert_eq!(
        driftvarningar(&db, &k, &config, &anna, företag).await,
        vec![]
    );
}

#[tokio::test]
async fn en_backupkatalog_som_inte_finns_ger_en_varning_men_samma_disk_gör_det_inte() {
    let db = Databas::tillfällig().await;
    let (anna, företag) = testhjalp::företag(&db).await;
    let (mål, arbete) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    ta_backup(&db, "2026-10-18T01:00:00Z", mål.path(), arbete.path()).await;
    let k = klocka("2026-10-18T08:00:00Z");

    let data = tempfile::tempdir().unwrap();
    let katalog = |dir: &str| {
        hel_konfiguration(&[
            ("MAYA_DATA_DIR", data.path().to_str().unwrap()),
            ("MAYA_BACKUP_DIR", dir),
            ("MAYA_BACKUP_S3_ENDPOINT", ""),
            ("MAYA_BACKUP_S3_BUCKET", ""),
            ("MAYA_BACKUP_S3_ACCESS_KEY_ID", ""),
            ("MAYA_BACKUP_S3_SECRET_ACCESS_KEY", ""),
        ])
    };

    // Att katalogen ligger på samma disk som databasen är driftens val: ingen varning.
    assert_eq!(
        driftvarningar(
            &db,
            &k,
            &katalog(mål.path().to_str().unwrap()),
            &anna,
            företag
        )
        .await,
        vec![]
    );

    let saknas = mål.path().join("saknas");
    let varningar =
        driftvarningar(&db, &k, &katalog(saknas.to_str().unwrap()), &anna, företag).await;
    let [varning @ Driftvarning::BackupkatalogSaknas(_)] = varningar.as_slice() else {
        panic!("väntade en varning om backupkatalogen: {varningar:?}");
    };
    assert_eq!(varning.rubrik(), "Backupkatalogen finns inte");
    assert!(
        varning.text().contains("saknas finns inte"),
        "{}",
        varning.text()
    );
}

#[tokio::test]
async fn driftvarningarna_syns_överst_i_varje_företag_och_räknas() {
    let db = Databas::tillfällig().await;
    let (anna, företag) = testhjalp::företag(&db).await;
    // Ingen backup har lyckats: en driftvarning.
    let config = hel_konfiguration(&[]);
    let k = klocka("2026-10-18T08:00:00Z");
    testhjalp::bokför(
        &db,
        &anna,
        företag,
        "2026-03-14",
        &[(1930, 12_500, 0), (3001, 0, 12_500)],
    )
    .await;
    maya_core::verifikat::spara_utkast(
        &db,
        &k,
        &anna,
        företag,
        None,
        &maya_domain::verifikat::NyttVerifikat::default(),
    )
    .await
    .unwrap();

    let saker = att_gora::att_göra(&db, &k, &config, &anna, företag)
        .await
        .unwrap()
        .saker;
    assert!(matches!(saker[0], Sak::Driftvarning(_)), "{saker:?}");
    assert!(matches!(saker[1], Sak::Utkast(_)), "{saker:?}");

    let antal = att_gora::antal(&db, &k, &config, &anna).await.unwrap();
    // Backupen saknas, plus utkastet.
    assert_eq!(antal[&företag], 2);
}
