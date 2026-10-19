//! Externa kundfakturor i webben (#143): Ny → Extern faktura, utkastet med originalet bredvid
//! formuläret, registreringen, dokumentet i Kundfakturor med märket Extern, en extern
//! kreditfaktura och Rätta registrering.

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
const GRÄNS: &str = "gransgransgrans";

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

fn plats(response: &Response<()>) -> String {
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    response.headers()[header::LOCATION]
        .to_str()
        .unwrap()
        .to_string()
}

fn parameter(plats: &str, namn: &str) -> String {
    let (_, fråga) = plats
        .split_once('?')
        .unwrap_or_else(|| panic!("ingen fråga i {plats}"));
    url::form_urlencoded::parse(fråga.as_bytes())
        .find(|(k, _)| k == namn)
        .map(|(_, v)| v.into_owned())
        .unwrap_or_else(|| panic!("{namn} saknas i {plats}"))
}

async fn ladda_upp(
    db: &TillfälligDatabas, session: &str, sökväg: &str, filnamn: &str
) -> String {
    let body = format!(
        "--{GRÄNS}\r\nContent-Disposition: form-data; name=\"fil\"; filename=\"{filnamn}\"\r\n\
         Content-Type: application/pdf\r\n\r\n%PDF-1.7\n{filnamn}\n%%EOF\n\r\n--{GRÄNS}--\r\n"
    );
    let (response, _) = skicka(
        db,
        Request::post(sökväg)
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
    plats(&response)
}

/// Ett företag med kunden Kund AB: sessionen, företagets adress och kundens id.
async fn företag(db: &TillfälligDatabas) -> (String, String, String) {
    let session = inloggad(db).await;
    let bas = posta(
        db,
        &session,
        "/foretag",
        "orgnr=556036-0793&namn=Exempel+AB&start=2026-01-01&slut=2026-12-31\
         &bokforingsmetod=faktura&momsperiod=kvartal",
    )
    .await;
    let kund = posta(
        db,
        &session,
        &format!("{bas}/kunder"),
        "namn=Kund+AB&identitet=&adress=Storgatan+1&epost=&momsregnr=&betalningsvillkor=30\
         &omvand_skattskyldighet=ingen",
    )
    .await;
    (session, bas, parameter(&kund, "k"))
}

fn formulär(kund: &str, belopp: &str) -> String {
    format!(
        "kund={kund}&fakturanummer=F-17&fakturadatum=2026-03-10&forfallodag=2026-04-09\
         &momssats_0=25&konto_0=3001&belopp_0={belopp}&momssats_1=25&konto_1=&belopp_1="
    )
}

#[tokio::test]
async fn en_extern_faktura_laddas_upp_skrivs_av_och_registreras_i_kundfakturor() {
    let db = Databas::tillfällig().await;
    let (session, bas, kund) = företag(&db).await;

    // Ny → Extern faktura finns i toppmenyn och visar uppladdningen.
    let sida = hämta(&db, &session, &format!("{bas}/kundfakturor")).await;
    assert!(
        sida.contains(&format!("href=\"{bas}/kundfakturor/extern/ny\"")),
        "Ny → Extern faktura"
    );
    let ny = hämta(&db, &session, &format!("{bas}/kundfakturor/extern/ny")).await;
    assert!(ny.contains("Ny extern kundfaktura"), "{ny}");

    let adress = ladda_upp(
        &db,
        &session,
        &format!("{bas}/kundfakturor/extern"),
        "F-17.pdf",
    )
    .await;
    assert!(adress.contains("uppladdat=1"), "{adress}");
    let f = parameter(&adress, "f");
    let utkast = hämta(&db, &session, &adress).await;
    let detalj = &utkast[utkast.find("med-underlag").expect(&utkast)..];
    assert!(detalj.contains("/underlag/"), "originalet visas: {detalj}");
    assert!(detalj.contains("Registrera och bokför"), "{detalj}");
    assert!(detalj.contains("Kund AB"), "kundväljaren");

    // En spärr skickar tillbaka formuläret med det som skrevs.
    let tillbaka = posta(
        &db,
        &session,
        &format!("{bas}/kundfakturor/extern/{f}"),
        &format!("handling=registrera&{}", formulär("", "8000")),
    )
    .await;
    assert!(tillbaka.contains("fel=extern_sparrat"), "{tillbaka}");
    let sida = hämta(&db, &session, &tillbaka).await;
    assert!(sida.contains("kund_saknas"), "spärren vid fältet");

    let registrerad = posta(
        &db,
        &session,
        &format!("{bas}/kundfakturor/extern/{f}"),
        &format!("handling=registrera&{}", formulär(&kund, "8000")),
    )
    .await;
    assert!(registrerad.contains("registrerad=V1"), "{registrerad}");
    let dokument = hämta(&db, &session, &registrerad).await;
    assert!(
        dokument.contains("Fakturan registrerades och bokfördes som V1."),
        "{dokument}"
    );
    let detalj = &dokument[dokument.find("verifikat-detalj").unwrap()..];
    assert!(detalj.contains("Extern kundfaktura F-17"), "{detalj}");
    assert!(detalj.contains("badge-extern"), "{detalj}");
    assert!(detalj.contains("10 000,00"), "totalen: {detalj}");
    assert!(detalj.contains("Registrera betalning"), "{detalj}");
    assert!(detalj.contains("Rätta registrering…"), "{detalj}");
    assert!(detalj.contains("Registrera kreditfaktura…"), "{detalj}");
    assert!(detalj.contains("1510"), "konteringen: {detalj}");
    // I listan med numret från det andra systemet och märket Extern.
    let lista = &dokument[..dokument.find("verifikat-detalj").unwrap()];
    assert!(lista.contains("F-17"), "{lista}");
    assert!(lista.contains("badge-extern"), "{lista}");
}

#[tokio::test]
async fn en_extern_kreditfaktura_och_rätta_registrering_i_dokumentet() {
    let db = Databas::tillfällig().await;
    let (session, bas, kund) = företag(&db).await;
    let adress = ladda_upp(
        &db,
        &session,
        &format!("{bas}/kundfakturor/extern"),
        "F-17.pdf",
    )
    .await;
    let f = parameter(&adress, "f");
    posta(
        &db,
        &session,
        &format!("{bas}/kundfakturor/extern/{f}"),
        &format!("handling=registrera&{}", formulär(&kund, "8000")),
    )
    .await;

    // Registrera kreditfaktura… laddar upp kreditfakturan med hänvisningen.
    let panel = hämta(
        &db,
        &session,
        &format!("{bas}/kundfakturor?f={f}&panel=kreditera-extern"),
    )
    .await;
    assert!(panel.contains("Registrera kreditfaktura"), "{panel}");
    let kredit = ladda_upp(
        &db,
        &session,
        &format!("{bas}/kundfakturor/extern/{f}/kreditera"),
        "K-4.pdf",
    )
    .await;
    assert!(kredit.contains("extern_kreditutkast=1"), "{kredit}");
    let k = parameter(&kredit, "f");
    let sida = hämta(&db, &session, &kredit).await;
    assert!(sida.contains("Krediterar faktura F-17"), "{sida}");
    let registrerad = posta(
        &db,
        &session,
        &format!("{bas}/kundfakturor/extern/{k}"),
        "handling=registrera&fakturanummer=K-4&fakturadatum=2026-03-20&forfallodag=\
         &momssats_0=25&konto_0=3001&belopp_0=-2000",
    )
    .await;
    assert!(registrerad.contains("registrerad=V2"), "{registrerad}");
    let original = hämta(&db, &session, &format!("{bas}/kundfakturor?f={f}")).await;
    assert!(original.contains("Kreditfaktura K-4"), "{original}");
    assert!(original.contains("7 500,00 kvar"), "{original}");

    // Rätta registrering spärras medan kreditfakturan finns, men går på kreditfakturan.
    let spärrad = posta(
        &db,
        &session,
        &format!("{bas}/kundfakturor/extern/{f}/ratta-registrering"),
        "orsak=",
    )
    .await;
    assert!(spärrad.contains("fel=har_kreditfakturor"), "{spärrad}");
    let rättad = posta(
        &db,
        &session,
        &format!("{bas}/kundfakturor/extern/{k}/ratta-registrering"),
        "orsak=fel+belopp",
    )
    .await;
    assert!(rättad.contains("registrering_rattad=V3"), "{rättad}");
    let upplåst = hämta(&db, &session, &rättad).await;
    let detalj = &upplåst[upplåst.find("verifikat-detalj").unwrap()..];
    assert!(detalj.contains("badge-upplast"), "{detalj}");
    assert!(
        detalj.contains("Registrera och bokför"),
        "skrivs av igen: {detalj}"
    );
    assert!(detalj.contains("V2 vändes med V3: fel belopp"), "{detalj}");
    assert!(!detalj.contains("Radera utkast"), "en upplåst raderas inte");
}
