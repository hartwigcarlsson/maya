//! Rapporter → Bokföring → Reskontran per datum i webben, genom routern (#151): de öppna
//! posterna, avstämningen mot 1510 och 2440 med differensen, exporten som PDF och CSV och
//! differensen i Kundfakturors rubrik.

use std::sync::Arc;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use http_body_util::BodyExt;
use maya_core::config::Config;
use maya_core::{Databas, Klocka, TillfälligDatabas, testhjalp};
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

async fn hämta(db: &TillfälligDatabas, session: &str, path: &str) -> (Response<()>, Bytes) {
    let request = Request::get(path)
        .header(header::HOST, HOST)
        .header(header::COOKIE, format!("maya_session={session}"))
        .body(Body::empty())
        .unwrap();
    let response = app(db).oneshot(request).await.unwrap();
    let (parts, body) = response.into_parts();
    let body = body.collect().await.unwrap().to_bytes();
    (Response::from_parts(parts, ()), body)
}

async fn sida(db: &TillfälligDatabas, session: &str, path: &str) -> String {
    let (response, html) = hämta(db, session, path).await;
    assert_eq!(response.status(), StatusCode::OK, "{path}");
    String::from_utf8(html.to_vec()).unwrap()
}

/// Ett fakturaföretag med en utfärdad kundfaktura till Kund AB på 11 875 kr och ett manuellt
/// verifikat på 1510 som inte hör till någon faktura: sessionen och företagets sökväg.
async fn med_differens(db: &TillfälligDatabas) -> (String, String) {
    let (anna, id) = testhjalp::fakturaföretag(db).await;
    let session = testhjalp::session(db, &Klocka::system(), &anna).await;
    testhjalp::utfärdad_kundfaktura(db, &anna, id, "Kund AB", "2026-03-10").await;
    testhjalp::bokför(
        db,
        &anna,
        id,
        "2026-03-20",
        &[(1510, 50_000, 0), (3001, 0, 50_000)],
    )
    .await;
    (session, format!("/foretag/{id}"))
}

