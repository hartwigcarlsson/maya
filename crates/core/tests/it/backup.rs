//! Nattlig backup med verifiering (A5), till en katalog, genom `core` mot en tillfällig databas.
//! S3-målet prövas i `backup_s3.rs`.

use std::path::Path;

use maya_core::aktor::Aktör;
use maya_core::backup::{self, BackupFel, Backupmål};
use maya_core::installationslogg;
use maya_core::kontrollsumma::Kontrollsummor;
use maya_core::{Databas, Klocka, testhjalp, underlag};
use sqlx::Connection;

const NU: &str = "2026-10-18T01:00:00Z";

fn klocka(tid: &str) -> Klocka {
    Klocka::fast(tid.parse().unwrap())
}

/// En databas med ett bokfört verifikat och ett underlag.
async fn med_bokföring() -> maya_core::TillfälligDatabas {
    let db = Databas::tillfällig().await;
    let (anna, företag) = testhjalp::företag(&db).await;
    testhjalp::bokför(
        &db,
        &anna,
        företag,
        "2026-03-14",
        &[(1930, 12_500, 0), (3001, 0, 12_500)],
    )
    .await;
    underlag::ladda_upp(
        &db,
        &Klocka::system(),
        &anna,
        företag,
        "kvitto.txt",
        "text/plain",
        b"kvitto 1",
    )
    .await
    .unwrap();
    db
}

async fn kör_sql(fil: &Path, sql: &'static str) {
    let mut conn = sqlx::SqliteConnection::connect(&format!("sqlite://{}", fil.display()))
        .await
        .unwrap();
    sqlx::raw_sql(sql).execute(&mut conn).await.unwrap();
    conn.close().await.unwrap();
}

#[tokio::test]
async fn backupen_skriver_databasen_och_ett_manifest_med_kontrollsummorna() {
    let db = med_bokföring().await;
    let (mål_dir, arbete) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mål = Backupmål::katalog(mål_dir.path());

    let resultat = backup::ta_backup(&db, &klocka(NU), &Aktör::maya(), &mål, arbete.path())
        .await
        .unwrap();

    assert_eq!(resultat.backup.namn, "maya-20261018T010000Z");
    assert!(
        resultat.verifiering.lyckades(),
        "{:?}",
        resultat.verifiering.problem
    );
    assert!(mål_dir.path().join("maya-20261018T010000Z.db").is_file());
    assert!(mål_dir.path().join("maya-20261018T010000Z.json").is_file());
    let manifest = backup::manifest(&mål, "maya-20261018T010000Z")
        .await
        .unwrap();
    assert_eq!(manifest.tid, NU.parse().unwrap());
    assert_eq!(
        manifest.kontrollsummor,
        Kontrollsummor::räkna(&db).await.unwrap()
    );
    assert_eq!(manifest.kontrollsummor.verifikat[0].antal, 1);
    assert_eq!(manifest.kontrollsummor.underlag.len(), 1);
    // Arbetskatalogen städas.
    assert_eq!(std::fs::read_dir(arbete.path()).unwrap().count(), 0);

    let status = backup::status(&db).await.unwrap();
    let senaste = status.senaste_lyckade_backup.unwrap();
    assert_eq!(senaste.backup.as_deref(), Some("maya-20261018T010000Z"));
    assert_eq!(senaste.tid, NU.parse().unwrap());
    assert!(status.senaste_verifiering.unwrap().lyckades);
}

#[tokio::test]
async fn bara_maya_självt_tar_backup() {
    let db = Databas::tillfällig().await;
    let (anna, _) = testhjalp::företag(&db).await;
    let (mål_dir, arbete) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mål = Backupmål::katalog(mål_dir.path());
    for aktör in [anna, Aktör::cli()] {
        let fel = backup::ta_backup(&db, &klocka(NU), &aktör, &mål, arbete.path()).await;
        assert!(matches!(fel, Err(BackupFel::KanalenFårInte)), "{fel:?}");
    }
    assert_eq!(std::fs::read_dir(mål_dir.path()).unwrap().count(), 0);
}

#[tokio::test]
async fn en_backup_som_inte_kan_skrivas_sparas_som_misslyckad() {
    let db = Databas::tillfällig().await;
    let arbete = tempfile::tempdir().unwrap();
    let mål = Backupmål::katalog(&arbete.path().join("finns-inte"));
    let fel = backup::ta_backup(&db, &klocka(NU), &Aktör::maya(), &mål, arbete.path()).await;
    assert!(matches!(fel, Err(BackupFel::Mål(_))), "{fel:?}");
    let status = backup::status(&db).await.unwrap();
    let senaste = status.senaste_backup.unwrap();
    assert!(!senaste.lyckades);
    assert!(
        senaste.meddelande.contains("finns-inte"),
        "{}",
        senaste.meddelande
    );
    assert!(status.senaste_lyckade_backup.is_none());
}

