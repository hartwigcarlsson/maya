//! Att göra i webben, genom routern: nyckeltalen, utkasten med sina frister, de okopplade
//! underlagen och "Vad är det här?", antalet i toppmenyn och Bokför som går vidare.
//!
//! Routern använder systemets klocka, så datumen räknas från i dag.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use http_body_util::BodyExt;
use jiff::ToSpan;
use jiff::civil::Date;
use maya_core::aktor::Aktör;
use maya_core::config::Config;
use maya_core::inloggning::{self, NyPasskey};
use maya_core::{Databas, Klocka, TillfälligDatabas};
use tower::ServiceExt;

const HOST: &str = "bokforing.example.se";
const ORIGIN: &str = "https://bokforing.example.se";
const GRÄNS: &str = "----mayagrans";

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

async fn hämta(db: &TillfälligDatabas, session: &str, path: &str) -> String {
    let (response, body) = skicka(
        db,
        Request::get(path)
            .header(header::HOST, HOST)
            .header(header::COOKIE, format!("maya_session={session}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK, "{path}: {body}");
    body
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
    plats(&response)
}

async fn ladda_upp(db: &TillfälligDatabas, session: &str, bas: &str, filnamn: &str) -> String {
    let mut body = format!(
        "--{GRÄNS}\r\nContent-Disposition: form-data; name=\"fil\"; filename=\"{filnamn}\"\r\n\
         Content-Type: application/pdf\r\n\r\n%PDF-1.7\n{filnamn}\n%%EOF\n\r\n"
    );
    body.push_str(&format!("--{GRÄNS}--\r\n"));
    let (response, _) = skicka(
        db,
        Request::post(format!("{bas}/underlag"))
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
    .await;
    parameter(&plats(&response), "u")
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

fn idag() -> Date {
    Klocka::system().idag()
}

fn dagar_sedan(n: i64) -> Date {
    idag().checked_sub(n.days()).unwrap()
}

/// Ett företag vars första räkenskapsår började för drygt ett halvår sedan.
async fn företag(db: &TillfälligDatabas) -> (String, String) {
    let session = inloggad(db).await;
    let start = dagar_sedan(200).first_of_month();
    let slut = start
        .checked_add(12.months())
        .unwrap()
        .checked_sub(1.day())
        .unwrap();
    let bas = posta(
        db,
        &session,
        "/foretag",
        &format!(
            "orgnr=556036-0793&namn=Exempel+AB&start={start}&slut={slut}\
             &bokforingsmetod=faktura&momsperiod=kvartal"
        ),
    )
    .await;
    (session, bas)
}

/// Ett verifikat med två rader: debet på `debet`, kredit på `kredit`.
fn formulär(datum: Date, beskrivning: &str, debet: u16, kredit: u16, belopp: &str) -> String {
    format!(
        "datum={datum}&beskrivning={beskrivning}\
         &konto_0={debet}&debet_0={belopp}&kredit_0=&text_0=\
         &konto_1={kredit}&debet_1=&kredit_1={belopp}&text_1="
    )
}

async fn spara_utkast(db: &TillfälligDatabas, session: &str, bas: &str, f: &str) -> String {
    let plats = posta(
        db,
        session,
        &format!("{bas}/verifikat"),
        &format!("handling=utkast&utkast=&{f}"),
    )
    .await;
    parameter(&plats, "v")
}

fn antal(text: &str, mönster: &str) -> usize {
    text.matches(mönster).count()
}

#[tokio::test]
async fn att_göra_visar_nyckeltalen_ur_bokföringen_och_utkasten_med_sina_frister() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;
    posta(
        &db,
        &session,
        &format!("{bas}/verifikat"),
        &format!(
            "handling=bokfor&utkast=&{}",
            formulär(idag(), "Försäljning", 1930, 3001, "1250")
        ),
    )
    .await;
    // Ett utkast räknas aldrig i nyckeltalen.
    spara_utkast(
        &db,
        &session,
        &bas,
        &formulär(dagar_sedan(100), "Hyra", 5010, 1930, "900"),
    )
    .await;
    let kassa = spara_utkast(
        &db,
        &session,
        &bas,
        &formulär(dagar_sedan(100), "Kontant", 1910, 3001, "300"),
    )
    .await;
    ladda_upp(&db, &session, &bas, "kvitto.pdf").await;

    let sida = hämta(&db, &session, &bas).await;

    assert!(sida.contains("Att göra"), "{sida}");
    assert!(sida.contains("Företagskontot"), "{sida}");
    assert!(sida.contains("Resultat hittills i år"), "{sida}");
    assert_eq!(antal(&sida, "· ur bokföringen</span>"), 2, "{sida}");
    assert!(sida.contains("1 250,00 kr"), "{sida}");
    // Kassans utkast ligger i tidslinjen som försenat, med fristen och utan 50-dagarsvarning.
    assert!(sida.contains("Försenat"), "{sida}");
    assert!(sida.contains(&format!("utkast-{kassa}")), "{sida}");
    assert!(sida.contains("Kontant"), "{sida}");
    // Bara Hyra har 50-dagarsvarningen.
    assert_eq!(antal(&sida, "badge-varning"), 1, "{sida}");
    assert!(sida.contains("Hyra"), "{sida}");
    assert!(sida.contains("Okopplade underlag"), "{sida}");
    assert!(sida.contains("kvitto.pdf"), "{sida}");
    // Antalet saker att göra i toppmenyn och i företagsväljaren.
    assert_eq!(antal(&sida, r#"<span class="antal">3</span>"#), 2, "{sida}");

    // Utkastet fälls ut på plats med formuläret, som visar kontantfristen som en notis.
    let sida = hämta(&db, &session, &format!("{bas}?oppna=utkast-{kassa}")).await;
    assert!(sida.contains(r#"id="verifikatformular""#), "{sida}");
    assert!(sida.contains("notis-frist"), "{sida}");
    assert!(sida.contains("Kontantfristen"), "{sida}");
    let sida = hämta(&db, &session, &format!("{bas}/verifikat?v={kassa}")).await;
    assert!(sida.contains("notis-frist"), "{sida}");
}

#[tokio::test]
async fn bokför_från_att_göra_fäller_ut_nästa_utkast() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;
    let första = spara_utkast(
        &db,
        &session,
        &bas,
        &formulär(dagar_sedan(3), "Kontorsmaterial", 5410, 1930, "120"),
    )
    .await;
    let andra = spara_utkast(
        &db,
        &session,
        &bas,
        &formulär(dagar_sedan(2), "Porto", 6250, 1930, "40"),
    )
    .await;

    let plats = posta(
        &db,
        &session,
        &format!("{bas}/verifikat"),
        &format!(
            "handling=bokfor&efter=att_gora&utkast={första}&{}",
            formulär(dagar_sedan(3), "Kontorsmaterial", 5410, 1930, "120")
        ),
    )
    .await;

    assert!(plats.starts_with(&format!("{bas}?")), "{plats}");
    let sida = hämta(&db, &session, &plats).await;
    assert!(sida.contains("Bokförd som V1. 1 utkast kvar."), "{sida}");
    assert!(sida.contains(r#"id="verifikatformular""#), "{sida}");
    assert!(
        sida.contains(&format!(r#"name="utkast" value="{andra}""#)),
        "{sida}"
    );
}

#[tokio::test]
async fn vad_är_det_här_gör_kvitto_komplettering_eller_tar_bort() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;
    let bokfört = posta(
        &db,
        &session,
        &format!("{bas}/verifikat"),
        &format!(
            "handling=bokfor&utkast=&{}",
            formulär(dagar_sedan(5), "Lunch", 5810, 1930, "200")
        ),
    )
    .await;
    let v = parameter(&bokfört, "v");
    let kvitto = ladda_upp(&db, &session, &bas, "kvitto.pdf").await;
    let komplettering = ladda_upp(&db, &session, &bas, "bilaga.pdf").await;
    let fel = ladda_upp(&db, &session, &bas, "fel.pdf").await;

    let sida = hämta(&db, &session, &format!("{bas}?oppna=underlag-{kvitto}")).await;
    assert!(sida.contains("Vad är det här?"), "{sida}");
    assert!(sida.contains("Kvitto"), "{sida}");
    assert!(sida.contains("Komplettering"), "{sida}");
    assert!(sida.contains("Dubblett eller fel fil"), "{sida}");

    // Kvitto: ett nytt utkast med underlaget, utfällt.
    let plats = posta(
        &db,
        &session,
        &format!("{bas}/att-gora/underlag/{kvitto}/kvitto"),
        "",
    )
    .await;
    let sida = hämta(&db, &session, &plats).await;
    assert!(sida.contains(r#"id="verifikatformular""#), "{sida}");
    assert!(sida.contains("Ett verifikatutkast skapades"), "{sida}");

    // Komplettering: kopplas till det bokförda verifikatet.
    let plats = posta(
        &db,
        &session,
        &format!("{bas}/att-gora/underlag/{komplettering}/komplettera"),
        &format!("verifikat={v}"),
    )
    .await;
    let sida = hämta(&db, &session, &plats).await;
    assert!(sida.contains("kompletterades"), "{sida}");
    let dokument = hämta(&db, &session, &format!("{bas}/verifikat?v={v}")).await;
    assert!(dokument.contains("bilaga.pdf"), "{dokument}");

    // Dubblett eller fel fil: tas bort.
    let plats = posta(
        &db,
        &session,
        &format!("{bas}/att-gora/underlag/{fel}/ta-bort"),
        "",
    )
    .await;
    let sida = hämta(&db, &session, &plats).await;
    assert!(sida.contains("Underlaget togs bort"), "{sida}");
    assert!(!sida.contains("fel.pdf"), "{sida}");
    assert!(!sida.contains("bilaga.pdf"), "{sida}");
    assert!(!sida.contains("Okopplade underlag"), "{sida}");
}