#[tokio::test]
async fn reskontran_per_datum_visar_de_öppna_posterna_och_differensen() {
    let db = Databas::tillfällig().await;
    let (session, bas) = med_differens(&db).await;

    let html = sida(
        &db,
        &session,
        &format!("{bas}/rapporter/reskontra?datum=2026-03-31"),
    )
    .await;
    let dokument = &html[html.find(r#"class="dokument"#).expect(&html)..];
    for väntat in [
        "Kundreskontra",
        "Kund AB",
        "2026-03-10",
        "11 875,00",
        "Bokförd",
        "Saldo på 1510 i huvudboken",
        "12 375,00",
        "Reskontran avviker från 1510 med -500,00 kr.",
        "Leverantörsreskontra",
        "Inga öppna poster.",
        "Reskontran stämmer med huvudboken.",
    ] {
        assert!(dokument.contains(väntat), "saknar {väntat:?}: {dokument}");
    }
    assert!(html.contains(r#"value="2026-03-31""#), "{html}");
    // Före fakturan och verifikatet finns inget.
    let html = sida(
        &db,
        &session,
        &format!("{bas}/rapporter/reskontra?datum=2026-03-01"),
    )
    .await;
    assert!(!html.contains("avviker"), "{html}");

    // Rapporterna och Bokföringens meny leder hit.
    let rapporter = sida(&db, &session, &format!("{bas}/rapporter")).await;
    assert!(
        rapporter.contains(&format!(r#"href="{bas}/rapporter/reskontra""#)),
        "{rapporter}"
    );
    let grundbok = sida(&db, &session, &format!("{bas}/rapporter/grundbok")).await;
    assert!(
        grundbok.contains(&format!(r#"href="{bas}/rapporter/reskontra""#)),
        "{grundbok}"
    );
}

#[tokio::test]
async fn reskontran_exporteras_som_pdf_och_csv_för_datumet() {
    let db = Databas::tillfällig().await;
    let (session, bas) = med_differens(&db).await;

    let (response, pdf) = hämta(
        &db,
        &session,
        &format!("{bas}/rapporter/reskontra/pdf?datum=2026-03-31"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/pdf");
    assert_eq!(
        response.headers()[header::CONTENT_DISPOSITION],
        r#"inline; filename="reskontra_5560360793_20260331.pdf""#
    );
    är_pdfa_2b(&pdf);
    let text = sidor(&pdf).join("\n");
    assert!(innehåller(&text, "Reskontran per 2026-03-31"), "{text}");
    assert!(innehåller(&text, "Kund AB"), "{text}");

    let (response, csv) = hämta(
        &db,
        &session,
        &format!("{bas}/rapporter/reskontra/csv?datum=2026-03-31"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_DISPOSITION],
        r#"attachment; filename="reskontra_5560360793_20260331.csv""#
    );
    let csv = String::from_utf8(csv.to_vec()).unwrap();
    assert!(
        csv.contains(
            "Kundreskontra;1;Kund AB;2026-03-10;2026-04-09;11875,00;11875,00;11875,00;Bokförd\r\n"
        ),
        "{csv}"
    );
    assert!(
        csv.contains("Kundreskontra;;Differens;;;;;-500,00;\r\n"),
        "{csv}"
    );
}

#[tokio::test]
async fn kundfakturornas_rubrik_visar_differensen_mot_1510() {
    let db = Databas::tillfällig().await;
    let (session, bas) = med_differens(&db).await;
    let html = sida(&db, &session, &format!("{bas}/kundfakturor")).await;
    let rubrik = &html[html.find("Kundfordringar").expect(&html)..];
    assert!(
        rubrik.contains("Avviker från 1510 med -500,00 kr"),
        "{rubrik}"
    );
    assert!(
        rubrik.contains(&format!(r#"href="{bas}/rapporter/reskontra""#)),
        "{rubrik}"
    );
    // Leverantörsreskontran stämmer, så dess rubrik visar ingen differens.
    let html = sida(&db, &session, &format!("{bas}/leverantorsfakturor")).await;
    assert!(!html.contains("Avviker från 2440"), "{html}");
}

#[tokio::test]
async fn bara_inloggade_med_åtkomst_ser_reskontran() {
    let db = Databas::tillfällig().await;
    let (session, bas) = med_differens(&db).await;
    let (response, html) = hämta(&db, &session, "/foretag/4711/rapporter/reskontra").await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(String::from_utf8_lossy(&html).contains("Du har inte åtkomst"));
    for export in ["pdf", "csv"] {
        let path = format!("{bas}/rapporter/reskontra/{export}");
        let (utloggad, _) = hämta(&db, "ingen", &path).await;
        assert_eq!(utloggad.status(), StatusCode::SEE_OTHER, "{path}");
        let annat = path.replace(&bas, "/foretag/4711");
        let (utan_åtkomst, _) = hämta(&db, &session, &annat).await;
        assert_eq!(utan_åtkomst.status(), StatusCode::FORBIDDEN, "{annat}");
    }
}

#[tokio::test]
async fn leverantörsfakturornas_rubrik_visar_differensen_mot_2440() {
    // En registrerad leverantörsfaktura på 1 250 kr och ett manuellt verifikat på 2440 som inte
    // hör till någon faktura: 2440 har 300 kr mer i skuld än reskontran.
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    let session = testhjalp::session(&db, &Klocka::system(), &anna).await;
    let l = testhjalp::leverantör(&db, &anna, id, "Kontorsvaror AB").await;
    testhjalp::registrerad_leverantörsfaktura(&db, &anna, id, l, "8812", "").await;
    testhjalp::bokför(
        &db,
        &anna,
        id,
        "2026-03-20",
        &[(5410, 30_000, 0), (2440, 0, 30_000)],
    )
    .await;
    let bas = format!("/foretag/{id}");
    let html = sida(&db, &session, &format!("{bas}/leverantorsfakturor")).await;
    let rubrik = &html[html.find("Leverantörsskulder").expect(&html)..];
    assert!(
        rubrik.contains("Avviker från 2440 med -300,00 kr"),
        "{rubrik}"
    );
    assert!(
        rubrik.contains(&format!(r#"href="{bas}/rapporter/reskontra""#)),
        "{rubrik}"
    );
    // Kundreskontran stämmer, så dess rubrik visar ingen differens.
    let html = sida(&db, &session, &format!("{bas}/kundfakturor")).await;
    assert!(!html.contains("Avviker från 1510"), "{html}");
}
