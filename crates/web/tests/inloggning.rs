//! Webben före och efter inloggning, genom routern. Passkey-ceremonierna själva prövas av
//! röktestet i `crates/maya/tests/rok.rs`, som har en riktig webbläsare.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use http_body_util::BodyExt;
use maya_core::aktor::Aktör;
use maya_core::config::Config;
use maya_core::inloggning::{self, NyPasskey};
use maya_core::{Databas, Klocka, TillfälligDatabas};
use tower::ServiceExt;

const HOST: &str = "bokforing.example.se";
const ORIGIN: &str = "https://bokforing.example.se";

fn app(db: &TillfälligDatabas) -> Router {
    let config = Config::from_env(
        [("MAYA_PUBLIC_URL", ORIGIN), ("MAYA_BACKUP_DIR", "/backup")]
            .map(|(k, v)| (k.to_string(), v.to_string())),
    )
    .unwrap();
    maya_web::router(Arc::new(config), (**db).clone())
}

async fn skicka(app: Router, request: Request<Body>) -> (Response<()>, String) {
    let response = app.oneshot(request).await.unwrap();
    let (parts, body) = response.into_parts();
    let body = body.collect().await.unwrap().to_bytes();
    (
        Response::from_parts(parts, ()),
        String::from_utf8_lossy(&body).into_owned(),
    )
}

fn get(path: &str) -> axum::http::request::Builder {
    Request::get(path).header(header::HOST, HOST)
}

fn post_json(path: &str, json: &str) -> Request<Body> {
    Request::post(path)
        .header(header::HOST, HOST)
        .header(header::ORIGIN, ORIGIN)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json.to_string()))
        .unwrap()
}

async fn skapa_användare(db: &Databas) -> String {
    inloggning::skapa_första_användare(db, &Klocka::system(), &Aktör::cli(), "anna@example.se")
        .await
        .unwrap()
        .token()
        .to_string()
}

/// En session som om användaren hade registrerat en passkey.
async fn inloggad(db: &Databas) -> String {
    let token = skapa_användare(db).await;
    let ny = NyPasskey {
        credential_id: b"ett".to_vec(),
        namn: "MacBook".into(),
        passkey: "{}".into(),
    };
    inloggning::slutför_registrering(db, &Klocka::system(), &token, ny)
        .await
        .unwrap()
        .token()
        .to_string()
}

#[tokio::test]
async fn innan_nagon_anvandare_finns_visar_webben_bara_hanvisningen_till_cli() {
    let db = Databas::tillfällig().await;

    for path in ["/", "/registrera/abc"] {
        let (response, body) = skicka(app(&db), get(path).body(Body::empty()).unwrap()).await;

        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert!(body.contains("maya user create"), "{path}: {body}");
        assert!(!body.contains("<form"), "{path}: {body}");
    }
}

#[tokio::test]
async fn utloggad_ser_inloggningen_med_e_post_forst() {
    let db = Databas::tillfällig().await;
    skapa_användare(&db).await;

    let (_, body) = skicka(app(&db), get("/").body(Body::empty()).unwrap()).await;

    assert!(body.contains(r#"type="email""#), "{body}");
    assert!(body.contains(r#"autocomplete="username""#), "{body}");
    assert!(!body.contains("webauthn\""), "ingen conditional UI: {body}");
    assert!(!body.contains("maya user create"), "{body}");
}

#[tokio::test]
async fn registreringssidan_har_formularet_for_passkeyn() {
    let db = Databas::tillfällig().await;
    let token = skapa_användare(&db).await;

    let (_, body) = skicka(
        app(&db),
        get(&format!("/registrera/{token}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert!(body.contains("Registrera passkey"), "{body}");
    assert!(body.contains(&token), "{body}");
}

#[tokio::test]
async fn sessionscookien_gor_anvandaren_inloggad() {
    let db = Databas::tillfällig().await;
    let session = inloggad(&db).await;

    let (_, body) = skicka(
        app(&db),
        get("/")
            .header(header::COOKIE, format!("maya_session={session}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert!(body.contains("anna@example.se"), "{body}");
    assert!(body.contains("Logga ut"), "{body}");
    assert!(!body.contains(r#"type="email""#), "{body}");
}

#[tokio::test]
async fn utloggning_avslutar_sessionen_och_tar_bort_cookien() {
    let db = Databas::tillfällig().await;
    let session = inloggad(&db).await;

    let (response, _) = skicka(
        app(&db),
        Request::post("/auth/utloggning")
            .header(header::HOST, HOST)
            .header(header::ORIGIN, ORIGIN)
            .header(header::COOKIE, format!("maya_session={session}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let cookie = response.headers()[header::SET_COOKIE].to_str().unwrap();
    assert!(cookie.starts_with("maya_session=;"), "{cookie}");
    assert!(cookie.contains("Max-Age=0"), "{cookie}");
    assert!(
        inloggning::session(&db, &Klocka::system(), &session)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn en_post_fran_ett_annat_origin_avvisas() {
    let db = Databas::tillfällig().await;
    let session = inloggad(&db).await;

    let (response, _) = skicka(
        app(&db),
        Request::post("/auth/utloggning")
            .header(header::HOST, HOST)
            .header(header::ORIGIN, "https://angripare.example.com")
            .header(header::COOKIE, format!("maya_session={session}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(
        inloggning::session(&db, &Klocka::system(), &session)
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn inloggning_for_en_okand_e_postadress_ger_ett_fel_pa_svenska() {
    let db = Databas::tillfällig().await;
    skapa_användare(&db).await;

    let (response, body) = skicka(
        app(&db),
        post_json("/auth/inloggning/start", r#"{"epost":"okand@example.se"}"#),
    )
    .await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(body.contains("ingen användare med passkey"), "{body}");
}

#[tokio::test]
async fn registrering_med_en_ogiltig_lank_ger_ett_fel() {
    let db = Databas::tillfällig().await;
    skapa_användare(&db).await;

    let (response, body) = skicka(
        app(&db),
        post_json("/auth/registrering/start", r#"{"token":"fel"}"#),
    )
    .await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(body.contains("länken är ogiltig"), "{body}");
}

#[tokio::test]
async fn registrering_med_en_giltig_lank_ger_en_webauthn_utmaning() {
    let db = Databas::tillfällig().await;
    let token = skapa_användare(&db).await;

    let (response, body) = skicka(
        app(&db),
        post_json(
            "/auth/registrering/start",
            &format!(r#"{{"token":"{token}"}}"#),
        ),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK, "{body}");
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(json["ceremoni"].is_string(), "{body}");
    let utmaning = &json["utmaning"]["publicKey"];
    assert_eq!(utmaning["rp"]["id"], "bokforing.example.se", "{body}");
    assert_eq!(utmaning["user"]["name"], "anna@example.se", "{body}");
    assert_eq!(
        utmaning["authenticatorSelection"]["userVerification"],
        "required"
    );
}

#[tokio::test]
async fn webauthn_shimmen_serveras_utan_bygge() {
    let db = Databas::tillfällig().await;

    let (response, body) = skicka(
        app(&db),
        get("/auth/webauthn.js").body(Body::empty()).unwrap(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert!(body.contains("navigator.credentials"), "{body}");
}
