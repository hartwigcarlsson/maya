//! Betalning av leverantörsfaktura i webben, genom routern (#147): betaluppgifterna i
//! dokumentet, panelen Registrera betalning med "Bokförs som", delbetalning, kursdifferens på en
//! faktura med originalvaluta, Rätta betalning och listans grupper med leverantörsskulderna.

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
    assert_eq!(response.status(), StatusCode::OK, "{path}");
    body
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

fn parameter(plats: &str, namn: &str) -> String {
    let (_, fråga) = plats
        .split_once('?')
        .unwrap_or_else(|| panic!("ingen fråga i {plats}"));
    url::form_urlencoded::parse(fråga.as_bytes())
        .find(|(k, _)| k == namn)
        .map(|(_, v)| v.into_owned())
        .unwrap_or_else(|| panic!("{namn} saknas i {plats}"))
}

fn efter(sida: &str, text: &str) -> String {
    sida[sida
        .find(text)
        .unwrap_or_else(|| panic!("{text} saknas i {sida}"))..]
        .to_string()
}

/// Ett företag med leverantören Kontorsvaror AB (bankgiro 5050-1055) och dess registrerade
/// faktura 8812 på 1 250,00 kr, förfallen 2026-04-13; sessionen, basen och dokumentets adress.
async fn registrerad(db: &TillfälligDatabas, originalvaluta: &str) -> (String, String, String) {
    let session = inloggad(db).await;
    let bas = plats(
        &posta(
            db,
            &session,
            "/foretag",
            "orgnr=556036-0793&namn=Exempel+AB&start=2026-01-01&slut=2026-12-31\
             &bokforingsmetod=faktura&momsperiod=kvartal",
        )
        .await,
    );
    let l = parameter(
        &plats(
            &posta(
                db,
                &session,
                &format!("{bas}/leverantorer"),
                "namn=Kontorsvaror+AB&land=SE&bankgiro=5050-1055&standardkonto=5410",
            )
            .await,
        ),
        "l",
    );
    let body = format!(
        "--{GRÄNS}\r\nContent-Disposition: form-data; name=\"fil\"; filename=\"f.pdf\"\r\n\
         Content-Type: application/pdf\r\n\r\n%PDF-1.7\nf\n%%EOF\n\r\n--{GRÄNS}--\r\n"
    );
    let (r, _) = skicka(
        db,
        Request::post(format!("{bas}/leverantorsfakturor"))
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
    let f = parameter(&plats(&r), "f");
    let registrerad = plats(
        &posta(
            db,
            &session,
            &format!("{bas}/leverantorsfakturor/{f}"),
            &format!(
                "handling=registrera&leverantor={l}&fakturanummer=8812&fakturadatum=2026-03-14\
                 &forfallodag=2026-04-13&referens=4455667788&totalbelopp=1250\
                 &originalvaluta={originalvaluta}\
                 &konto_0=&netto_0=1000&momssats_0=25&moms_0="
            ),
        )
        .await,
    );
    assert!(registrerad.contains("registrerad="), "{registrerad}");
    let här = format!("{bas}/leverantorsfakturor?f={f}");
    (session, bas, här)
}

#[tokio::test]
async fn en_betalning_registreras_i_dokumentet_med_betaluppgifterna_synliga() {
    let db = Databas::tillfällig().await;
    let (session, bas, här) = registrerad(&db, "").await;
    let f = parameter(&här, "f");

    let dokument = hämta(&db, &session, &här).await;
    let dokument = efter(&dokument, "class=\"dokument\"");
    assert!(dokument.contains("badge-forfallen"), "{dokument}");
    assert!(dokument.contains("1 250,00 kvar"));
    assert!(dokument.contains("Inga betalningar än."));
    let betaluppgifter = efter(&dokument, "class=\"metadata betaluppgifter\"");
    assert!(betaluppgifter.contains("Bankgiro"), "{betaluppgifter}");
    assert!(betaluppgifter.contains("5050-1055"));
    assert!(betaluppgifter.contains("4455667788"));

    let panel = hämta(&db, &session, &format!("{här}&panel=betalning")).await;
    assert!(
        efter(&panel, "class=\"dokument\"").contains("5050-1055"),
        "betaluppgifterna syns vid panelen"
    );
    let panel = efter(&panel, "betalning-rubrik");
    assert!(panel.contains(r#"value="1 250,00""#), "{panel}");
    assert!(panel.contains("Från konto"));
    assert!(panel.contains("Bokförs som verifikat"));
    assert!(panel.contains("2440"));
    assert!(
        !panel.contains(r#"name="kursdifferens""#),
        "ingen originalvaluta"
    );
    assert!(panel.contains(&format!("{bas}/leverantorsfakturor/{f}/betalning")));

    let betald = plats(
        &posta(
            &db,
            &session,
            &format!("{bas}/leverantorsfakturor/{f}/betalning"),
            "betaldag=2026-04-01&belopp=500&konto=1930",
        )
        .await,
    );
    assert!(betald.contains("betald=1"), "{betald}");
    let dokument = hämta(&db, &session, &betald).await;
    assert!(dokument.contains("Betalningen registrerades"));
    let betalningar = efter(&dokument, "<h3>Betalningar");
    assert!(betalningar.contains("2026-04-01"));
    assert!(betalningar.contains("500,00"));
    assert!(betalningar.contains("750,00"), "det som återstår");
    assert!(betalningar.contains("Rätta betalning…"));

    // En överbetalning spärras vid beloppet.
    let tillbaka = plats(
        &posta(
            &db,
            &session,
            &format!("{bas}/leverantorsfakturor/{f}/betalning"),
            "betaldag=2026-04-01&belopp=1000&konto=1930",
        )
        .await,
    );
    assert!(tillbaka.contains("fel=betalning"), "{tillbaka}");
    let sida = hämta(&db, &session, &tillbaka).await;
    assert!(sida.contains(r#"data-kod="overbetalning""#), "{sida}");
}

#[tokio::test]
async fn en_avvikelse_i_originalvaluta_stängs_mot_kursdifferens() {
    let db = Databas::tillfällig().await;
    let (session, bas, här) = registrerad(&db, "EUR+109,50").await;
    let f = parameter(&här, "f");
    let panel = hämta(&db, &session, &format!("{här}&panel=betalning")).await;
    let panel = efter(&panel, "betalning-rubrik");
    assert!(panel.contains("EUR 109,50"), "{panel}");
    assert!(panel.contains(r#"name="kursdifferens""#), "{panel}");

    let betald = plats(
        &posta(
            &db,
            &session,
            &format!("{bas}/leverantorsfakturor/{f}/betalning"),
            "betaldag=2026-04-10&belopp=1300&konto=1930&kursdifferens=1",
        )
        .await,
    );
    assert!(betald.contains("betald=1"), "{betald}");
    let dokument = hämta(&db, &session, &betald).await;
    let dokument = efter(&dokument, "class=\"dokument\"");
    assert!(dokument.contains("badge-betald"), "{dokument}");
    assert!(
        efter(&dokument, "<h3>Betalningar").contains("50,00 kursförlust"),
        "{dokument}"
    );
}

#[tokio::test]
async fn rätta_betalning_tar_bort_betalningen_ur_reskontran() {
    let db = Databas::tillfällig().await;
    let (session, bas, här) = registrerad(&db, "").await;
    let f = parameter(&här, "f");
    posta(
        &db,
        &session,
        &format!("{bas}/leverantorsfakturor/{f}/betalning"),
        "betaldag=2026-04-01&belopp=1250&konto=1930",
    )
    .await;
    let dokument = hämta(&db, &session, &här).await;
    assert!(dokument.contains("badge-betald"));
    let b = dokument
        .split("panel=ratta-betalning&amp;b=")
        .nth(1)
        .expect("länken Rätta betalning")
        .split('"')
        .next()
        .unwrap()
        .to_string();
    let panel = hämta(&db, &session, &format!("{här}&panel=ratta-betalning&b={b}")).await;
    let panel = efter(&panel, "ratta-betalning-rubrik");
    assert!(panel.contains("vänds med en rättelse"));
    assert!(panel.contains(&format!(
        "{bas}/leverantorsfakturor/{f}/betalningar/{b}/ratta"
    )));
    let rättad = plats(
        &posta(
            &db,
            &session,
            &format!("{bas}/leverantorsfakturor/{f}/betalningar/{b}/ratta"),
            "datum=2026-10-01&orsak=fel+belopp",
        )
        .await,
    );
    assert!(rättad.contains("betalning_rattad=1"), "{rättad}");
    let dokument = hämta(&db, &session, &rättad).await;
    assert!(dokument.contains("Betalningen rättades"), "{dokument}");
    assert!(dokument.contains("1 250,00 kvar"));
    assert!(efter(&dokument, "<h3>Betalningar").contains("Rättad"));
}

#[tokio::test]
async fn listan_grupperas_med_leverantörsskulderna_i_rubriken() {
    let db = Databas::tillfällig().await;
    let (session, bas, här) = registrerad(&db, "").await;
    let f = parameter(&här, "f");
    let lista = hämta(&db, &session, &format!("{bas}/leverantorsfakturor")).await;
    assert!(lista.contains("Leverantörsskulder 1 250,00 kr"), "{lista}");
    assert!(lista.contains(">Förfallna<"), "{lista}");
    assert!(lista.contains("badge-forfallen"));
    // Berättelse 70: gruppens summa av skulderna står i rubriken.
    assert!(
        efter(&lista, ">Förfallna<").contains(" · 1 · 1 250,00 kr</span>"),
        "{lista}"
    );

    posta(
        &db,
        &session,
        &format!("{bas}/leverantorsfakturor/{f}/betalning"),
        "betaldag=2026-04-01&belopp=1250&konto=1930",
    )
    .await;
    let lista = hämta(&db, &session, &format!("{bas}/leverantorsfakturor")).await;
    assert!(lista.contains("Leverantörsskulder 0,00 kr"), "{lista}");
    assert!(lista.contains(">Avslutade<"));
    assert!(!lista.contains(">Förfallna<"));
}
