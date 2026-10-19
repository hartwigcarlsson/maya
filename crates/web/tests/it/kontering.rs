//! Rätta kontering på en utfärdad kundfaktura i webben, genom routern (#142): panelen, POST:en
//! och fakturan efteråt.

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

/// Ett företag med fakturauppgifterna ifyllda och en utfärdad faktura 1007 till Kund AB med en
/// rad på 3001: sessionen, företagets adress och fakturans id.
async fn utfärdad(db: &TillfälligDatabas) -> (String, String, String) {
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
    posta(
        db,
        &session,
        &format!("{bas}/installningar/uppgifter"),
        "namn=Exempel+AB&gatuadress=Exempelgatan+1&postnummer=12345&postort=Exempelstad\
         &epost=&telefon=&sate=Stockholm&momsregnr=SE556036079301&f_skatt=ja",
    )
    .await;
    posta(
        db,
        &session,
        &format!("{bas}/installningar/fakturauppgifter"),
        "bankgiro=50501055&plusgiro=&iban=&bic=&svarsadress=&standardmeddelande=",
    )
    .await;
    let kund = parameter(
        &plats(
            &posta(
                db,
                &session,
                &format!("{bas}/kunder"),
                "namn=Kund+AB&identitet=&adress=Storgatan+1%0D%0A111+22+Stockholm&epost=\
                 &momsregnr=&betalningsvillkor=30&omvand_skattskyldighet=ingen",
            )
            .await,
        ),
        "k",
    );
    let sparad = plats(
        &posta(
            db,
            &session,
            &format!("{bas}/kundfakturor"),
            &format!(
                "kund={kund}&fakturadatum=2026-03-10&leveransdatum=&forfallodag=&er_referens=\
                 &startnummer=1007\
                 &beskrivning_0=Konsulttimmar&antal_0=10&enhet_0=tim&a_pris_0=950&rabatt_0=\
                 &momssats_0=25&konto_0=3001&handling=spara"
            ),
        )
        .await,
    );
    let f = parameter(&sparad, "f");
    let utfärdad =
        plats(&posta(db, &session, &format!("{bas}/kundfakturor/{f}/utfarda"), "").await);
    assert!(utfärdad.contains("utfardad=1"), "{utfärdad}");
    (session, bas, f)
}

#[tokio::test]
async fn rätta_kontering_öppnas_som_panel_och_bokför_rättelsen_och_ett_nytt_verifikat() {
    let db = Databas::tillfällig().await;
    let (session, bas, f) = utfärdad(&db).await;

    let sida = hämta(&db, &session, &format!("{bas}/kundfakturor?f={f}")).await;
    assert!(
        sida.contains(&format!("{bas}/kundfakturor?f={f}&amp;panel=ratta")),
        "{sida}"
    );
    assert!(sida.contains("Rätta kontering…"));

    let panel = hämta(
        &db,
        &session,
        &format!("{bas}/kundfakturor?f={f}&panel=ratta"),
    )
    .await;
    let panel = &panel[panel.find("Rätta konteringen").unwrap()..];
    assert!(panel.contains(&format!(
        r#"action="{bas}/kundfakturor/{f}/ratta-kontering""#
    )));
    assert!(panel.contains(r#"name="konto_0""#));
    assert!(panel.contains("Konsulttimmar"));
    assert!(panel.contains("V2 vänder V1, V3 bokförs"), "{panel}");
    assert!(panel.contains("Rätta V1"));

    let rättad = plats(
        &posta(
            &db,
            &session,
            &format!("{bas}/kundfakturor/{f}/ratta-kontering"),
            "konto_0=3002&orsak=fel+konto",
        )
        .await,
    );
    assert!(rättad.contains("rattad=1"), "{rättad}");
    let sida = hämta(&db, &session, &rättad).await;
    assert!(sida.contains("Konteringen rättades"));
    let dokument = &sida[sida.find(r#"class="dokument"#).unwrap()..];
    // Fakturan visar det nya verifikatet och kontot som raden nu är bokförd på; PDF:en är kvar.
    assert!(dokument.contains(">V3</a>"), "{dokument}");
    assert!(dokument.contains("<td>3002</td>"), "{dokument}");
    assert!(dokument.contains("kontering rättad"));
    assert!(dokument.contains("Kundfaktura_1007.pdf"));
}

#[tokio::test]
async fn en_rättning_utan_nytt_konto_visar_spärren_i_panelen() {
    let db = Databas::tillfällig().await;
    let (session, bas, f) = utfärdad(&db).await;

    let tillbaka = plats(
        &posta(
            &db,
            &session,
            &format!("{bas}/kundfakturor/{f}/ratta-kontering"),
            "konto_0=3001&orsak=",
        )
        .await,
    );
    assert!(tillbaka.contains("panel=ratta"), "{tillbaka}");
    assert!(tillbaka.contains("fel=kontering"), "{tillbaka}");
    let sida = hämta(&db, &session, &tillbaka).await;
    assert!(
        sida.contains(r#"data-kod="kontering_oforandrad""#),
        "{sida}"
    );
    assert!(sida.contains("Konteringen kunde inte rättas"));
}

#[tokio::test]
async fn den_som_saknar_åtkomst_får_inte_rätta_ett_annat_företags_kontering() {
    let db = Databas::tillfällig().await;
    let (session, _, _) = utfärdad(&db).await;
    let svar = posta(
        &db,
        &session,
        "/foretag/4711/kundfakturor/1/ratta-kontering",
        "konto_0=3002&orsak=",
    )
    .await;
    assert_eq!(svar.status(), StatusCode::FORBIDDEN);
}
