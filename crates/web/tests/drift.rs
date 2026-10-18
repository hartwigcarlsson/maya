//! Driftvarningarna överst i Att göra och sidan Installation i användarmenyn, genom routern.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use maya_core::aktor::Aktör;
use maya_core::config::Config;
use maya_core::{Databas, Klocka, testhjalp};
use tower::ServiceExt;

const HOST: &str = "bokforing.example.se";

fn config(extra: &[(&str, &str)]) -> Config {
    Config::from_env(
        [
            ("MAYA_PUBLIC_URL", "https://bokforing.example.se"),
            ("MAYA_BACKUP_S3_ENDPOINT", "https://s3.example.se"),
            ("MAYA_BACKUP_S3_BUCKET", "maya"),
            ("MAYA_BACKUP_S3_ACCESS_KEY_ID", "maya-backup"),
            ("MAYA_BACKUP_S3_SECRET_ACCESS_KEY", "hemlig"),
        ]
        .iter()
        .chain(extra)
        .map(|(k, v)| (k.to_string(), v.to_string())),
    )
    .unwrap()
}

async fn hämta(db: &Databas, config: Config, session: &str, path: &str) -> String {
    let response = maya_web::router(Arc::new(config), db.clone())
        .oneshot(
            Request::get(path)
                .header(header::HOST, HOST)
                .header(header::COOKIE, format!("maya_session={session}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8_lossy(&body).into_owned();
    assert_eq!(status, StatusCode::OK, "{path}: {body}");
    body
}

async fn session(db: &Databas, anna: &Aktör) -> String {
    testhjalp::session(db, &Klocka::system(), anna).await
}

#[tokio::test]
async fn driftvarningarna_visas_överst_i_att_göra_bara_när_något_är_fel() {
    let db = Databas::tillfällig().await;
    let (anna, företag) = testhjalp::företag(&db).await;
    let session = session(&db, &anna).await;
    let bas = format!("/foretag/{företag}");

    // Ingen backup har lyckats än.
    let sida = hämta(&db, config(&[]), &session, &bas).await;
    assert!(sida.contains("Ingen backup har lyckats än"), "{sida}");
    assert_eq!(sida.matches(r#"class="driftvarning""#).count(), 1);
    assert!(sida.contains(r#"<span class="antal">1</span>"#), "{sida}");
    // Före nyckeltalen.
    assert!(
        sida.find("Ingen backup har lyckats än").unwrap() < sida.find("Företagskontot").unwrap(),
        "{sida}"
    );

    testhjalp::backup(&db, &Klocka::system()).await;
    let sida = hämta(&db, config(&[]), &session, &bas).await;
    assert_eq!(sida.matches(r#"class="driftvarning""#).count(), 0);
    assert!(!sida.contains(r#"<span class="antal">"#), "{sida}");
}

#[tokio::test]
async fn sidan_installation_visar_version_publik_url_backupstatus_och_arkivplanen() {
    let db = Databas::tillfällig().await;
    let (anna, _) = testhjalp::företag(&db).await;
    testhjalp::backup(&db, &Klocka::fast("2026-10-18T01:00:00Z".parse().unwrap())).await;
    let session = session(&db, &anna).await;
    let config = config(&[
        (
            "MAYA_ARKIVPLAN_OFFSITE",
            "Kopieras varje vecka till en disk hos ansvarig",
        ),
        ("MAYA_ARKIVPLAN_ANSVARIG", "Anna Andersson"),
        ("MAYA_DRIFT_EPOST", "drift@example.se"),
    ]);

    let sida = hämta(&db, config, &session, "/installation").await;
    assert!(sida.contains("<h1>Installation</h1>"), "{sida}");
    assert!(sida.contains(env!("CARGO_PKG_VERSION")), "{sida}");
    assert!(sida.contains("https://bokforing.example.se/"), "{sida}");
    assert!(sida.contains("S3-bucketen"), "{sida}");
    // Backupen 01:00 UTC är 03:00 i Stockholm.
    assert!(sida.contains("2026-10-18 03:00"), "{sida}");
    assert!(sida.contains("verifierad"), "{sida}");
    assert!(
        sida.contains("Kopieras varje vecka till en disk hos ansvarig"),
        "{sida}"
    );
    assert!(sida.contains("Anna Andersson"), "{sida}");
    assert!(sida.contains("drift@example.se"), "{sida}");
    // Krypteringsnyckeln saknas. Plats och land frågas inte efter: Maya förutsätter Sverige.
    assert!(sida.contains("ej angivet"), "{sida}");
    assert!(!sida.contains("Plats och land"), "{sida}");
    // Användarmenyn länkar hit.
    assert!(sida.contains(r#"href="/installation""#), "{sida}");
}

#[tokio::test]
async fn sidan_installation_visar_installationsloggen_nyast_först() {
    let db = Databas::tillfällig().await;
    let (anna, _) = testhjalp::företag(&db).await;
    let klocka = Klocka::system();
    let andra = maya_core::foretag::lägg_upp(
        &db,
        &klocka,
        &anna,
        &maya_domain::foretag::NyttFöretag {
            orgnr: "556677-8899".into(),
            namn: "Provbolaget AB".into(),
            första_år_start: "2026-01-01".into(),
            första_år_slut: "2026-12-31".into(),
            bokföringsmetod: "faktura".into(),
            momsperiod: "kvartal".into(),
        },
    )
    .await
    .unwrap();
    maya_core::foretag::radera(&db, &klocka, &anna, andra)
        .await
        .unwrap();
    maya_core::inloggning::återställ_användare(&db, &klocka, &Aktör::cli(), "anna@example.se")
        .await
        .unwrap();
    // Bo delar inget företag med Anna, så hon ser inte att han avaktiverades.
    testhjalp::användare(&db, "bo@example.se").await;
    maya_core::inloggning::avaktivera_användare(&db, &klocka, &Aktör::cli(), "bo@example.se")
        .await
        .unwrap();
    let session = session(&db, &anna).await;

    let sida = hämta(&db, config(&[]), &session, "/installation").await;

    assert!(sida.contains("Installationens logg"), "{sida}");
    let raderat = sida.find("företag raderat").expect(&sida);
    let återställd = sida.find("användare återställd").expect(&sida);
    assert!(återställd < raderat, "nyast först: {sida}");
    assert!(!sida.contains("bo@example.se"), "{sida}");
    assert!(sida.contains("556677-8899"), "{sida}");
    assert!(sida.contains("Provbolaget AB"), "{sida}");
}
