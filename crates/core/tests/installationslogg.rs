use jiff::Timestamp;
use maya_core::aktor::{Aktör, Kanal};
use maya_core::{Databas, Klocka, inloggning, installationslogg};

#[tokio::test]
async fn cli_handelser_loggas_med_aktor_och_tid() {
    let db = Databas::tillfällig().await;
    let tid: Timestamp = "2026-10-18T08:00:00Z".parse().unwrap();

    inloggning::skapa_första_användare(&db, &Klocka::fast(tid), &Aktör::cli(), "anna@example.se")
        .await
        .unwrap();

    let logg = installationslogg::läs(&db).await.unwrap();
    assert_eq!(logg.len(), 1);
    let post = &logg[0];
    assert_eq!(post.tid, tid);
    assert_eq!(post.aktör.kanal, Kanal::Cli);
    assert_eq!(post.typ, "användare skapad");
    assert_eq!(post.objekt, "anna@example.se");
    assert_eq!(
        post.beskrivning["registreringslänk_giltig_till"],
        "2026-10-19T08:00:00Z"
    );
}

#[tokio::test]
async fn en_handelse_som_vagras_loggas_inte() {
    let db = Databas::tillfällig().await;

    let _ = inloggning::skapa_första_användare(
        &db,
        &Klocka::system(),
        &Aktör::maya(),
        "anna@example.se",
    )
    .await;

    assert!(installationslogg::läs(&db).await.unwrap().is_empty());
}

#[tokio::test]
async fn installationsloggen_gar_inte_att_andra_eller_radera() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("maya.db");
    let db = Databas::öppna(&path).await.unwrap();
    inloggning::skapa_första_användare(&db, &Klocka::system(), &Aktör::cli(), "anna@example.se")
        .await
        .unwrap();
    db.stäng().await;

    // Förbi `core`, som en angripare eller ett felaktigt verktyg med databasfilen.
    let ändra = kör_sql(&path, "UPDATE installationslogg SET objekt = 'bo@example.se'").await;
    let radera = kör_sql(&path, "DELETE FROM installationslogg").await;

    assert!(ändra.unwrap_err().contains("går inte att ändra"));
    assert!(radera.unwrap_err().contains("går inte att radera"));
    let db = Databas::öppna(&path).await.unwrap();
    let logg = installationslogg::läs(&db).await.unwrap();
    assert_eq!(logg.len(), 1);
    assert_eq!(logg[0].objekt, "anna@example.se");
}

async fn kör_sql(path: &std::path::Path, sql: &'static str) -> Result<(), String> {
    use sqlx::Connection;
    let mut conn = sqlx::SqliteConnection::connect(&format!("sqlite://{}", path.display()))
        .await
        .unwrap();
    let resultat = sqlx::query(sql).execute(&mut conn).await;
    conn.close().await.unwrap();
    resultat.map(|_| ()).map_err(|e| e.to_string())
}
