//! Inställningar → Åtkomst genom routern: listan, inbjudningslänken, åtkomst given och tagen,
//! och kravet på färsk passkey.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use http_body_util::BodyExt;
use jiff::{SignedDuration, Timestamp};
use maya_core::config::Config;
use maya_core::inloggning;
use maya_core::{Databas, Klocka, TillfälligDatabas, testhjalp};
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

async fn skicka(db: &TillfälligDatabas, request: Request<Body>) -> (Response<()>, String) {
    let response = app(db).oneshot(request).await.unwrap();
    let (parts, body) = response.into_parts();
    let body = body.collect().await.unwrap().to_bytes();
    (
        Response::from_parts(parts, ()),
        String::from_utf8_lossy(&body).into_owned(),
    )
}

async fn hämta(db: &TillfälligDatabas, session: &str, path: &str) -> (Response<()>, String) {
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

/// Anna med Exempel AB: en session där passkeyn är färsk, en där den inte är det, och
/// sökvägen till företaget.
async fn anna(db: &Databas) -> (String, String, String) {
    let (anna, företag) = testhjalp::företag(db).await;
    let färsk = testhjalp::session(db, &Klocka::system(), &anna).await;
    let för_länge_sedan = Klocka::fast(Timestamp::now() - SignedDuration::from_mins(10));
    let gammal = testhjalp::session(db, &för_länge_sedan, &anna).await;
    (färsk, gammal, format!("/foretag/{företag}"))
}

#[tokio::test]
async fn sidan_listar_användarna_och_når_från_inställningsmenyn() {
    let db = Databas::tillfällig().await;
    let (session, _, bas) = anna(&db).await;

    let (response, body) = hämta(&db, &session, &format!("{bas}/installningar/atkomst")).await;

    assert_eq!(response.status(), StatusCode::OK, "{body}");
    assert!(body.contains("anna@example.se"), "{body}");
    assert!(body.contains(r#"name="epost""#), "{body}");
    let (_, inställningar) = hämta(&db, &session, &format!("{bas}/installningar")).await;
    assert!(
        inställningar.contains(&format!(r#"href="{bas}/installningar/atkomst""#)),
        "{inställningar}"
    );
}

#[tokio::test]
async fn en_inbjudan_visar_länken_för_kopiering_och_listas_som_öppen() {
    let db = Databas::tillfällig().await;
    let (session, _, bas) = anna(&db).await;

    let response = posta(
        &db,
        &session,
        &format!("{bas}/atkomst/inbjudan"),
        "epost=bo%40example.se",
    )
    .await;

    let plats = plats(&response);
    let token = plats
        .strip_prefix(&format!("{bas}/installningar/atkomst?lank="))
        .expect(&plats)
        .to_string();
    let (_, body) = hämta(&db, &session, &plats).await;
    assert!(
        body.contains(&format!("{ORIGIN}/registrera/{token}")),
        "{body}"
    );
    assert!(body.contains("bo@example.se"), "{body}");
    let registrering = inloggning::registrering(&db, &Klocka::system(), &token)
        .await
        .unwrap();
    assert_eq!(registrering.användare.epost, "bo@example.se");
}

#[tokio::test]
async fn en_inbjudan_till_en_befintlig_användare_ger_åtkomst_och_den_kan_tas() {
    let db = Databas::tillfällig().await;
    let (session, _, bas) = anna(&db).await;
    let bo = testhjalp::användare(&db, "bo@example.se").await;

    let response = posta(
        &db,
        &session,
        &format!("{bas}/atkomst/inbjudan"),
        "epost=bo%40example.se",
    )
    .await;
    assert_eq!(
        plats(&response),
        format!("{bas}/installningar/atkomst?meddelande=atkomst_given")
    );
    let (_, body) = hämta(&db, &session, &plats(&response)).await;
    assert!(body.contains("bo@example.se"), "{body}");

    let bo_id = bo.användare.unwrap();
    let response = posta(
        &db,
        &session,
        &format!("{bas}/atkomst/ta"),
        &format!("anvandare={bo_id}"),
    )
    .await;
    assert_eq!(
        plats(&response),
        format!("{bas}/installningar/atkomst?meddelande=atkomst_tagen")
    );
    let bo_session = testhjalp::session(&db, &Klocka::system(), &bo).await;
    let (response, _) = hämta(&db, &bo_session, &format!("{bas}/installningar/atkomst")).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn den_sista_åtkomsten_kan_inte_tas() {
    let db = Databas::tillfällig().await;
    let (session, _, bas) = anna(&db).await;
    let lista = maya_core::atkomst::åtkomst(
        &db,
        &Klocka::system(),
        &inloggning::session(&db, &Klocka::system(), &session)
            .await
            .unwrap()
            .unwrap()
            .aktör,
        bas.trim_start_matches("/foretag/").parse().unwrap(),
    )
    .await
    .unwrap();
    let anna = lista.användare[0].id;

    let response = posta(
        &db,
        &session,
        &format!("{bas}/atkomst/ta"),
        &format!("anvandare={anna}"),
    )
    .await;

    let plats = plats(&response);
    assert_eq!(plats, format!("{bas}/installningar/atkomst?fel=sista"));
    let (_, body) = hämta(&db, &session, &plats).await;
    assert!(body.contains(r#"data-kod="sista""#), "{body}");
}

#[tokio::test]
async fn utan_färsk_passkey_ber_sidan_om_passkeyn_och_ändringar_nekas() {
    let db = Databas::tillfällig().await;
    let (_, gammal, bas) = anna(&db).await;

    let (_, body) = hämta(&db, &gammal, &format!("{bas}/installningar/atkomst")).await;
    assert!(body.contains(r#"data-passkey="bekrafta""#), "{body}");
    assert!(!body.contains(r#"name="epost""#), "{body}");

    let response = posta(
        &db,
        &gammal,
        &format!("{bas}/atkomst/inbjudan"),
        "epost=bo%40example.se",
    )
    .await;
    assert_eq!(
        plats(&response),
        format!("{bas}/installningar/atkomst?fel=farsk")
    );
}

#[tokio::test]
async fn ett_annat_företag_ger_403() {
    let db = Databas::tillfällig().await;
    let (_, _, bas) = anna(&db).await;
    let bo = testhjalp::användare(&db, "bo@example.se").await;
    let bo = testhjalp::session(&db, &Klocka::system(), &bo).await;

    let (response, _) = hämta(&db, &bo, &format!("{bas}/installningar/atkomst")).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = posta(
        &db,
        &bo,
        &format!("{bas}/atkomst/inbjudan"),
        "epost=bo%40example.se",
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}
