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
    let ändra = kör_sql(
        &path,
        "UPDATE installationslogg SET objekt = 'bo@example.se'",
    )
    .await;
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

mod sidan_installation {
    //! Loggen på sidan Installation: en användare ser bara poster om sig själv, om användare
    //! hen delar ett företag med och om företag hen har eller hade åtkomst till (story 36).

    use maya_core::aktor::Aktör;
    use maya_core::atkomst::{self, Inbjudan};
    use maya_core::config::Config;
    use maya_core::foretag::{self, FöretagsId};
    use maya_core::{Databas, Klocka, drift, inloggning, testhjalp};
    use maya_domain::foretag::NyttFöretag;

    fn config(backup: &std::path::Path) -> Config {
        Config::from_env(
            [
                ("MAYA_PUBLIC_URL", "https://bokforing.example.se"),
                ("MAYA_BACKUP_DIR", backup.to_str().unwrap()),
            ]
            .map(|(k, v)| (k.to_string(), v.to_string())),
        )
        .unwrap()
    }

    async fn loggen(db: &Databas, aktör: &Aktör) -> Vec<(String, String)> {
        let backup = tempfile::tempdir().unwrap();
        drift::installation(db, &Klocka::system(), &config(backup.path()), aktör)
            .await
            .unwrap()
            .logg
            .into_iter()
            .map(|p| (p.typ, p.objekt))
            .collect()
    }

    async fn eget_företag(db: &Databas, aktör: &Aktör, orgnr: &str) -> FöretagsId {
        foretag::lägg_upp(
            db,
            &Klocka::system(),
            aktör,
            &NyttFöretag {
                orgnr: orgnr.into(),
                namn: format!("Bolag {orgnr}"),
                första_år_start: "2026-01-01".into(),
                första_år_slut: "2026-12-31".into(),
                bokföringsmetod: "faktura".into(),
                momsperiod: "kvartal".into(),
            },
        )
        .await
        .unwrap()
    }

    /// Anna med Exempel AB, som hon delar med Cecilia. Bo har ett eget företag.
    async fn tre_användare(db: &Databas) -> (Aktör, Aktör, Aktör, FöretagsId) {
        let k = Klocka::system();
        let (anna, exempel) = testhjalp::företag(db).await;
        let bo = testhjalp::användare(db, "bo@example.se").await;
        eget_företag(db, &bo, "556677-8899").await;
        let cecilia = testhjalp::användare(db, "cecilia@example.se").await;
        let inloggad = testhjalp::inloggad(db, &k, &anna).await;
        let inbjudan = atkomst::bjud_in(db, &k, &inloggad, exempel, "cecilia@example.se")
            .await
            .unwrap();
        assert!(matches!(inbjudan, Inbjudan::ÅtkomstGiven));
        (anna, bo, cecilia, exempel)
    }

    fn post(typ: &str, objekt: &str) -> (String, String) {
        (typ.into(), objekt.into())
    }

    #[tokio::test]
    async fn en_användare_ser_bara_poster_om_sig_själv_och_dem_hen_delar_ett_företag_med() {
        let db = Databas::tillfällig().await;
        let (anna, bo, _cecilia, _) = tre_användare(&db).await;
        for epost in ["anna@example.se", "bo@example.se", "cecilia@example.se"] {
            inloggning::återställ_användare(&db, &Klocka::system(), &Aktör::cli(), epost)
                .await
                .unwrap();
        }

        assert_eq!(
            loggen(&db, &anna).await,
            [
                post("användare återställd", "cecilia@example.se"),
                post("användare återställd", "anna@example.se"),
            ]
        );
        assert_eq!(
            loggen(&db, &bo).await,
            [post("användare återställd", "bo@example.se")]
        );
        assert_eq!(loggen(&db, &Aktör::cli()).await.len(), 3);
    }

    #[tokio::test]
    async fn ett_raderat_företag_syns_bara_för_dem_som_hade_åtkomst_till_det() {
        let db = Databas::tillfällig().await;
        let (anna, bo, cecilia, exempel) = tre_användare(&db).await;
        let annat = eget_företag(&db, &bo, "559000-0005").await;

        foretag::radera(&db, &Klocka::system(), &anna, exempel)
            .await
            .unwrap();
        foretag::radera(&db, &Klocka::system(), &bo, annat)
            .await
            .unwrap();

        let exempel_raderat = [post("företag raderat", "556036-0793")];
        assert_eq!(loggen(&db, &anna).await, exempel_raderat);
        assert_eq!(loggen(&db, &cecilia).await, exempel_raderat);
        assert_eq!(
            loggen(&db, &bo).await,
            [post("företag raderat", "559000-0005")]
        );
    }
}
