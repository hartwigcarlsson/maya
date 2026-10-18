//! Skriv ut och exportera som PDF i webben: verifikatet från dokumentvyn och kontoplanen från
//! Inställningar → Kontoplan. Skriv ut och Exportera pekar på samma PDF.

use std::sync::Arc;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use http_body_util::BodyExt;
use maya_core::aktor::Aktör;
use maya_core::config::Config;
use maya_core::inloggning::{self, NyPasskey};
use maya_core::{Databas, Klocka, TillfälligDatabas};
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

/// Ett inloggat företag med ett bokfört verifikat: sessionen, företagets sökväg och
/// verifikatets sida.
async fn med_verifikat(db: &TillfälligDatabas) -> (String, String, String) {
    let session = användare(db, "anna@example.se", b"ett").await;
    let bas = posta(
        db,
        &session,
        "/foretag",
        "orgnr=556036-0793&namn=Exempel+AB&start=2026-01-01&slut=2026-12-31\
         &bokforingsmetod=faktura&momsperiod=kvartal",
    )
    .await;
    let sida = posta(
        db,
        &session,
        &format!("{bas}/verifikat"),
        "datum=2026-03-14&beskrivning=F%C3%B6rs%C3%A4ljning+kontant\
         &konto_0=1930&debet_0=1250&kredit_0=&text_0=Kassan\
         &konto_1=3001&debet_1=&kredit_1=1250&text_1=",
    )
    .await;
    (session, bas, sida)
}

/// `href` för länken med `class="{klass}"` i HTML:en.
fn länk<'a>(html: &'a str, klass: &str) -> &'a str {
    let i = html
        .find(&format!(r#"class="{klass}""#))
        .unwrap_or_else(|| panic!("ingen länk {klass} i {html}"));
    let start = html[..i].rfind("<a").unwrap();
    let tagg = &html[start..start + html[start..].find('>').unwrap()];
    let href = tagg.find(r#"href=""#).unwrap() + 6;
    &tagg[href..href + tagg[href..].find('"').unwrap()]
}

fn är_pdf(response: &Response<()>, filnamn: &str) {
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/pdf");
    assert_eq!(
        response.headers()[header::CONTENT_DISPOSITION],
        format!(r#"inline; filename="{filnamn}""#).as_str()
    );
}

#[tokio::test]
async fn verifikatets_skriv_ut_och_exportera_ger_samma_pdf() {
    let db = Databas::tillfällig().await;
    let (session, _, sida) = med_verifikat(&db).await;
    let (_, html) = hämta(&db, &session, &sida).await;
    let html = String::from_utf8_lossy(&html);

    let skriv_ut = länk(&html, "skriv-ut");
    let exportera = länk(&html, "exportera-pdf");
    assert_eq!(skriv_ut, exportera, "samma PDF");
    assert!(html.contains(r#"download="V1.pdf""#), "{html}");

    let (response, pdf) = hämta(&db, &session, skriv_ut).await;
    är_pdf(&response, "V1.pdf");
    är_pdfa_2b(&pdf);
    let text = sidor(&pdf).concat();
    for väntat in [
        "V1",
        "Försäljning kontant",
        "Kassan",
        "1 250,00",
        "Inga underlag",
        "verifikat bokfört",
        "av anna@example.se",
        "Sida 1 av 1",
    ] {
        assert!(innehåller(&text, väntat), "saknar {väntat:?} i {text}");
    }
}

#[tokio::test]
async fn kontoplanens_skriv_ut_och_exportera_ger_samma_pdf() {
    let db = Databas::tillfällig().await;
    let (session, bas, _) = med_verifikat(&db).await;
    let (_, html) = hämta(&db, &session, &format!("{bas}/installningar/kontoplan")).await;
    let html = String::from_utf8_lossy(&html);

    let skriv_ut = länk(&html, "skriv-ut");
    assert_eq!(skriv_ut, länk(&html, "exportera-pdf"), "samma PDF");

    let (response, pdf) = hämta(&db, &session, skriv_ut).await;
    är_pdf(&response, "kontoplan.pdf");
    är_pdfa_2b(&pdf);
    let text = sidor(&pdf).concat();
    assert!(innehåller(&text, "Kontoplan"));
    assert!(innehåller(&text, "1930 Företagskonto"));
}

#[tokio::test]
async fn bara_inloggade_med_åtkomst_får_pdf_en() {
    let db = Databas::tillfällig().await;
    let (session, bas, sida) = med_verifikat(&db).await;
    let v = sida.rsplit('=').next().unwrap();

    for path in [
        format!("{bas}/verifikat/{v}/pdf"),
        format!("{bas}/installningar/kontoplan/pdf"),
    ] {
        let (utloggad, _) = hämta(&db, "ingen", &path).await;
        assert_eq!(utloggad.status(), StatusCode::SEE_OTHER, "{path}");
        let annat = path.replace(&bas, "/foretag/4711");
        let (utan_åtkomst, _) = hämta(&db, &session, &annat).await;
        assert_eq!(utan_åtkomst.status(), StatusCode::FORBIDDEN, "{annat}");
    }
}

#[tokio::test]
async fn ett_okänt_verifikat_finns_inte() {
    let db = Databas::tillfällig().await;
    let (session, bas, _) = med_verifikat(&db).await;

    let (response, _) = hämta(&db, &session, &format!("{bas}/verifikat/999/pdf")).await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
