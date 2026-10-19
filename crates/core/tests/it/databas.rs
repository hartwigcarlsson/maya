use maya_core::Databas;

#[tokio::test]
async fn en_ny_databas_far_migreringarna_och_bar_schemaepok_0() {
    let db = Databas::tillfällig().await;

    assert_eq!(db.schemaepok().await.unwrap(), "0");
}

#[tokio::test]
async fn en_befintlig_databas_oppnas_igen_utan_fel() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("maya.db");
    Databas::öppna(&path).await.unwrap().stäng().await;

    let db = Databas::öppna(&path).await.unwrap();

    assert_eq!(db.schemaepok().await.unwrap(), "0");
}

#[tokio::test]
async fn maya_vagrar_starta_mot_en_databas_med_nyare_schema_an_binaren() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("maya.db");
    Databas::öppna(&path).await.unwrap().stäng().await;
    // Som om en nyare Maya hade kört en migrering som den här binären inte känner till.
    kör_sql(
        &path,
        "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time)
         VALUES (99990101000000, 'från en nyare Maya', TRUE, x'00', 0)",
    )
    .await;

    let err = Databas::öppna(&path).await.unwrap_err();

    assert!(err.to_string().contains("nyare schema"), "{err}");
}

#[tokio::test]
async fn maya_vagrar_starta_mot_en_databas_fran_en_annan_schemaepok() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("maya.db");
    Databas::öppna(&path).await.unwrap().stäng().await;
    kör_sql(&path, "UPDATE schemaepok SET epok = '1'").await;

    let err = Databas::öppna(&path).await.unwrap_err();

    assert!(err.to_string().contains("schemaepok 1"), "{err}");
}

/// Ändrar databasen förbi `core`, för att ställa upp en databas som en annan Maya har skrivit.
async fn kör_sql(path: &std::path::Path, sql: &'static str) {
    use sqlx::Connection;
    let mut conn = sqlx::SqliteConnection::connect(&format!("sqlite://{}", path.display()))
        .await
        .unwrap();
    sqlx::query(sql).execute(&mut conn).await.unwrap();
    conn.close().await.unwrap();
}
