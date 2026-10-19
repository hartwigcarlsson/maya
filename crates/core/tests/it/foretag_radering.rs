//! Ett företag med räkenskapsinformation går inte att radera, inte heller förbi `core` med
//! främmande nycklar avstängda (H3, A1). Bara `foretag::radera` får ta bort ett företag utan
//! räkenskapsinformation, och den tömmer då varje tabell med företagets data.

use maya_core::{Databas, Klocka, foretag, testhjalp, verifikat};
use maya_domain::verifikat::{NyRad, NyttVerifikat};

/// En databasfil med ett företag; med `bokför` också ett bokfört verifikat (V1).
async fn databas(dir: &tempfile::TempDir, bokför: bool) -> std::path::PathBuf {
    let path = dir.path().join("maya.db");
    let db = Databas::öppna(&path).await.unwrap();
    let (anna, id) = testhjalp::företag(&db).await;
    if bokför {
        testhjalp::bokför(
            &db,
            &anna,
            id,
            "2026-03-14",
            &[(1930, 100, 0), (3001, 0, 100)],
        )
        .await;
    }
    db.stäng().await;
    path
}

/// Kör SQL förbi `core` med främmande nycklar avstängda, som `sqlite3` gör som standard.
async fn kör_sql_utan_främmande_nycklar(
    path: &std::path::Path,
    sql: &'static str,
) -> Result<(), String> {
    use sqlx::Connection;
    let mut conn = sqlx::SqliteConnection::connect(&format!("sqlite://{}", path.display()))
        .await
        .unwrap();
    sqlx::raw_sql("PRAGMA foreign_keys = OFF")
        .execute(&mut conn)
        .await
        .unwrap();
    let resultat = sqlx::raw_sql(sql).execute(&mut conn).await;
    conn.close().await.unwrap();
    resultat.map(|_| ()).map_err(|e| e.to_string())
}

async fn antal(path: &std::path::Path, tabell: &str) -> i64 {
    use sqlx::Connection;
    let mut conn = sqlx::SqliteConnection::connect(&format!("sqlite://{}", path.display()))
        .await
        .unwrap();
    let n = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT count(*) FROM {tabell}"
    )))
    .fetch_one(&mut conn)
    .await
    .unwrap();
    conn.close().await.unwrap();
    n
}

#[tokio::test]
async fn företaget_kontoplanen_och_historiken_raderas_inte_förbi_core_när_det_finns_bokfört() {
    let dir = tempfile::tempdir().unwrap();
    let path = databas(&dir, true).await;

    for sql in [
        "DELETE FROM foretag",
        "DELETE FROM behandlingshistorik",
        "DELETE FROM konto",
        "INSERT INTO foretag_radering (foretag_id) SELECT id FROM foretag",
    ] {
        assert!(
            kör_sql_utan_främmande_nycklar(&path, sql).await.is_err(),
            "{sql} gick igenom"
        );
    }
    assert_eq!(antal(&path, "foretag").await, 1);
    assert!(antal(&path, "behandlingshistorik").await > 0);
    assert!(antal(&path, "konto").await > 0);
}

#[tokio::test]
async fn ett_företag_med_ingående_balans_raderas_inte_förbi_core() {
    let dir = tempfile::tempdir().unwrap();
    let path = databas(&dir, false).await;
    kör_sql_utan_främmande_nycklar(
        &path,
        "INSERT INTO ingaende_balans (rakenskapsar_id, konto, saldo)
         SELECT id, 1930, 100000 FROM rakenskapsar",
    )
    .await
    .unwrap();

    for sql in [
        "INSERT INTO foretag_radering (foretag_id) SELECT id FROM foretag",
        "DELETE FROM foretag",
    ] {
        assert!(
            kör_sql_utan_främmande_nycklar(&path, sql).await.is_err(),
            "{sql} gick igenom"
        );
    }
}

#[tokio::test]
async fn historiken_och_kontoplanen_raderas_inte_förbi_core_ens_utan_bokfört() {
    let dir = tempfile::tempdir().unwrap();
    let path = databas(&dir, false).await;

    for sql in [
        "DELETE FROM foretag",
        "DELETE FROM behandlingshistorik",
        "DELETE FROM konto",
    ] {
        assert!(
            kör_sql_utan_främmande_nycklar(&path, sql).await.is_err(),
            "{sql} gick igenom"
        );
    }
}

/// Varje tabell med en kolumn som pekar på företaget, ett räkenskapsår, ett verifikat eller
/// ett underlag.
async fn företagets_tabeller(path: &std::path::Path) -> Vec<String> {
    use sqlx::Connection;
    let mut conn = sqlx::SqliteConnection::connect(&format!("sqlite://{}", path.display()))
        .await
        .unwrap();
    let tabeller: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT m.name FROM sqlite_master m, pragma_table_info(m.name) k
         WHERE m.type = 'table' AND k.name IN ('foretag_id', 'rakenskapsar_id', 'verifikat_id', 'underlag_id')
         ORDER BY m.name",
    )
    .fetch_all(&mut conn)
    .await
    .unwrap();
    conn.close().await.unwrap();
    tabeller
}

#[tokio::test]
async fn radera_tömmer_varje_tabell_med_företagets_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("maya.db");
    let db = Databas::öppna(&path).await.unwrap();
    let (anna, id) = testhjalp::företag(&db).await;
    // Ett utkast med en rad, så att verifikatets tabeller har rader.
    let utkast = NyttVerifikat {
        datum: "2026-03-14".into(),
        beskrivning: "Utkast".into(),
        rader: vec![NyRad {
            konto: "1930".into(),
            debet: "100".into(),
            ..NyRad::default()
        }],
    };
    verifikat::spara_utkast(&db, &Klocka::system(), &anna, id, None, &utkast)
        .await
        .unwrap();
    // #144: en leverantör med standardkonto.
    maya_core::leverantor::skapa(
        &db,
        &Klocka::system(),
        &anna,
        id,
        &maya_domain::leverantor::NyLeverantör {
            namn: "Kontorsvaror AB".into(),
            standardkonto: "5410".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let tabeller = företagets_tabeller(&path).await;
    for tabell in [
        "leverantor",
        "behandlingshistorik",
        "period",
        "verifikatrad",
        "underlagskoppling",
    ] {
        assert!(tabeller.contains(&tabell.to_string()), "{tabell} saknas");
    }

    foretag::radera(&db, &Klocka::system(), &anna, id)
        .await
        .unwrap();
    db.stäng().await;

    for tabell in tabeller {
        assert_eq!(antal(&path, &tabell).await, 0, "{tabell} har rader kvar");
    }
}
