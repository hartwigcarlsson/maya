//! Rapporter → Bokföring i webben: grundboken och huvudboken som dokument, panelen till
//! verifikatet och exporten som PDF och CSV.

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

async fn användare(db: &Databas, epost: &str) -> String {
    let länk = inloggning::skapa_första_användare(db, &Klocka::system(), &Aktör::cli(), epost)
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

async fn sida(db: &TillfälligDatabas, session: &str, path: &str) -> String {
    let (response, html) = hämta(db, session, path).await;
    assert_eq!(response.status(), StatusCode::OK, "{path}");
    String::from_utf8(html.to_vec()).unwrap()
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

/// Ett inloggat företag med två bokförda verifikat i mars och ett utkast: sessionen, företagets
/// sökväg och det första verifikatets id.
async fn med_bokföring(db: &TillfälligDatabas) -> (String, String, String) {
    let session = användare(db, "anna@example.se").await;
    let bas = posta(
        db,
        &session,
        "/foretag",
        "orgnr=556036-0793&namn=Exempel+AB&start=2026-01-01&slut=2026-12-31\
         &bokforingsmetod=faktura&momsperiod=kvartal",
    )
    .await;
    let v1 = posta(
        db,
        &session,
        &format!("{bas}/verifikat"),
        "datum=2026-03-20&beskrivning=F%C3%B6rs%C3%A4ljning\
         &konto_0=1930&debet_0=500&kredit_0=&text_0=Swish\
         &konto_1=3001&debet_1=&kredit_1=500&text_1=",
    )
    .await;
    posta(
        db,
        &session,
        &format!("{bas}/verifikat"),
        "datum=2026-03-05&beskrivning=Hyra\
         &konto_0=5010&debet_0=200&kredit_0=&text_0=\
         &konto_1=1930&debet_1=&kredit_1=200&text_1=",
    )
    .await;
    posta(
        db,
        &session,
        &format!("{bas}/verifikat"),
        "handling=utkast&utkast=&datum=2026-03-28&beskrivning=Ett+utkast\
         &konto_0=1930&debet_0=9&kredit_0=&text_0=\
         &konto_1=3001&debet_1=&kredit_1=9&text_1=",
    )
    .await;
    let id = v1.rsplit("v=").next().unwrap().to_string();
    (session, bas, id)
}

/// `href` för länken med `class="{klass}"` i HTML:en, med `&amp;` avkodat.
fn länk(html: &str, klass: &str) -> String {
    let i = html
        .find(&format!(r#"class="{klass}""#))
        .unwrap_or_else(|| panic!("ingen länk {klass} i {html}"));
    let start = html[..i].rfind("<a").unwrap();
    let tagg = &html[start..start + html[start..].find('>').unwrap()];
    let href = tagg.find(r#"href=""#).unwrap() + 6;
    tagg[href..href + tagg[href..].find('"').unwrap()].replace("&amp;", "&")
}

#[tokio::test]
async fn grundboken_visar_bokförda_verifikat_i_registreringsordning_utan_utkast() {
    let db = Databas::tillfällig().await;
    let (session, bas, _) = med_bokföring(&db).await;

    let html = sida(&db, &session, &format!("{bas}/rapporter/grundbok")).await;

    assert!(html.contains("Grundbok"), "{html}");
    assert!(html.contains("mars 2026"), "{html}");
    let (v1, v2) = (html.find(">V1<").unwrap(), html.find(">V2<").unwrap());
    assert!(v1 < v2, "V1 bokfördes först");
    assert!(
        html.contains("Försäljning") && html.contains("Hyra"),
        "{html}"
    );
    assert!(html.contains("Swish"), "{html}");
    assert!(!html.contains("Ett utkast"), "utkast syns aldrig: {html}");
    assert!(html.contains("Bokfört "), "registreringstidpunkten: {html}");
}

#[tokio::test]
async fn huvudboken_visar_saldon_för_valda_konton() {
    let db = Databas::tillfällig().await;
    let (session, bas, _) = med_bokföring(&db).await;

    let html = sida(
        &db,
        &session,
        &format!("{bas}/rapporter/huvudbok?konton=1930"),
    )
    .await;

    assert!(html.contains("Huvudbok"), "{html}");
    assert!(html.contains("1930 Företagskonto"), "{html}");
    assert!(!html.contains("Lokalhyra"), "bara 1930: {html}");
    for väntat in ["Ingående balans", "500,00", "300,00", "Utgående balans"] {
        assert!(html.contains(väntat), "saknar {väntat}: {html}");
    }
    assert!(
        html.contains(r#"value="1930""#),
        "urvalet står kvar i fältet: {html}"
    );
}

#[tokio::test]
async fn ett_ogiltigt_kontourval_visas_som_fel() {
    let db = Databas::tillfällig().await;
    let (session, bas, _) = med_bokföring(&db).await;

    let html = sida(
        &db,
        &session,
        &format!("{bas}/rapporter/huvudbok?konton=abc"),
    )
    .await;

    assert!(html.contains(r#"class="fel""#), "{html}");
}

#[tokio::test]
async fn ett_verifikat_öppnas_i_en_panel_med_länk_till_verifikatet() {
    let db = Databas::tillfällig().await;
    let (session, bas, id) = med_bokföring(&db).await;

    let html = sida(&db, &session, &format!("{bas}/rapporter/grundbok?v={id}")).await;

    assert!(html.contains(r#"class="sidopanel""#), "{html}");
    assert_eq!(
        länk(&html, "till-verifikatet"),
        format!("{bas}/verifikat?v={id}")
    );
}

#[tokio::test]
async fn grundboken_och_huvudboken_exporteras_som_pdf_och_csv() {
    let db = Databas::tillfällig().await;
    let (session, bas, _) = med_bokföring(&db).await;

    for (rapport, väntat) in [("grundbok", "Grundbok"), ("huvudbok", "Huvudbok")] {
        let html = sida(
            &db,
            &session,
            &format!("{bas}/rapporter/{rapport}?till=2026-03"),
        )
        .await;
        let pdf = länk(&html, "skriv-ut");
        assert_eq!(pdf, länk(&html, "exportera-pdf"), "samma PDF");
        let (response, innehåll) = hämta(&db, &session, &pdf).await;
        assert_eq!(response.status(), StatusCode::OK, "{pdf}");
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/pdf");
        assert_eq!(
            response.headers()[header::CONTENT_DISPOSITION],
            format!(r#"inline; filename="{rapport}_5560360793_2026_202603.pdf""#).as_str()
        );
        är_pdfa_2b(&innehåll);
        assert!(innehåller(&sidor(&innehåll).concat(), väntat));

        let csv = länk(&html, "exportera-csv");
        let (response, innehåll) = hämta(&db, &session, &csv).await;
        assert_eq!(response.status(), StatusCode::OK, "{csv}");
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "text/csv; charset=utf-8"
        );
        assert_eq!(
            response.headers()[header::CONTENT_DISPOSITION],
            format!(r#"attachment; filename="{rapport}_5560360793_2026_202603.csv""#).as_str()
        );
        let text = String::from_utf8(innehåll.to_vec()).unwrap();
        assert!(text.starts_with('\u{feff}'), "BOM");
        assert!(text.contains(";V1;"), "{text}");
        assert!(!text.contains("Ett utkast"), "{text}");
    }
}

#[tokio::test]
async fn bara_inloggade_med_åtkomst_ser_bokföringen() {
    let db = Databas::tillfällig().await;
    let (session, bas, _) = med_bokföring(&db).await;

    for rapport in ["grundbok", "huvudbok"] {
        let (response, html) =
            hämta(&db, &session, &format!("/foretag/4711/rapporter/{rapport}")).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert!(String::from_utf8_lossy(&html).contains("Du har inte åtkomst"));
        for export in ["pdf", "csv"] {
            let path = format!("{bas}/rapporter/{rapport}/{export}");
            let (utloggad, _) = hämta(&db, "ingen", &path).await;
            assert_eq!(utloggad.status(), StatusCode::SEE_OTHER, "{path}");
            let annat = path.replace(&bas, "/foretag/4711");
            let (utan_åtkomst, _) = hämta(&db, &session, &annat).await;
            assert_eq!(utan_åtkomst.status(), StatusCode::FORBIDDEN, "{annat}");
        }
    }
}

#[tokio::test]
async fn rapporterna_leder_till_grundbok_huvudbok_och_kontoplan() {
    let db = Databas::tillfällig().await;
    let (session, bas, _) = med_bokföring(&db).await;

    let html = sida(&db, &session, &format!("{bas}/rapporter")).await;

    for href in [
        format!("{bas}/rapporter/grundbok"),
        format!("{bas}/rapporter/huvudbok"),
        format!("{bas}/installningar/kontoplan"),
    ] {
        assert!(
            html.contains(&format!(r#"href="{href}""#)),
            "saknar {href}: {html}"
        );
    }
}
