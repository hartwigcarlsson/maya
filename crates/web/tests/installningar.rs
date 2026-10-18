//! Inställningar → Företag: uppgifter, byten, första årets slut och radering, genom routern.

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

async fn skicka(db: &TillfälligDatabas, request: Request<Body>) -> (Response<()>, String) {
    let response = app(db).oneshot(request).await.unwrap();
    let (parts, body) = response.into_parts();
    let body = body.collect().await.unwrap().to_bytes();
    (
        Response::from_parts(parts, ()),
        String::from_utf8_lossy(&body).into_owned(),
    )
}

/// En session som om användaren hade registrerat en passkey.
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
    inloggning::slutför_registrering(db, &Klocka::system(), länk.token(), ny)
        .await
        .unwrap()
        .token()
        .to_string()
}

async fn hämta(db: &TillfälligDatabas, session: &str, path: &str) -> (Response<()>, String) {
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

/// Ett inloggat företag med kalenderåret 2026; sökvägen till företaget.
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

async fn sida(db: &TillfälligDatabas, session: &str, bas: &str) -> String {
    let (response, body) = hämta(db, session, &format!("{bas}/installningar")).await;
    assert_eq!(response.status(), StatusCode::OK, "{body}");
    body
}

#[tokio::test]
async fn sidan_visar_uppgifterna_metoden_momsperioden_och_året() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;

    let body = sida(&db, &session, &bas).await;

    for text in [
        r#"name="namn""#,
        r#"name="gatuadress""#,
        r#"name="postnummer""#,
        r#"name="postort""#,
        r#"name="epost""#,
        r#"name="telefon""#,
        r#"name="sate""#,
        r#"name="momsregnr""#,
        r#"name="f_skatt""#,
        r#"value="Exempel AB""#,
        "SE556036079301",
        "Fakturametoden",
        "Kvartal",
        "2026-12-31",
        r#"name="galler_fran""#,
        "Radera företaget",
    ] {
        assert!(body.contains(text), "{text}: {body}");
    }
    let meny = &body[body.find(r#"class="installningsmeny""#).expect(&body)..];
    assert!(
        meny.contains(&format!(r#"href="{bas}/installningar""#)),
        "{meny}"
    );
    assert!(meny.contains("Kontoplan"), "{meny}");
}

#[tokio::test]
async fn ändrade_uppgifter_sparas_och_syns_i_företagsväljaren() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;

    let response = posta(
        &db,
        &session,
        &format!("{bas}/installningar/uppgifter"),
        "namn=Exempel+Handel+AB&gatuadress=Storgatan+1&postnummer=11122&postort=Stockholm\
         &epost=&telefon=&sate=Stockholm&momsregnr=SE556036079301&f_skatt=ja",
    )
    .await;

    assert_eq!(
        plats(&response),
        format!("{bas}/installningar?sparat=uppgifter")
    );
    let body = sida(&db, &session, &bas).await;
    assert!(body.contains(r#"value="Exempel Handel AB""#), "{body}");
    assert!(body.contains(r#"value="111 22""#), "{body}");
    assert!(body.contains("Exempel Handel AB</summary>"), "{body}");
}

#[tokio::test]
async fn felaktiga_uppgifter_visas_med_koderna_vid_fälten() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;

    let response = posta(
        &db,
        &session,
        &format!("{bas}/installningar/uppgifter"),
        "namn=&postnummer=12&momsregnr=SE1",
    )
    .await;

    let plats = plats(&response);
    assert!(
        plats.starts_with(&format!("{bas}/installningar?")),
        "{plats}"
    );
    let (_, body) = hämta(&db, &session, &plats).await;
    for kod in ["namn_saknas", "postnummer_ogiltigt", "momsregnr_ogiltigt"] {
        assert!(
            body.contains(&format!(r#"data-kod="{kod}""#)),
            "{kod}: {body}"
        );
    }
    assert!(body.contains(r#"value="SE1""#), "{body}");
}

#[tokio::test]
async fn bokföringsmetoden_byts_från_ett_nytt_år_och_nekas_mitt_i_året() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;
    let url = format!("{bas}/installningar/bokforingsmetod");

    let response = posta(
        &db,
        &session,
        &url,
        "galler_fran=2026-07-01&bokforingsmetod=kontant",
    )
    .await;
    let (_, body) = hämta(&db, &session, &plats(&response)).await;
    assert!(body.contains(r#"data-kod="inte_arets_start""#), "{body}");

    let response = posta(
        &db,
        &session,
        &url,
        "galler_fran=2027-01-01&bokforingsmetod=kontant",
    )
    .await;
    assert_eq!(
        plats(&response),
        format!("{bas}/installningar?sparat=bokforingsmetod")
    );
    let body = sida(&db, &session, &bas).await;
    let historik = &body[body.find(r#"id="bokforingsmetod-historik""#).expect(&body)..];
    assert!(historik.contains("2027-01-01"), "{historik}");
    assert!(historik.contains("Kontantmetoden"), "{historik}");
}

#[tokio::test]
async fn momsperioden_byts_från_en_ny_momsperiod() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;
    let url = format!("{bas}/installningar/momsperiod");

    let response = posta(
        &db,
        &session,
        &url,
        "galler_fran=2027-02-01&momsperiod=manad",
    )
    .await;
    let (_, body) = hämta(&db, &session, &plats(&response)).await;
    assert!(
        body.contains(r#"data-kod="inte_momsperiodens_start""#),
        "{body}"
    );

    let response = posta(
        &db,
        &session,
        &url,
        "galler_fran=2027-04-01&momsperiod=manad",
    )
    .await;
    assert_eq!(
        plats(&response),
        format!("{bas}/installningar?sparat=momsperiod")
    );
    let body = sida(&db, &session, &bas).await;
    let historik = &body[body.find(r#"id="momsperiod-historik""#).expect(&body)..];
    assert!(historik.contains("2027-04-01"), "{historik}");
}

#[tokio::test]
async fn första_årets_slut_flyttas() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;
    let url = format!("{bas}/installningar/rakenskapsar");

    let response = posta(&db, &session, &url, "slut=2027-07-31").await;
    let (_, body) = hämta(&db, &session, &plats(&response)).await;
    assert!(body.contains(r#"data-kod="ar_for_langt""#), "{body}");

    let response = posta(&db, &session, &url, "slut=2027-06-30").await;
    assert_eq!(
        plats(&response),
        format!("{bas}/installningar?sparat=rakenskapsar")
    );
    let body = sida(&db, &session, &bas).await;
    assert!(body.contains(r#"value="2027-06-30""#), "{body}");
}

#[tokio::test]
async fn ett_företag_raderas_när_organisationsnumret_bekräftas() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;
    let url = format!("{bas}/radera");

    let response = posta(&db, &session, &url, "bekrafta=556036-0792").await;
    let (_, body) = hämta(&db, &session, &plats(&response)).await;
    assert!(body.contains(r#"data-kod="bekraftelse_fel""#), "{body}");

    let response = posta(&db, &session, &url, "bekrafta=5560360793").await;
    assert_eq!(plats(&response), "/");
    let (response, _) = hämta(&db, &session, &bas).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn ett_företag_med_bokförda_verifikat_kan_inte_raderas() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;
    let verifikat = "datum=2026-03-14&beskrivning=Kontant&konto_0=1930&debet_0=100&kredit_0=\
        &text_0=&konto_1=3001&debet_1=&kredit_1=100&text_1=";
    plats(&posta(&db, &session, &format!("{bas}/verifikat"), verifikat).await);

    let body = sida(&db, &session, &bas).await;
    assert!(body.contains("har bokförda verifikat"), "{body}");
    assert!(
        !body.contains(&format!(r#"action="{bas}/radera""#)),
        "{body}"
    );

    let response = posta(
        &db,
        &session,
        &format!("{bas}/radera"),
        "bekrafta=556036-0793",
    )
    .await;
    let (_, body) = hämta(&db, &session, &plats(&response)).await;
    assert!(
        body.contains(r#"data-kod="har_rakenskapsinformation""#),
        "{body}"
    );
}

#[tokio::test]
async fn inställningarna_för_ett_annat_företag_nekas() {
    let db = Databas::tillfällig().await;
    let (session, _) = företag(&db).await;

    let (response, _) = hämta(&db, &session, "/foretag/4711/installningar").await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = posta(
        &db,
        &session,
        "/foretag/4711/installningar/uppgifter",
        "namn=X",
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = posta(&db, &session, "/foretag/4711/radera", "bekrafta=x").await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}
