//! Underlag i webben, genom routern: ladda upp, visa eller ladda ned, ta bort, koppla till ett
//! utkast och komplettera ett bokfört verifikat.

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
use maya_domain::underlag::STÖRSTA_STORLEK;
use tower::ServiceExt;

const HOST: &str = "bokforing.example.se";
const ORIGIN: &str = "https://bokforing.example.se";
const KVITTO: &[u8] = b"%PDF-1.7\nett kvitto\n%%EOF\n";
const GRÄNS: &str = "----mayagrans";

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

async fn hämta_rått(
    db: &TillfälligDatabas,
    session: &str,
    path: &str,
) -> (Response<()>, Vec<u8>) {
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

async fn hämta(db: &TillfälligDatabas, session: &str, path: &str) -> String {
    let (response, body) = hämta_rått(db, session, path).await;
    assert_eq!(response.status(), StatusCode::OK, "{path}");
    String::from_utf8(body).unwrap()
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

/// Ett multipart-formulär med fälten och, om den finns, filen i fältet `fil`.
async fn posta_fil(
    db: &TillfälligDatabas,
    session: &str,
    path: &str,
    fält: &[(&str, &str)],
    fil: Option<(&str, &str, &[u8])>,
) -> Response<()> {
    let mut body = Vec::new();
    for (namn, värde) in fält {
        body.extend_from_slice(
            format!(
                "--{GRÄNS}\r\nContent-Disposition: form-data; name=\"{namn}\"\r\n\r\n{värde}\r\n"
            )
            .as_bytes(),
        );
    }
    if let Some((filnamn, mime, innehåll)) = fil {
        body.extend_from_slice(
            format!(
                "--{GRÄNS}\r\nContent-Disposition: form-data; name=\"fil\"; \
                 filename=\"{filnamn}\"\r\nContent-Type: {mime}\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(innehåll);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{GRÄNS}--\r\n").as_bytes());
    skicka(
        db,
        Request::post(path)
            .header(header::HOST, HOST)
            .header(header::ORIGIN, ORIGIN)
            .header(header::COOKIE, format!("maya_session={session}"))
            .header(
                header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={GRÄNS}"),
            )
            .body(Body::from(body))
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

fn parameter(plats: &str, namn: &str) -> String {
    url::Url::parse(&format!("https://x{plats}"))
        .unwrap()
        .query_pairs()
        .find(|(k, _)| k == namn)
        .map(|(_, v)| v.into_owned())
        .unwrap_or_else(|| panic!("{namn} saknas i {plats}"))
}

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

async fn ladda_upp(
    db: &TillfälligDatabas,
    session: &str,
    bas: &str,
    filnamn: &str,
    innehåll: &[u8],
) -> String {
    let response = posta_fil(
        db,
        session,
        &format!("{bas}/underlag"),
        &[],
        Some((filnamn, "application/octet-stream", innehåll)),
    )
    .await;
    parameter(&plats(&response), "u")
}

#[tokio::test]
async fn ett_uppladdat_underlag_visas_som_okopplat_med_sin_hash() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;

    let sida = hämta(&db, &session, &format!("{bas}/underlag")).await;
    assert!(sida.contains(r#"enctype="multipart/form-data""#), "{sida}");
    assert!(sida.contains(r#"type="file""#), "{sida}");

    let u = ladda_upp(&db, &session, &bas, "kvitto.pdf", KVITTO).await;

    let sida = hämta(&db, &session, &format!("{bas}/underlag?u={u}")).await;
    for text in [
        "kvitto.pdf",
        "7228b83aa5f1b9ef37d5d26106b76a3448842824c67176dbc947b0a9a77503b5",
        "anna@example.se",
        "Okopplat",
        &format!(r#"src="{bas}/underlag/{u}/fil""#),
    ] {
        assert!(sida.contains(text), "{text}: {sida}");
    }
}

#[tokio::test]
async fn pdf_och_bilder_visas_och_andra_format_laddas_ned() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;
    let pdf = ladda_upp(&db, &session, &bas, "kvitto.pdf", KVITTO).await;
    let csv = ladda_upp(&db, &session, &bas, "utdrag.csv", b"datum;belopp\n").await;

    let (response, body) = hämta_rått(&db, &session, &format!("{bas}/underlag/{pdf}/fil")).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body, KVITTO);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/pdf");
    let visning = response.headers()[header::CONTENT_DISPOSITION]
        .to_str()
        .unwrap();
    assert!(visning.starts_with("inline"), "{visning}");
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");

    let (response, _) = hämta_rått(&db, &session, &format!("{bas}/underlag/{csv}/fil")).await;
    let visning = response.headers()[header::CONTENT_DISPOSITION]
        .to_str()
        .unwrap();
    assert!(visning.starts_with("attachment"), "{visning}");
    assert!(visning.contains("utdrag.csv"), "{visning}");
    let sida = hämta(&db, &session, &format!("{bas}/underlag?u={csv}")).await;
    assert!(sida.contains("Ladda ned"), "{sida}");
    assert!(!sida.contains(&format!(r#"src="{bas}/underlag/{csv}/fil""#)));

    // Ett annat företags underlag nås inte.
    let (response, _) =
        hämta_rått(&db, &session, &format!("/foretag/999/underlag/{pdf}/fil")).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn samma_fil_igen_ger_en_varning_men_laddas_upp() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;
    ladda_upp(&db, &session, &bas, "kvitto.pdf", KVITTO).await;

    let response = posta_fil(
        &db,
        &session,
        &format!("{bas}/underlag"),
        &[],
        Some(("kvitto igen.pdf", "application/pdf", KVITTO)),
    )
    .await;

    let plats = plats(&response);
    let sida = hämta(&db, &session, &plats).await;
    assert!(sida.contains("kvitto igen.pdf"), "{sida}");
    assert!(sida.contains("Samma fil finns redan"), "{sida}");
}

#[tokio::test]
async fn ett_underlag_över_25_mb_nekas() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;
    let för_stort = vec![b'x'; STÖRSTA_STORLEK + 1];

    let response = posta_fil(
        &db,
        &session,
        &format!("{bas}/underlag"),
        &[],
        Some(("stor.bin", "application/octet-stream", &för_stort)),
    )
    .await;

    let plats = plats(&response);
    assert_eq!(parameter(&plats, "fel"), "for_stort");
    let sida = hämta(&db, &session, &plats).await;
    assert!(sida.contains("högst 25 MB"), "{sida}");
    assert!(!sida.contains("stor.bin"), "{sida}");
}

#[tokio::test]
async fn ett_okopplat_underlag_tas_bort_efter_en_fråga() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;
    let u = ladda_upp(&db, &session, &bas, "fel.pdf", KVITTO).await;

    let sida = hämta(&db, &session, &format!("{bas}/underlag?u={u}&ta_bort=1")).await;
    assert!(
        sida.contains(&format!(r#"action="{bas}/underlag/{u}/ta-bort""#)),
        "{sida}"
    );

    let response = posta(&db, &session, &format!("{bas}/underlag/{u}/ta-bort"), "").await;

    let plats = plats(&response);
    let sida = hämta(&db, &session, &plats).await;
    assert!(sida.contains("Underlaget togs bort"), "{sida}");
    assert!(!sida.contains("fel.pdf"), "{sida}");
}

const FORMULÄR: &[(&str, &str)] = &[
    ("datum", "2026-03-14"),
    ("beskrivning", "Datortillbehör"),
    ("konto_0", "6540"),
    ("debet_0", "1000"),
    ("kredit_0", ""),
    ("text_0", ""),
    ("konto_1", "1930"),
    ("debet_1", ""),
    ("kredit_1", "1000"),
    ("text_1", ""),
];

#[tokio::test]
async fn ett_underlag_kopplas_till_ett_utkast_och_kopplas_bort() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;
    let u = ladda_upp(&db, &session, &bas, "kvitto.pdf", KVITTO).await;

    // Formuläret visar okopplade underlag att välja.
    let sida = hämta(&db, &session, &format!("{bas}/verifikat/nytt")).await;
    assert!(sida.contains(r#"name="koppla""#), "{sida}");
    assert!(sida.contains("kvitto.pdf"), "{sida}");

    // Koppla utan JavaScript: formuläret kommer tillbaka med underlaget valt.
    let mut fält = FORMULÄR.to_vec();
    fält.push(("koppla", &u));
    let response = posta_fil(&db, &session, &format!("{bas}/verifikat"), &fält, None).await;
    let tillbaka = plats(&response);
    assert!(
        tillbaka.starts_with(&format!("{bas}/verifikat/nytt?")),
        "{tillbaka}"
    );
    let sida = hämta(&db, &session, &tillbaka).await;
    assert!(
        sida.contains(&format!(r#"name="underlag" value="{u}""#)),
        "{sida}"
    );
    assert!(sida.contains(r#"name="koppla_bort""#), "{sida}");

    // Spara utkast med underlaget.
    let mut fält = FORMULÄR.to_vec();
    fält.extend([
        ("underlag", u.as_str()),
        ("handling", "utkast"),
        ("underlag_finns", "1"),
    ]);
    let response = posta_fil(&db, &session, &format!("{bas}/verifikat"), &fält, None).await;
    let utkast = parameter(&plats(&response), "v");
    let sida = hämta(&db, &session, &format!("{bas}/verifikat?v={utkast}")).await;
    assert!(
        sida.contains(&format!(r#"name="underlag" value="{u}""#)),
        "{sida}"
    );
    let underlagssida = hämta(&db, &session, &format!("{bas}/underlag")).await;
    assert!(
        !underlagssida.contains("kvitto.pdf"),
        "inte längre okopplat"
    );

    // Koppla bort och spara: underlaget är okopplat igen.
    let mut fält = FORMULÄR.to_vec();
    fält.extend([("utkast", utkast.as_str()), ("handling", "utkast")]);
    let response = posta(
        &db,
        &session,
        &format!("{bas}/verifikat"),
        &fält
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("&"),
    )
    .await;
    plats(&response);
    // Ett urlencoded formulär utan underlagsfält rör inte kopplingarna (äldre klienter).
    let sida = hämta(&db, &session, &format!("{bas}/verifikat?v={utkast}")).await;
    assert!(sida.contains("kvitto.pdf"), "{sida}");
    let mut fält = FORMULÄR.to_vec();
    fält.extend([
        ("utkast", utkast.as_str()),
        ("handling", "utkast"),
        ("underlag_finns", "1"),
    ]);
    let response = posta_fil(&db, &session, &format!("{bas}/verifikat"), &fält, None).await;
    plats(&response);
    let underlagssida = hämta(&db, &session, &format!("{bas}/underlag")).await;
    assert!(underlagssida.contains("kvitto.pdf"), "okopplat igen");
}

#[tokio::test]
async fn en_fil_laddas_upp_i_formuläret_och_bokförs_med_verifikatet() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;
    let mut fält = FORMULÄR.to_vec();
    fält.extend([("handling", "bokfor"), ("underlag_finns", "1")]);

    let response = posta_fil(
        &db,
        &session,
        &format!("{bas}/verifikat"),
        &fält,
        Some(("kvitto.pdf", "application/pdf", KVITTO)),
    )
    .await;

    let plats = plats(&response);
    let sida = hämta(&db, &session, &plats).await;
    let dokument = &sida[sida.find(r#"class="dokument""#).expect(&sida)..];
    assert!(dokument.contains("V1"), "{dokument}");
    assert!(sida.contains("kvitto.pdf"), "{sida}");
    assert!(sida.contains("Vid bokföring"), "{sida}");
}

#[tokio::test]
async fn ett_bokfört_verifikat_kompletteras_och_visas_med_streckad_ram_datum_och_vem() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;
    let mut fält = FORMULÄR.to_vec();
    fält.push(("handling", "bokfor"));
    let response = posta_fil(&db, &session, &format!("{bas}/verifikat"), &fält, None).await;
    let v = parameter(&plats(&response), "v");
    let u = ladda_upp(&db, &session, &bas, "sent-kvitto.pdf", KVITTO).await;

    let sida = hämta(&db, &session, &format!("{bas}/verifikat?v={v}")).await;
    assert!(
        sida.contains("Kopplingen loggas och kan inte tas bort"),
        "{sida}"
    );
    assert!(
        sida.contains(&format!(r#"action="{bas}/verifikat/{v}/komplettera""#)),
        "{sida}"
    );

    let response = posta(
        &db,
        &session,
        &format!("{bas}/verifikat/{v}/komplettera"),
        &format!("underlag={u}"),
    )
    .await;

    let kompletterad = plats(&response);
    let sida = hämta(&db, &session, &kompletterad).await;
    let komplettering = &sida[sida.find("ufil komplettering").expect(&sida)..];
    for text in ["sent-kvitto.pdf", "Komplettering", "anna@example.se"] {
        assert!(komplettering.contains(text), "{text}: {komplettering}");
    }
    assert!(sida.contains("verifikat kompletterat"), "{sida}");

    // Komplettera med en ny fil direkt.
    let response = posta_fil(
        &db,
        &session,
        &format!("{bas}/verifikat/{v}/komplettera"),
        &[],
        Some(("avgiftsspec.pdf", "application/pdf", b"%PDF-1.7\nspec\n")),
    )
    .await;
    let sida = hämta(&db, &session, &plats(&response)).await;
    assert!(sida.contains("avgiftsspec.pdf"), "{sida}");

    // Ett underlag som hör till ett bokfört verifikat kan inte tas bort.
    let sida = hämta(&db, &session, &format!("{bas}/underlag?u={u}")).await;
    assert!(sida.contains("kan inte tas bort"), "{sida}");
    let response = posta(&db, &session, &format!("{bas}/underlag/{u}/ta-bort"), "").await;
    assert_eq!(parameter(&plats(&response), "fel"), "hor_till_bokfort");
}
