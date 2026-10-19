//! Inställningar → Systemdokumentation genom routern: sidan, PDF:en och att varje sida och fil
//! som dokumentationen hänvisar till finns.

use std::sync::Arc;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use http_body_util::BodyExt;
use maya_core::aktor::Aktör;
use maya_core::config::Config;
use maya_core::foretag::FöretagsId;
use maya_core::inloggning::{self, NyPasskey};
use maya_core::{Databas, Klocka, TillfälligDatabas, kontoplan, systemdokumentation};
use maya_domain::kontoplan::{Kontonummer, ink2};
use maya_domain::systemdokumentation::Block;
use maya_pdf::testhjalp::{innehåller, sidor, är_pdfa_2b};
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

async fn skicka(db: &TillfälligDatabas, request: Request<Body>) -> (Response<()>, Bytes) {
    let response = app(db).oneshot(request).await.unwrap();
    let (parts, body) = response.into_parts();
    let body = body.collect().await.unwrap().to_bytes();
    (Response::from_parts(parts, ()), body)
}

async fn användare(db: &Databas, epost: &str, credential: &[u8]) -> String {
    let länk = inloggning::skapa_första_användare(db, &Klocka::system(), &Aktör::cli(), epost)
        .await
        .unwrap();
    let ny = NyPasskey {
        credential_id: credential.to_vec(),
        namn: "MacBook".into(),
        passkey: "{}".into(),
    };
    inloggning::slutför_registrering(db, &Klocka::system(), länk.token(), ny, None)
        .await
        .unwrap()
        .token()
        .to_string()
}

async fn hämta(db: &TillfälligDatabas, session: &str, path: &str) -> (Response<()>, Bytes) {
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

async fn text(db: &TillfälligDatabas, session: &str, path: &str) -> String {
    let (response, body) = hämta(db, session, path).await;
    assert_eq!(response.status(), StatusCode::OK, "{path}");
    String::from_utf8(body.to_vec()).unwrap()
}

/// Anna med Exempel AB, där 1930 har bytt namn; sökvägen till företaget.
async fn företag(db: &TillfälligDatabas) -> (String, String) {
    let session = användare(db, "anna@example.se", b"ett").await;
    let (response, _) = skicka(
        db,
        Request::post("/foretag")
            .header(header::HOST, HOST)
            .header(header::ORIGIN, ORIGIN)
            .header(header::COOKIE, format!("maya_session={session}"))
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(
                "orgnr=556036-0793&namn=Exempel+AB&start=2026-01-01&slut=2026-12-31\
                 &bokforingsmetod=faktura&momsperiod=kvartal",
            ))
            .unwrap(),
    )
    .await;
    let bas = response.headers()[header::LOCATION]
        .to_str()
        .unwrap()
        .to_string();
    let id: FöretagsId = bas.trim_start_matches("/foretag/").parse().unwrap();
    let anna = inloggning::session(db, &Klocka::system(), &session)
        .await
        .unwrap()
        .unwrap()
        .aktör;
    kontoplan::byt_namn(
        db,
        &Klocka::system(),
        &anna,
        id,
        Kontonummer::new(1930).unwrap(),
        "Bankkonto SEB",
    )
    .await
    .unwrap();
    (session, bas)
}

fn sida(bas: &str) -> String {
    format!("{bas}/installningar/systemdokumentation")
}

#[tokio::test]
async fn sidan_visar_avsnitten_versionen_och_förvaringen_i_sverige() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;

    let body = text(&db, &session, &sida(&bas)).await;

    for väntat in [
        "<h1>Systemdokumentation</h1>",
        "556036-0793",
        r#"id="kontoplan""#,
        "Sammanställning till årsredovisningen",
        "Samlingsplan och serier",
        "Behandlingsregler",
        "Verifieringskedja och informationsflöden",
        "Överföringar",
        "Arkivplan",
        "Version och ändringslogg",
        env!("CARGO_PKG_VERSION"),
        ink2::UTGÅVA,
        "BAS 2026",
        "Bankkonto SEB",
        "<strong>Rättelse</strong>",
        "Första versionen av behandlingsreglerna.",
        // #155 Fakturaflödena
        "<strong>T2-samlingsverifikat</strong>",
        "Intäktskonto per momssats",
        "Fakturaserien",
        "Reskontrorna som delsystem",
        "Avstämningen mot 1510 och 2440",
        "ej angivet",
        "förvaras i Sverige",
        // Skriv ut och Exportera ger samma PDF.
        r#"class="skriv-ut""#,
        &format!(r#"href="{}/pdf""#, sida(&bas)),
    ] {
        assert!(body.contains(väntat), "{väntat}: {body}");
    }
    // Menyn på inställningssidorna leder hit.
    let inställningar = text(&db, &session, &format!("{bas}/installningar")).await;
    assert!(
        inställningar.contains(&format!(r#"href="{}""#, sida(&bas))),
        "{inställningar}"
    );
}

#[tokio::test]
async fn pdf_en_är_pdfa_2b_med_samma_avsnitt() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;

    let (response, body) = hämta(&db, &session, &format!("{}/pdf", sida(&bas))).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/pdf");
    är_pdfa_2b(&body);
    let text = sidor(&body).concat();
    for väntat in [
        "Systemdokumentation",
        "Bankkonto SEB",
        "Arkivplan",
        "ej angivet",
    ] {
        assert!(innehåller(&text, väntat), "{väntat}");
    }
}

#[tokio::test]
async fn varje_sida_och_fil_som_dokumentationen_hänvisar_till_finns() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;
    let anna = inloggning::session(&db, &Klocka::system(), &session)
        .await
        .unwrap()
        .unwrap()
        .aktör;
    let id: FöretagsId = bas.trim_start_matches("/foretag/").parse().unwrap();
    let config = Config::from_env(
        [("MAYA_PUBLIC_URL", ORIGIN), ("MAYA_BACKUP_DIR", "/backup")]
            .map(|(k, v)| (k.to_string(), v.to_string())),
    )
    .unwrap();
    let dok = systemdokumentation::systemdokumentation(&db, &config, &anna, id)
        .await
        .unwrap();

    let länkar: Vec<String> = dok
        .avsnitt
        .iter()
        .flat_map(|a| &a.block)
        .filter_map(|b| match b {
            Block::Länk { href, .. } => Some(href.clone()),
            _ => None,
        })
        .collect();
    assert!(länkar.len() >= 10, "{länkar:?}");
    for länk in &länkar {
        assert!(länk.starts_with(&bas), "{länk}");
        let (response, body) = hämta(&db, &session, länk).await;
        assert_eq!(response.status(), StatusCode::OK, "{länk}");
        assert!(
            !String::from_utf8_lossy(&body).contains("Sidan finns inte"),
            "{länk}"
        );
    }
}

#[tokio::test]
async fn ett_annat_företag_ger_403_och_utan_session_skickas_man_till_start() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;

    let pdf = format!("{}/pdf", sida(&bas));
    let annat = pdf.replace(&bas, "/foretag/4711");
    let (response, _) = hämta(&db, &session, &annat).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let (response, _) = hämta(&db, "ingen", &pdf).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let (_, body) = hämta(&db, &session, &sida("/foretag/4711")).await;
    let body = String::from_utf8_lossy(&body);
    assert!(!body.contains("Behandlingsregler"), "{body}");
    assert!(body.contains("inte åtkomst"), "{body}");
}
