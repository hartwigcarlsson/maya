//! Att skriva in den ingående balansen under Inställningar, genom routern.

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

/// Ett inloggat företag med kalenderåret 2026; sökvägen till företaget.
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
    (session, plats(&response))
}

/// Bank 1 000 kr och aktiekapital 1 000 kr, med kontona på rad 0 och 1 i formuläret.
const STARTKAPITAL: &str = "ar=2026-01-01&konto_0=1930&debet_0=1+000&kredit_0=\
    &konto_1=2081&debet_1=&kredit_1=1000&konto_2=1510&debet_2=&kredit_2=";

fn sida(bas: &str) -> String {
    format!("{bas}/installningar/ingaende-balans")
}

#[tokio::test]
async fn formuläret_har_en_rad_per_aktivt_balanskonto_och_inga_resultatkonton() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;

    let (response, body) = hämta(&db, &session, &sida(&bas)).await;

    assert_eq!(response.status(), StatusCode::OK);
    for text in [
        "Ingående balans",
        "2026-01-01 – 2026-12-31",
        r#"value="1930""#,
        r#"value="2081""#,
        r#"value="2099""#,
        r#"name="debet_0""#,
        r#"name="kredit_0""#,
        "Spara",
    ] {
        assert!(body.contains(text), "{text}: {body}");
    }
    assert!(!body.contains(r#"value="3001""#), "{body}");
    assert!(
        body.contains(&format!(r#"action="{bas}/ingaende-balans""#)),
        "{body}"
    );
}

#[tokio::test]
async fn en_sparad_ingående_balans_visas_i_formuläret() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;

    let response = posta(
        &db,
        &session,
        &format!("{bas}/ingaende-balans"),
        STARTKAPITAL,
    )
    .await;

    assert_eq!(plats(&response), format!("{}?ar=2026-01-01", sida(&bas)));
    let (_, body) = hämta(&db, &session, &plats(&response)).await;
    assert!(body.contains(r#"value="1 000,00""#), "{body}");
    assert!(!body.contains(r#"data-kod="#), "{body}");
}

#[tokio::test]
async fn servern_nekar_en_ingående_balans_vars_summa_inte_är_noll_med_samma_kod_som_formuläret() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;
    let fel = STARTKAPITAL.replace("kredit_1=1000", "kredit_1=900");

    let response = posta(&db, &session, &format!("{bas}/ingaende-balans"), &fel).await;

    let plats = plats(&response);
    assert!(plats.starts_with(&format!("{}?", sida(&bas))), "{plats}");
    let (_, body) = hämta(&db, &session, &plats).await;
    assert!(body.contains(r#"data-kod="differens""#), "{body}");
    assert!(body.contains(r#"value="900""#), "{body}");
    assert!(body.contains("1 problem kvar"), "{body}");
    let (_, sparat) = hämta(&db, &session, &sida(&bas)).await;
    assert!(!sparat.contains(r#"value="1 000,00""#), "{sparat}");
}

#[tokio::test]
async fn ett_senare_år_visar_den_preliminära_ingående_balansen_utan_formulär() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;
    posta(
        &db,
        &session,
        &format!("{bas}/ingaende-balans"),
        STARTKAPITAL,
    )
    .await;
    let verifikat = "datum=2027-02-01&beskrivning=F%C3%B6rs%C3%A4ljning\
        &konto_0=1930&debet_0=70&kredit_0=&text_0=&konto_1=3001&debet_1=&kredit_1=70&text_1=";
    plats(&posta(&db, &session, &format!("{bas}/verifikat"), verifikat).await);

    let (response, body) = hämta(&db, &session, &format!("{}?ar=2027-01-01", sida(&bas))).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert!(body.contains("Preliminär"), "{body}");
    assert!(body.contains("1 000,00"), "{body}");
    assert!(!body.contains(r#"name="debet_0""#), "{body}");
    // Året kan väljas.
    assert!(body.contains("?ar=2026-01-01"), "{body}");
}

#[tokio::test]
async fn ett_annat_företags_ingående_balans_nekas() {
    let db = Databas::tillfällig().await;
    let (session, _) = företag(&db).await;

    let (response, _) = hämta(&db, &session, "/foretag/999/installningar/ingaende-balans").await;
    let post = posta(&db, &session, "/foretag/999/ingaende-balans", STARTKAPITAL).await;

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(post.status(), StatusCode::FORBIDDEN);
}
