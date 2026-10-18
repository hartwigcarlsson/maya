//! Sökningen med ⌘K och sökfältet, genom routern.

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

async fn skicka(db: &TillfälligDatabas, request: Request<Body>) -> (Response<()>, String) {
    let response = app(db).oneshot(request).await.unwrap();
    let (parts, body) = response.into_parts();
    let body = body.collect().await.unwrap().to_bytes();
    (
        Response::from_parts(parts, ()),
        String::from_utf8_lossy(&body).into_owned(),
    )
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

async fn posta(db: &TillfälligDatabas, session: &str, path: &str, body: &str) -> String {
    let (response, _) = skicka(
        db,
        Request::post(path)
            .header(header::HOST, HOST)
            .header(header::ORIGIN, ORIGIN)
            .header(header::COOKIE, format!("maya_session={session}"))
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(body.to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    response.headers()[header::LOCATION]
        .to_str()
        .unwrap()
        .to_string()
}

async fn lägg_upp(db: &TillfälligDatabas, session: &str, orgnr: &str, namn: &str) -> String {
    posta(
        db,
        session,
        "/foretag",
        &format!(
            "orgnr={orgnr}&namn={namn}&start=2026-01-01&slut=2026-12-31\
             &bokforingsmetod=faktura&momsperiod=kvartal"
        ),
    )
    .await
}

async fn bokför(db: &TillfälligDatabas, session: &str, bas: &str, beskrivning: &str) -> String {
    posta(
        db,
        session,
        &format!("{bas}/verifikat"),
        &format!(
            "datum=2026-03-14&beskrivning={beskrivning}\
             &konto_0=1930&debet_0=1+250&kredit_0=&text_0=\
             &konto_1=3001&debet_1=&kredit_1=1250&text_1="
        ),
    )
    .await
}

/// Anna med Exempel AB (verifikatet "Kaffe") och Annat AB (verifikatet "Hyra"); sökvägarna
/// till företagen och till Exempels verifikat.
async fn två_företag(db: &TillfälligDatabas) -> (String, String, String, String) {
    let session = inloggad(db).await;
    let exempel = lägg_upp(db, &session, "556036-0793", "Exempel+AB").await;
    let annat = lägg_upp(db, &session, "559900-0006", "Annat+AB").await;
    let kaffe = bokför(db, &session, &exempel, "Kaffe").await;
    bokför(db, &session, &annat, "Hyra").await;
    (session, exempel, annat, kaffe)
}

#[tokio::test]
async fn sökfältet_i_toppmenyn_leder_till_sökningen_och_visar_kortkommandot() {
    let db = Databas::tillfällig().await;
    let (session, exempel, _, _) = två_företag(&db).await;

    let (_, body) = hämta(&db, &session, &exempel).await;

    let meny = &body[body.find(r#"class="toppmeny""#).unwrap()..body.find("</header>").unwrap()];
    assert!(
        meny.contains(&format!(r#"action="{exempel}/sok""#)),
        "{meny}"
    );
    assert!(meny.contains(r#"name="q""#), "{meny}");
    assert!(meny.contains("⌘K"), "{meny}");
}

#[tokio::test]
async fn sökningen_hittar_verifikat_i_det_valda_företaget_och_länkar_till_det() {
    let db = Databas::tillfällig().await;
    let (session, exempel, annat, kaffe) = två_företag(&db).await;

    let (response, body) = hämta(&db, &session, &format!("{exempel}/sok?q=V1")).await;

    assert_eq!(response.status(), StatusCode::OK);
    let träffar = &body[body.find(r#"class="sokning""#).expect(&body)..];
    assert!(träffar.contains(&format!(r#"href="{kaffe}""#)), "{träffar}");
    assert!(träffar.contains("Kaffe"), "{träffar}");
    assert!(träffar.contains("1 250,00"), "{träffar}");
    assert!(!träffar.contains("Hyra"), "{träffar}");

    let (_, body) = hämta(&db, &session, &format!("{annat}/sok?q=kaffe")).await;
    let träffar = &body[body.find(r#"class="sokning""#).unwrap()..];
    assert!(!träffar.contains("Kaffe"), "{träffar}");
}

#[tokio::test]
async fn sidor_och_handlingar_nås_från_sökningen() {
    let db = Databas::tillfällig().await;
    let (session, exempel, _, _) = två_företag(&db).await;

    let (_, body) = hämta(&db, &session, &format!("{exempel}/sok")).await;
    let träffar = &body[body.find(r#"class="sokning""#).unwrap()..];
    for (href, text) in [
        (format!("{exempel}/verifikat/nytt"), "Nytt verifikat"),
        (exempel.clone(), "Att göra"),
        (format!("{exempel}/verifikat"), "Verifikat"),
        (format!("{exempel}/rapporter"), "Rapporter"),
        (format!("{exempel}/installningar"), "Inställningar"),
        (format!("{exempel}/installningar/kontoplan"), "Kontoplan"),
        ("/foretag/nytt".to_string(), "Lägg upp företag"),
    ] {
        assert!(
            träffar.contains(&format!(r#"href="{href}""#)),
            "{href}: {träffar}"
        );
        assert!(träffar.contains(text), "{text}: {träffar}");
    }

    let (_, body) = hämta(&db, &session, &format!("{exempel}/sok?q=nytt+ver")).await;
    let träffar = &body[body.find(r#"class="sokning""#).unwrap()..];
    assert!(träffar.contains("Nytt verifikat"), "{träffar}");
    assert!(!träffar.contains("Rapporter"), "{träffar}");
}

#[tokio::test]
async fn företagsbyte_görs_från_sökningen() {
    let db = Databas::tillfällig().await;
    let (session, exempel, annat, _) = två_företag(&db).await;

    let (_, body) = hämta(&db, &session, &format!("{exempel}/sok?q=annat")).await;

    let träffar = &body[body.find(r#"class="sokning""#).unwrap()..];
    assert!(träffar.contains("Byt företag"), "{träffar}");
    assert!(träffar.contains(&format!(r#"href="{annat}""#)), "{träffar}");
    assert!(träffar.contains("Annat AB"), "{träffar}");
}

#[tokio::test]
async fn sökningen_i_ett_företag_utan_åtkomst_nekas() {
    let db = Databas::tillfällig().await;
    let (session, _, _, _) = två_företag(&db).await;

    let (response, body) = hämta(&db, &session, "/foretag/4711/sok?q=kaffe").await;

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(!body.contains("Kaffe"), "{body}");
}

#[tokio::test]
async fn paletten_hämtar_verifikaten_från_samma_sökning() {
    let db = Databas::tillfällig().await;
    let (session, exempel, _, kaffe) = två_företag(&db).await;
    let id = exempel.trim_start_matches("/foretag/");

    let (response, body) = hämta(&db, &session, &format!("/api/sok?foretag={id}&q=kaffe")).await;

    assert_eq!(response.status(), StatusCode::OK, "{body}");
    let träffar: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(träffar.as_array().unwrap().len(), 1, "{body}");
    assert_eq!(träffar[0]["href"], kaffe.as_str(), "{body}");

    let (response, body) = hämta(&db, &session, "/api/sok?foretag=4711&q=kaffe").await;
    assert!(!response.status().is_success(), "{body}");
    assert!(!body.contains("Kaffe"), "{body}");
}