#[tokio::test]
async fn verifieringen_upptäcker_en_manipulerad_backup() {
    let db = med_bokföring().await;
    let (mål_dir, arbete) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mål = Backupmål::katalog(mål_dir.path());
    let namn = backup::ta_backup(&db, &klocka(NU), &Aktör::maya(), &mål, arbete.path())
        .await
        .unwrap()
        .backup
        .namn;

    kör_sql(
        &mål_dir.path().join(format!("{namn}.db")),
        "DROP TRIGGER verifikatrad_bokford_andras_inte;
         UPDATE verifikatrad SET debet = 12600 WHERE debet = 12500;",
    )
    .await;

    let senare = klocka("2026-10-18T09:00:00Z");
    let verifiering = backup::verifiera(
        &db,
        &senare,
        &Aktör::cli(),
        &mål,
        Some(&namn),
        arbete.path(),
    )
    .await
    .unwrap();
    assert!(!verifiering.lyckades());
    let problem = verifiering.problem.join("\n");
    assert!(
        problem.contains("databasfilens SHA-256 stämmer inte med manifestet"),
        "{problem}"
    );
    assert!(problem.contains("konto 1930 för 5560360793"), "{problem}");
    assert!(
        problem.contains("hashen över bokförda rader skiljer sig"),
        "{problem}"
    );

    let status = backup::status(&db).await.unwrap();
    let senaste = status.senaste_verifiering.unwrap();
    assert!(!senaste.lyckades);
    assert_eq!(senaste.tid, "2026-10-18T09:00:00Z".parse().unwrap());
    // Backupen själv lyckades fortfarande.
    assert!(status.senaste_lyckade_backup.is_some());

    // CLI:ts verifiering hamnar i den installationsövergripande loggen.
    let logg = installationslogg::läs(&db).await.unwrap();
    let post = logg.last().unwrap();
    assert_eq!(post.typ, "backup verifierad");
    assert_eq!(post.aktör, Aktör::cli());
    assert_eq!(post.objekt, namn);
    assert_eq!(post.beskrivning["lyckades"], false);
}

#[tokio::test]
async fn verifieringen_upptäcker_ett_ändrat_underlag() {
    let db = med_bokföring().await;
    let (mål_dir, arbete) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mål = Backupmål::katalog(mål_dir.path());
    let namn = backup::ta_backup(&db, &klocka(NU), &Aktör::maya(), &mål, arbete.path())
        .await
        .unwrap()
        .backup
        .namn;

    kör_sql(
        &mål_dir.path().join(format!("{namn}.db")),
        "DROP TRIGGER underlag_andras_inte;
         UPDATE underlag SET innehall = CAST('kvitto 2' AS BLOB);",
    )
    .await;

    let verifiering = backup::verifiera(&db, &klocka(NU), &Aktör::cli(), &mål, None, arbete.path())
        .await
        .unwrap();
    assert_eq!(
        verifiering.backup.namn, namn,
        "utan namn verifieras den senaste"
    );
    let problem = verifiering.problem.join("\n");
    assert!(
        problem.contains("innehållet stämmer inte med underlagets SHA-256"),
        "{problem}"
    );
    assert!(
        problem.contains("har ändrats (SHA-256 skiljer sig)"),
        "{problem}"
    );
}

#[tokio::test]
async fn verifieringen_utan_backup_är_ett_fel() {
    let db = Databas::tillfällig().await;
    let (mål_dir, arbete) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mål = Backupmål::katalog(mål_dir.path());
    let fel = backup::verifiera(&db, &klocka(NU), &Aktör::cli(), &mål, None, arbete.path()).await;
    assert!(matches!(fel, Err(BackupFel::IngenBackup)), "{fel:?}");
    let fel = backup::verifiera(
        &db,
        &klocka(NU),
        &Aktör::cli(),
        &mål,
        Some("maya-20200101T000000Z"),
        arbete.path(),
    )
    .await;
    assert!(matches!(fel, Err(BackupFel::IngenBackup)), "{fel:?}");
}

