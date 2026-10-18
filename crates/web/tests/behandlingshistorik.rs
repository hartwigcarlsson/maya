//! Inställningar → Behandlingshistorik genom routern: urvalet i adressen, och export av samma
//! urval som CSV och PDF.

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
use maya_core::{Databas, Klocka, TillfälligDatabas, kontoplan};
use maya_domain::kontoplan::Kontonummer;
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
    format!("{bas}/installningar/behandlingshistorik")
}

#[tokio::test]
async fn sidan_visar_posterna_med_tid_aktör_typ_objekt_och_ändring() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;

    let body = text(&db, &session, &sida(&bas)).await;

    for väntat in [
        "<h1>Behandlingshistorik</h1>",
        "företag skapat",
        "556036-0793",
        "konto namnbytt",
        "Namn ändrat från ”Företagskonto” till ”Bankkonto SEB”",
        "anna@example.se",
        "webb",
        "2 poster",
        // Filtren med det som finns i historiken.
        r#"<option value="konto namnbytt">"#,
        r#"name="fran""#,
        r#"name="till""#,
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
async fn urvalet_i_adressen_filtrerar_sidan_och_följer_med_till_exporten() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;

    let body = text(&db, &session, &format!("{}?typ=konto+namnbytt", sida(&bas))).await;

    assert!(body.contains("Bankkonto SEB"), "{body}");
    assert!(!body.contains("Exempel AB upplagt"), "{body}");
    assert!(body.contains("1 post"), "{body}");
    assert!(
        body.contains(r#"<option value="konto namnbytt" selected"#),
        "{body}"
    );
    let csv = format!("{}/csv?typ=konto+namnbytt", sida(&bas));
    let pdf = format!("{}/pdf?typ=konto+namnbytt", sida(&bas));
    assert!(body.contains(&format!(r#"href="{csv}""#)), "{body}");
    assert!(body.contains(&format!(r#"href="{pdf}""#)), "{body}");

    // Ett urval utan träffar.
    let tomt = text(&db, &session, &format!("{}?till=2000-01-01", sida(&bas))).await;
    assert!(tomt.contains("Inga poster i urvalet."), "{tomt}");
}

#[tokio::test]
async fn csv_exporten_har_bom_semikolon_och_bara_urvalet() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;

    let (response, body) = hämta(
        &db,
        &session,
        &format!("{}/csv?typ=konto+namnbytt", sida(&bas)),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "text/csv; charset=utf-8"
    );
    let disposition = response.headers()[header::CONTENT_DISPOSITION]
        .to_str()
        .unwrap();
    assert!(
        disposition.starts_with(r#"attachment; filename="behandlingshistorik_5560360793_"#),
        "{disposition}"
    );
    assert!(
        body.starts_with(
            "\u{feff}Tidpunkt;Aktör;Kanal;Typ;Objekt;Belopp;Beskrivning\r\n".as_bytes()
        )
    );
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert_eq!(text.lines().count(), 2, "{text}");
    assert!(
        text.contains(";anna@example.se;webb;konto namnbytt;1930;;"),
        "{text}"
    );
}

#[tokio::test]
async fn pdf_exporten_är_pdfa_2b_med_urvalet() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;

    let (response, body) = hämta(
        &db,
        &session,
        &format!("{}/pdf?typ=konto+namnbytt", sida(&bas)),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/pdf");
    är_pdfa_2b(&body);
    let text = sidor(&body).concat();
    assert!(
        innehåller(&text, "Urval: typ konto namnbytt · 1 post"),
        "{text}"
    );
    assert!(innehåller(&text, "Bankkonto SEB"), "{text}");
}

#[tokio::test]
async fn ett_annat_företag_ger_403_och_utan_session_skickas_man_till_start() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;

    for path in [format!("{}/csv", sida(&bas)), format!("{}/pdf", sida(&bas))] {
        let annat = path.replace(&bas, "/foretag/4711");
        let (response, _) = hämta(&db, &session, &annat).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{annat}");
        let (response, _) = hämta(&db, "ingen", &path).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER, "{path}");
    }
    let (_, body) = hämta(&db, &session, &sida("/foretag/4711")).await;
    let body = String::from_utf8_lossy(&body);
    assert!(!body.contains("konto namnbytt"), "{body}");
    assert!(body.contains("inte åtkomst"), "{body}");
}
