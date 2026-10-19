use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use maya_core::Databas;
use maya_core::config::Config;
use tower::ServiceExt;

fn config() -> Config {
    Config::from_env(
        [
            ("MAYA_PUBLIC_URL", "https://bokforing.example.se"),
            ("MAYA_BACKUP_DIR", "/backup"),
        ]
        .map(|(k, v)| (k.to_string(), v.to_string())),
    )
    .unwrap()
}

async fn get(host: &str, path: &str) -> (StatusCode, String) {
    let db = Databas::tillfällig().await;
    let app = maya_web::router(Arc::new(config()), (*db).clone());
    let response = app
        .oneshot(
            Request::get(path)
                .header(header::HOST, host)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&body).into_owned())
}

#[tokio::test]
async fn healthz_svarar_aven_for_dockers_halsokontroll_mot_localhost() {
    let (status, _) = get("localhost:8080", "/healthz").await;

    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn en_begaran_med_fel_host_avvisas() {
    let (status, body) = get("angripare.example.com", "/").await;

    assert_eq!(status, StatusCode::MISDIRECTED_REQUEST);
    assert!(body.contains("bokforing.example.se"), "{body}");
}

#[tokio::test]
async fn startsidan_ar_en_svensk_leptos_sida_med_hydrering() {
    let (status, body) = get("bokforing.example.se", "/").await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(r#"<html lang="sv""#), "{body}");
    assert!(body.contains("/pkg/maya.js"), "{body}");
    assert!(body.contains("Välkommen till Maya"), "{body}");
}