#[tokio::test]
async fn gallringen_behåller_sju_dagliga_och_tolv_månatliga_i_målet() {
    let db = Databas::tillfällig().await;
    let (mål_dir, arbete) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mål = Backupmål::katalog(mål_dir.path());
    let mut tider = vec!["2026-08-15T01:00:00Z".to_string()];
    tider.extend((20..=29).map(|d| format!("2026-09-{d}T01:00:00Z")));
    for tid in &tider {
        backup::ta_backup(&db, &klocka(tid), &Aktör::maya(), &mål, arbete.path())
            .await
            .unwrap();
    }
    let kvar: Vec<String> = backup::lista(&mål)
        .await
        .unwrap()
        .into_iter()
        .map(|b| b.namn)
        .collect();
    let mut väntat = vec!["maya-20260815T010000Z".to_string()];
    väntat.extend((23..=29).map(|d| format!("maya-202609{d}T010000Z")));
    assert_eq!(kvar, väntat);
    // Både databas och manifest är borta för de gallrade.
    assert_eq!(
        std::fs::read_dir(mål_dir.path()).unwrap().count(),
        2 * väntat.len()
    );
}

#[tokio::test]
async fn en_gallring_som_misslyckas_gör_inte_backupen_misslyckad() {
    let db = Databas::tillfällig().await;
    let (mål_dir, arbete) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mål = Backupmål::katalog(mål_dir.path());
    // En rest utan manifest som inte går att ta bort: en katalog med en backups filnamn.
    std::fs::create_dir(mål_dir.path().join("maya-20200101T000000Z.db")).unwrap();

    let resultat = backup::ta_backup(&db, &klocka(NU), &Aktör::maya(), &mål, arbete.path())
        .await
        .unwrap();

    assert!(resultat.verifiering.lyckades());
    let fel = resultat.gallringsfel.expect("gallringen misslyckades");
    assert!(fel.contains("maya-20200101T000000Z.db"), "{fel}");
    let status = backup::status(&db).await.unwrap();
    assert!(status.senaste_backup.unwrap().lyckades);
}

#[tokio::test]
async fn gallringen_tränger_inte_undan_goda_backuper_med_misslyckade() {
    let db = Databas::tillfällig().await;
    let (mål_dir, arbete) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mål = Backupmål::katalog(mål_dir.path());
    let ta = async |dag: u32| {
        backup::ta_backup(
            &db,
            &klocka(&format!("2026-09-{dag:02}T01:00:00Z")),
            &Aktör::maya(),
            &mål,
            arbete.path(),
        )
        .await
        .unwrap()
        .backup
        .namn
    };
    for dag in 1..=7 {
        ta(dag).await;
    }
    // Tio nätter där backupen i målet sedan visar sig vara trasig.
    for dag in 10..=19 {
        let namn = ta(dag).await;
        kör_sql(
            &mål_dir.path().join(format!("{namn}.db")),
            "CREATE TABLE trasig (id INTEGER)",
        )
        .await;
        let verifiering = backup::verifiera(
            &db,
            &klocka(&format!("2026-09-{dag:02}T09:00:00Z")),
            &Aktör::cli(),
            &mål,
            Some(&namn),
            arbete.path(),
        )
        .await
        .unwrap();
        assert!(!verifiering.lyckades());
    }
    ta(20).await;

    let kvar: Vec<String> = backup::lista(&mål)
        .await
        .unwrap()
        .into_iter()
        .map(|b| b.namn)
        .collect();
    let mut väntat: Vec<String> = (2..=7)
        .map(|d| format!("maya-2026090{d}T010000Z"))
        .collect();
    väntat.push("maya-20260920T010000Z".into());
    assert_eq!(kvar, väntat);
}

#[tokio::test]
async fn backupen_kan_hämtas_till_en_tom_katalog_med_samma_kontrollsummor() {
    let db = med_bokföring().await;
    let före = Kontrollsummor::räkna(&db).await.unwrap();
    let (mål_dir, arbete, tom) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    let mål = Backupmål::katalog(mål_dir.path());
    let namn = backup::ta_backup(&db, &klocka(NU), &Aktör::maya(), &mål, arbete.path())
        .await
        .unwrap()
        .backup
        .namn;

    let fil = backup::hämta(&mål, &namn, tom.path()).await.unwrap();
    assert_eq!(fil, tom.path().join("maya.db"));
    let återställd = Databas::öppna(&fil).await.unwrap();
    assert_eq!(Kontrollsummor::räkna(&återställd).await.unwrap(), före);
}
