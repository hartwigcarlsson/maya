//! SIE4-exporten under Rapporter, genom routern.

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

async fn skicka(db: &TillfälligDatabas, request: Request<Body>) -> (Response<()>, Vec<u8>) {
    let response = app(db).oneshot(request).await.unwrap();
    let (parts, body) = response.into_parts();
    let body = body.collect().await.unwrap().to_bytes();
    (Response::from_parts(parts, ()), body.to_vec())
}

/// En session som om användaren hade registrerat en passkey.
async fn inloggad(db: &Databas) -> String {
    let länk = inloggning::skapa_första_användare(
        db,
        &Klocka::system(),
        &Aktör::cli(),
        "anna@example.se",
    )
    .await
    .unwrap();
    let ny = NyPasskey {
        credential_id: b"ett".to_vec(),
        namn: "MacBook".into(),
        passkey: "{}".into(),
    };
    inloggning::slutför_registrering(db, &Klocka::system(), länk.token(), ny, None)
        .await
        .unwrap()
        .token()
        .to_string()
}

async fn hämta(db: &TillfälligDatabas, session: &str, path: &str) -> (Response<()>, Vec<u8>) {
    skicka(
        db,
        Request::get(path)
            .header(header::HOST, HOST)
            .header(header::COOKIE, format!("maya_session={session}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await
}

async fn posta(db: &TillfälligDatabas, session: &str, path: &str, body: &str) -> Response<()> {
    skicka(
        db,
        Request::post(path)
            .header(header::HOST, HOST)
            .header(header::ORIGIN, ORIGIN)
            .header(header::COOKIE, format!("maya_session={session}"))
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(body.to_string()))
            .unwrap(),
    )
    .await
    .0
}

fn plats(response: &Response<()>) -> String {
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    response.headers()[header::LOCATION]
        .to_str()
        .unwrap()
        .to_string()
}

/// Ett inloggat företag med kalenderåret 2026 och ett bokfört verifikat; sökvägen till
/// företaget.
async fn företag(db: &TillfälligDatabas) -> (String, String) {
    let session = inloggad(db).await;
    let response = posta(
        db,
        &session,
        "/foretag",
        "orgnr=556036-0793&namn=Exempel+AB&start=2026-01-01&slut=2026-12-31\
         &bokforingsmetod=faktura&momsperiod=kvartal",
    )
    .await;
    let bas = plats(&response);
    let response = posta(
        db,
        &session,
        &format!("{bas}/verifikat"),
        "handling=bokfor&utkast=&datum=2026-03-14&beskrivning=F%C3%B6rs%C3%A4ljning\
         &konto_0=1930&debet_0=125&kredit_0=&text_0=\
         &konto_1=3001&debet_1=&kredit_1=125&text_1=",
    )
    .await;
    plats(&response);
    (session, bas)
}

#[tokio::test]
async fn rapporter_har_en_sie4_länk_per_räkenskapsår() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;

    let (response, body) = hämta(&db, &session, &format!("{bas}/rapporter")).await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = String::from_utf8(body).unwrap();
    assert!(body.contains("SIE4"), "{body}");
    assert!(
        body.contains(&format!(r#"href="{bas}/sie4?ar=2026-01-01""#)),
        "{body}"
    );
    assert!(body.contains("2026-01-01 – 2026-12-31"), "{body}");
}

#[tokio::test]
async fn exporten_laddas_ned_som_en_sie4_fil() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;

    let (response, body) = hämta(&db, &session, &format!("{bas}/sie4?ar=2026-01-01")).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_DISPOSITION],
        r#"attachment; filename="SIE4_5560360793_2026.se""#
    );
    let fil = maya_sie::läs(&body).unwrap();
    assert_eq!(fil.företagsnamn, "Exempel AB");
    assert_eq!(fil.verifikat.len(), 1);
    assert_eq!(fil.verifikat[0].text, "Försäljning");
}

#[tokio::test]
async fn den_utan_åtkomst_får_ingen_fil() {
    let db = Databas::tillfällig().await;
    let bas = "/foretag/1";
    let utan_session = skicka(
        &db,
        Request::get(format!("{bas}/sie4?ar=2026-01-01"))
            .header(header::HOST, HOST)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(utan_session.0.status(), StatusCode::FORBIDDEN);

    // Ett företag som Anna inte har åtkomst till (det finns inte).
    let (session, _) = företag(&db).await;
    let (response, _) = hämta(&db, &session, "/foretag/9999/sie4?ar=2026-01-01").await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn ett_okänt_år_ger_404() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;

    let (response, _) = hämta(&db, &session, &format!("{bas}/sie4?ar=2025-01-01")).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn en_stoppad_export_visas_på_rapportsidan() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;

    let (_, body) = hämta(&db, &session, &format!("{bas}/rapporter?fel=sie4")).await;
    let body = String::from_utf8(body).unwrap();
    assert!(body.contains("stämmer inte med bokföringen"), "{body}");
}

#[tokio::test]
async fn sökningen_på_sie4_hittar_rapporter() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;

    let (_, body) = hämta(&db, &session, &format!("{bas}/sok?q=sie4")).await;
    let body = String::from_utf8(body).unwrap();
    let träffar = &body[body.find(r#"class="sokning""#).unwrap()..];
    assert!(
        träffar.contains(&format!(r#"href="{bas}/rapporter""#)),
        "{träffar}"
    );
}
