//! Ett bokfört verifikat är oföränderligt också förbi `core` (L1), och schemat avvisar att
//! bokföra något obalanserat (L7) eller med en lucka i serien (VF3).

use maya_core::verifikat::{self, Urval};
use maya_core::{Databas, testhjalp};

/// En databasfil med ett företag och ett bokfört verifikat (V1, id 1).
async fn databas_med_bokfört_verifikat(dir: &tempfile::TempDir) -> std::path::PathBuf {
    let path = dir.path().join("maya.db");
    let db = Databas::öppna(&path).await.unwrap();
    let (anna, id) = testhjalp::företag(&db).await;
    let v1 = testhjalp::bokför(
        &db,
        &anna,
        id,
        "2026-03-14",
        &[(1930, 100, 0), (3001, 0, 100)],
    );
    assert_eq!(v1.await.nummer.to_string(), "V1");
    assert_eq!(
        verifikat::lista(&db, &anna, id, Urval::Bokförda)
            .await
            .unwrap()
            .len(),
        1
    );
    db.stäng().await;
    path
}

/// Kör SQL förbi `core`, som en angripare eller ett felaktigt verktyg med databasfilen.
async fn kör_sql(path: &std::path::Path, sql: &'static str) -> Result<(), String> {
    use sqlx::Connection;
    let mut conn = sqlx::SqliteConnection::connect(&format!("sqlite://{}", path.display()))
        .await
        .unwrap();
    let resultat = sqlx::raw_sql(sql).execute(&mut conn).await;
    conn.close().await.unwrap();
    resultat.map(|_| ()).map_err(|e| e.to_string())
}

#[tokio::test]
async fn ett_bokfört_verifikat_och_dess_rader_går_inte_att_ändra_eller_radera() {
    let dir = tempfile::tempdir().unwrap();
    let path = databas_med_bokfört_verifikat(&dir).await;

    for (sql, fel) in [
        (
            "UPDATE verifikat SET beskrivning = 'annat'",
            "går inte att ändra",
        ),
        ("UPDATE verifikat SET nummer = 7", "går inte att ändra"),
        (
            "UPDATE verifikat SET datum = '2026-04-01'",
            "går inte att ändra",
        ),
        ("DELETE FROM verifikat", "går inte att radera"),
        ("UPDATE verifikatrad SET debet = 0", "går inte att ändra"),
        ("UPDATE verifikatrad SET konto = 1910", "går inte att ändra"),
        ("DELETE FROM verifikatrad", "går inte att radera"),
        (
            "INSERT INTO verifikatrad (verifikat_id, radnr, konto, debet, kredit)
             VALUES (1, 3, 1930, 5, 0)",
            "går inte att ändra",
        ),
    ] {
        let resultat = kör_sql(&path, sql).await;
        assert!(
            resultat.as_ref().unwrap_err().contains(fel),
            "{sql}: {resultat:?}"
        );
    }
}

#[tokio::test]
async fn schemat_avvisar_att_bokföra_obalanserat_eller_med_en_lucka_i_serien() {
    let dir = tempfile::tempdir().unwrap();
    let path = databas_med_bokfört_verifikat(&dir).await;
    let utkast = "INSERT INTO verifikat (id, foretag_id, rakenskapsar_id, datum, beskrivning,
                      kalla, skapad, skapad_kanal)
                  VALUES (2, 1, 1, '2026-03-15', 'Utkast', 'manuell', '2026-03-15T00:00:00Z',
                      'webb');";
    kör_sql(&path, utkast).await.unwrap();
    kör_sql(
        &path,
        "INSERT INTO verifikatrad (verifikat_id, radnr, konto, debet, kredit)
         VALUES (2, 1, 1930, 100, 0), (2, 2, 3001, 0, 90)",
    )
    .await
    .unwrap();
    const SOM_V2: &str = "UPDATE verifikat SET nummer = 2, bokford = '2026-03-15T00:00:00Z',
                              bokford_kanal = 'webb' WHERE id = 2";
    const SOM_V3: &str = "UPDATE verifikat SET nummer = 3, bokford = '2026-03-15T00:00:00Z',
                              bokford_kanal = 'webb' WHERE id = 2";

    let obalanserat = kör_sql(&path, SOM_V2).await;
    assert!(
        obalanserat
            .unwrap_err()
            .contains("debet är inte lika med kredit")
    );

    kör_sql(
        &path,
        "UPDATE verifikatrad SET kredit = 100 WHERE verifikat_id = 2 AND radnr = 2",
    )
    .await
    .unwrap();
    let lucka = kör_sql(&path, SOM_V3).await;
    assert!(lucka.unwrap_err().contains("lucka"));
    kör_sql(&path, SOM_V2).await.unwrap();

    let direkt = kör_sql(
        &path,
        "INSERT INTO verifikat (foretag_id, rakenskapsar_id, datum, beskrivning, kalla, skapad,
             skapad_kanal, nummer, bokford, bokford_kanal)
         VALUES (1, 1, '2026-03-15', 'Direkt', 'manuell', '2026-03-15T00:00:00Z', 'webb', 3,
             '2026-03-15T00:00:00Z', 'webb')",
    )
    .await;
    assert!(direkt.unwrap_err().contains("skapas som utkast"));
}
