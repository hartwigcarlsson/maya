//! Passkeys och sessioner under användarmenyn, genom routern. Färsk passkey prövas mot en
//! fast klocka i `crates/core/tests/it/passkeys.rs`; här är sessionen nyss inloggad och
//! färsk.

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

fn get(path: &str, session: &str) -> Request<Body> {
    Request::get(path)
        .header(header::HOST, HOST)
        .header(header::COOKIE, format!("maya_session={session}"))
        .body(Body::empty())
        .unwrap()
}

fn post_form(path: &str, session: &str, form: &str) -> Request<Body> {
    Request::post(path)
        .header(header::HOST, HOST)
        .header(header::ORIGIN, ORIGIN)
        .header(header::COOKIE, format!("maya_session={session}"))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(form.to_string()))
        .unwrap()
}

fn passkey(id: &[u8], namn: &str) -> NyPasskey {
    NyPasskey {
        credential_id: id.to_vec(),
        namn: namn.into(),
        passkey: "{}".into(),
    }
}

/// Anna har registrerat passkeyn "MacBook" i Firefox. Ger sessionstoken.
async fn anna(db: &Databas) -> String {
    let länk = inloggning::skapa_första_användare(
        db,
        &Klocka::system(),
        &Aktör::cli(),
        "anna@example.se",
    )
    .await
    .unwrap();
    inloggning::slutför_registrering(
        db,
        &Klocka::system(),
        länk.token(),
        passkey(b"ett", "MacBook"),
        Some("Firefox"),
    )
    .await
    .unwrap()
    .token()
    .to_string()
}

async fn passkeys(db: &Databas, session: &str) -> Vec<(String, String)> {
    let inloggad = inloggning::session(db, &Klocka::system(), session)
        .await
        .unwrap()
        .unwrap();
    inloggning::passkeys(db, &inloggad)
        .await
        .unwrap()
        .into_iter()
        .map(|p| (p.id, p.namn))
        .collect()
}

