//! Behandlingshistoriken är append-only också förbi `core` (H3).

use maya_core::foretag;
use maya_core::{Databas, Klocka, behandlingshistorik, testhjalp};
use maya_domain::foretag::NyttFöretag;

#[tokio::test]
async fn behandlingshistoriken_går_inte_att_ändra_eller_radera() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("maya.db");
    let db = Databas::öppna(&path).await.unwrap();
    let anna = testhjalp::användare(&db, "anna@example.se").await;
    let id = foretag::lägg_upp(
        &db,
        &Klocka::system(),
        &anna,
        &NyttFöretag {
            orgnr: "556036-0793".into(),
            namn: "Exempel AB".into(),
            första_år_start: "2026-01-01".into(),
            första_år_slut: "2026-12-31".into(),
            bokföringsmetod: "faktura".into(),
            momsperiod: "manad".into(),
        },
    )
    .await
    .unwrap();
    db.stäng().await;

    // Förbi `core`, som en angripare eller ett felaktigt verktyg med databasfilen.
    let ändra = kör_sql(&path, "UPDATE behandlingshistorik SET objekt = 'annat'").await;
    let radera = kör_sql(&path, "DELETE FROM behandlingshistorik").await;

    assert!(ändra.unwrap_err().contains("går inte att ändra"));
    assert!(radera.unwrap_err().contains("går inte att radera"));
    let db = Databas::öppna(&path).await.unwrap();
    let historik = behandlingshistorik::läs(&db, &anna, id).await.unwrap();
    assert_eq!(historik.len(), 1);
    assert_eq!(historik[0].objekt, "556036-0793");
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