#[tokio::test]
async fn anvandarmenyn_har_passkeys_och_sessioner() {
    let db = Databas::tillfällig().await;
    let session = anna(&db).await;

    let (_, body) = skicka(app(&db), get("/", &session)).await;

    assert!(body.contains(r#"href="/passkeys""#), "{body}");
    assert!(body.contains(r#"href="/sessioner""#), "{body}");
}

#[tokio::test]
async fn användarmenyn_fälls_ut_under_e_postadressen_så_att_toppmenyn_får_plats() {
    let db = Databas::tillfällig().await;
    let session = anna(&db).await;

    let (_, body) = skicka(app(&db), get("/", &session)).await;

    // Allt i användarmenyn ligger i en meny som fälls ut, inte på rad i toppmenyn.
    let (_, meny) = body
        .split_once(r#"<details class="anvandarmeny""#)
        .unwrap_or_else(|| panic!("ingen användarmeny i {body}"));
    let (meny, _) = meny.split_once("</details>").unwrap();
    let (summary, innehåll) = meny.split_once("</summary>").unwrap();
    assert!(summary.contains("anna@example.se"), "{summary}");
    for del in [
        r#"href="/passkeys""#,
        r#"href="/sessioner""#,
        r#"href="/installation""#,
        r#"action="/auth/utloggning""#,
        "Ljust/mörkt",
    ] {
        assert!(innehåll.contains(del), "{del} saknas i {innehåll}");
    }
}

#[tokio::test]
async fn passkeysidan_listar_passkeys_och_later_en_farsk_session_hantera_dem() {
    let db = Databas::tillfällig().await;
    let session = anna(&db).await;

    let (response, body) = skicka(app(&db), get("/passkeys", &session)).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert!(body.contains("MacBook"), "{body}");
    assert!(body.contains(r#"data-passkey="lagg-till""#), "{body}");
    assert!(body.contains(r#"action="/konto/passkeys/namn""#), "{body}");
    assert!(
        body.contains(r#"action="/konto/passkeys/ta-bort""#),
        "{body}"
    );
}

#[tokio::test]
async fn efter_registreringen_ber_maya_om_en_passkey_till() {
    let db = Databas::tillfällig().await;
    let länk = inloggning::skapa_första_användare(
        &db,
        &Klocka::system(),
        &Aktör::cli(),
        "anna@example.se",
    )
    .await
    .unwrap();

    let (_, registrering) = skicka(
        app(&db),
        Request::get(format!("/registrera/{}", länk.token()))
            .header(header::HOST, HOST)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert!(
        registrering.contains(r#"data-efter="/passkeys?ny=1""#),
        "{registrering}"
    );

    let session = anna_med_länk(&db, länk.token()).await;
    let (_, body) = skicka(app(&db), get("/passkeys?ny=1", &session)).await;
    assert!(body.contains("Lägg till en passkey till"), "{body}");
}

async fn anna_med_länk(db: &Databas, token: &str) -> String {
    inloggning::slutför_registrering(
        db,
        &Klocka::system(),
        token,
        passkey(b"ett", "MacBook"),
        None,
    )
    .await
    .unwrap()
    .token()
    .to_string()
}

#[tokio::test]
async fn en_passkey_byter_namn_och_tas_bort_men_inte_den_sista() {
    let db = Databas::tillfällig().await;
    let session = anna(&db).await;
    let inloggad = inloggning::session(&db, &Klocka::system(), &session)
        .await
        .unwrap()
        .unwrap();
    inloggning::lägg_till_passkey(&db, &Klocka::system(), &inloggad, passkey(b"tva", "iPhone"))
        .await
        .unwrap();
    let [(macbook, _), (iphone, _)] = passkeys(&db, &session).await.try_into().unwrap();

    let (response, _) = skicka(
        app(&db),
        post_form(
            "/konto/passkeys/namn",
            &session,
            &format!("id={iphone}&namn=Annas+iPhone"),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers()[header::LOCATION], "/passkeys");

    let (response, _) = skicka(
        app(&db),
        post_form(
            "/konto/passkeys/ta-bort",
            &session,
            &format!("id={macbook}"),
        ),
    )
    .await;
    assert_eq!(response.headers()[header::LOCATION], "/passkeys");
    assert_eq!(
        passkeys(&db, &session).await,
        [(iphone.clone(), "Annas iPhone".to_string())]
    );

    let (response, _) = skicka(
        app(&db),
        post_form("/konto/passkeys/ta-bort", &session, &format!("id={iphone}")),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers()[header::LOCATION], "/passkeys?fel=sista");
    assert_eq!(passkeys(&db, &session).await.len(), 1);
    let (_, body) = skicka(app(&db), get("/passkeys?fel=sista", &session)).await;
    assert!(
        body.contains("Den sista passkeyn går inte att ta bort"),
        "{body}"
    );
}

#[tokio::test]
async fn en_inloggad_far_en_webauthn_utmaning_for_en_ny_passkey() {
    let db = Databas::tillfällig().await;
    let session = anna(&db).await;

    let (response, body) = skicka(
        app(&db),
        Request::post("/auth/passkey/start")
            .header(header::HOST, HOST)
            .header(header::ORIGIN, ORIGIN)
            .header(header::COOKIE, format!("maya_session={session}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from("{}"))
            .unwrap(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK, "{body}");
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        json["utmaning"]["publicKey"]["user"]["name"],
        "anna@example.se"
    );
    let exclude = &json["utmaning"]["publicKey"]["excludeCredentials"];
    assert_eq!(exclude.as_array().map(Vec::len), Some(1), "{body}");
}

#[tokio::test]
async fn utan_session_finns_ingen_ny_passkey_och_ingen_passkeysida() {
    let db = Databas::tillfällig().await;
    anna(&db).await;

    let (response, _) = skicka(
        app(&db),
        Request::post("/auth/passkey/start")
            .header(header::HOST, HOST)
            .header(header::ORIGIN, ORIGIN)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from("{}"))
            .unwrap(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let (_, body) = skicka(app(&db), get("/passkeys", "okand")).await;
    assert!(body.contains(r#"type="email""#), "inloggningen: {body}");
    assert!(!body.contains("MacBook"), "{body}");
}

#[tokio::test]
async fn sessionslistan_visar_sessionerna_och_en_avslutad_session_loggas_ut_direkt() {
    let db = Databas::tillfällig().await;
    let session = anna(&db).await;
    let inloggad = inloggning::session(&db, &Klocka::system(), &session)
        .await
        .unwrap()
        .unwrap();
    let annan = inloggning::logga_in(
        &db,
        &Klocka::system(),
        inloggad.användare.id,
        b"ett",
        None,
        Some("Safari på iPhone"),
    )
    .await
    .unwrap();

    let (_, body) = skicka(app(&db), get("/sessioner", &session)).await;
    assert!(body.contains("Firefox"), "{body}");
    assert!(body.contains("Safari på iPhone"), "{body}");
    assert!(body.contains("Den här sessionen"), "{body}");

    let andra = inloggning::sessioner(&db, &Klocka::system(), &inloggad)
        .await
        .unwrap()
        .into_iter()
        .find(|s| !s.denna)
        .unwrap();
    let (response, _) = skicka(
        app(&db),
        post_form(
            "/konto/sessioner/avsluta",
            &session,
            &format!("id={}", andra.id),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers()[header::LOCATION], "/sessioner");

    let (_, body) = skicka(app(&db), get("/", annan.token())).await;
    assert!(body.contains(r#"type="email""#), "utloggad: {body}");
}

#[tokio::test]
async fn att_avsluta_den_egna_sessionen_loggar_ut() {
    let db = Databas::tillfällig().await;
    let session = anna(&db).await;
    let inloggad = inloggning::session(&db, &Klocka::system(), &session)
        .await
        .unwrap()
        .unwrap();

    let (response, _) = skicka(
        app(&db),
        post_form(
            "/konto/sessioner/avsluta",
            &session,
            &format!("id={}", inloggad.session_id()),
        ),
    )
    .await;

    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers()[header::LOCATION], "/");
    let cookie = response.headers()[header::SET_COOKIE].to_str().unwrap();
    assert!(cookie.contains("Max-Age=0"), "{cookie}");
}
