//! Inställningar → Företag: fakturauppgifterna och logotypen, genom routern.

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
const GRÄNS: &str = "maya-grans-134";
const PNG: &[u8] = b"\x89PNG\r\n\x1a\nen liten logotyp";

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
    let body = body.collect().await.unwrap().to_bytes().to_vec();
    (Response::from_parts(parts, ()), body)
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

async fn hämta(db: &TillfälligDatabas, session: &str, path: &str) -> (Response<()>, Vec<u8>) {
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
    let (response, body) = hämta(db, session, path).await;
    let body = String::from_utf8(body).unwrap();
    assert_eq!(response.status(), StatusCode::OK, "{body}");
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

async fn posta_fil(
    db: &TillfälligDatabas,
    session: &str,
    path: &str,
    filnamn: &str,
    innehåll: &[u8],
) -> Response<()> {
    let mut body = format!(
        "--{GRÄNS}\r\nContent-Disposition: form-data; name=\"fil\"; \
         filename=\"{filnamn}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(innehåll);
    body.extend_from_slice(format!("\r\n--{GRÄNS}--\r\n").as_bytes());
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

/// Ett inloggat företag; sökvägen till företaget.
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

#[tokio::test]
async fn sidan_visar_formuläret_för_fakturauppgifterna_och_logotypen() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;

    let body = sida(&db, &session, &format!("{bas}/installningar")).await;

    for text in [
        "Fakturor",
        r#"name="bankgiro""#,
        r#"name="plusgiro""#,
        r#"name="iban""#,
        r#"name="bic""#,
        r#"name="ocr""#,
        r#"name="svarsadress""#,
        r#"name="standardmeddelande""#,
        &format!(r#"action="{bas}/installningar/fakturauppgifter""#),
        &format!(r#"action="{bas}/installningar/logotyp""#),
        r#"enctype="multipart/form-data""#,
        r#"accept="image/png,image/jpeg,image/svg+xml""#,
        "Ingen logotyp",
    ] {
        assert!(body.contains(text), "{text}: {body}");
    }
}

#[tokio::test]
async fn fakturauppgifterna_sparas_och_syns_på_sidan() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;

    let response = posta(
        &db,
        &session,
        &format!("{bas}/installningar/fakturauppgifter"),
        "bankgiro=50501055&plusgiro=&iban=se4550000000058398257466&bic=essesess&ocr=ja\
         &svarsadress=Faktura%40exempel.se&standardmeddelande=Hej%21+H%C3%A4r+kommer+fakturan.",
    )
    .await;

    assert_eq!(
        plats(&response),
        format!("{bas}/installningar?sparat=fakturauppgifter")
    );
    let body = sida(&db, &session, &format!("{bas}/installningar")).await;
    for text in [
        r#"value="5050-1055""#,
        r#"value="SE45 5000 0000 0583 9825 7466""#,
        r#"value="ESSESESS""#,
        r#"value="faktura@exempel.se""#,
        "Hej! Här kommer fakturan.</textarea>",
    ] {
        assert!(body.contains(text), "{text}: {body}");
    }
    let ocr = &body[body.find(r#"name="ocr""#).unwrap()..];
    assert!(ocr[..ocr.find('>').unwrap()].contains("checked"), "{ocr}");
}

#[tokio::test]
async fn felaktiga_betalsätt_visas_med_koderna_vid_fälten() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;

    let response = posta(
        &db,
        &session,
        &format!("{bas}/installningar/fakturauppgifter"),
        "bankgiro=5050-1056&plusgiro=1&iban=SE4550000000058398257466&bic=&svarsadress=faktura",
    )
    .await;

    let plats = plats(&response);
    assert!(
        plats.starts_with(&format!("{bas}/installningar?")),
        "{plats}"
    );
    let body = sida(&db, &session, &plats).await;
    for kod in [
        "bankgiro_kontrollsiffra",
        "plusgiro_ogiltigt",
        "bic_saknas",
        "svarsadress_ogiltig",
    ] {
        assert!(
            body.contains(&format!(r#"data-kod="{kod}""#)),
            "{kod}: {body}"
        );
    }
    assert!(body.contains(r#"value="5050-1056""#), "{body}");
}

#[tokio::test]
async fn logotypen_laddas_upp_visas_och_tas_bort() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;

    let response = posta_fil(
        &db,
        &session,
        &format!("{bas}/installningar/logotyp"),
        "logga.png",
        PNG,
    )
    .await;
    assert_eq!(
        plats(&response),
        format!("{bas}/installningar?sparat=logotyp")
    );
    let body = sida(&db, &session, &format!("{bas}/installningar")).await;
    assert!(body.contains("logga.png"), "{body}");
    assert!(
        body.contains(&format!(r#"src="{bas}/installningar/logotyp?v="#)),
        "{body}"
    );

    let (response, fil) = hämta(&db, &session, &format!("{bas}/installningar/logotyp")).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "image/png");
    assert_eq!(
        response.headers()[header::X_CONTENT_TYPE_OPTIONS],
        "nosniff"
    );
    assert!(
        response.headers()[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap()
            .contains("sandbox")
    );
    assert_eq!(fil, PNG);

    let response = posta(
        &db,
        &session,
        &format!("{bas}/installningar/logotyp/ta-bort"),
        "",
    )
    .await;
    assert_eq!(
        plats(&response),
        format!("{bas}/installningar?sparat=logotyp")
    );
    let (response, _) = hämta(&db, &session, &format!("{bas}/installningar/logotyp")).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn en_logotyp_i_ett_annat_format_nekas_med_koden_vid_fältet() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;

    let response = posta_fil(
        &db,
        &session,
        &format!("{bas}/installningar/logotyp"),
        "logga.gif",
        b"GIF89a...",
    )
    .await;

    let plats = plats(&response);
    assert_eq!(
        plats,
        format!("{bas}/installningar?form=logotyp&fel=logotyp_format")
    );
    let body = sida(&db, &session, &plats).await;
    assert!(body.contains(r#"data-kod="logotyp_format""#), "{body}");
}

#[tokio::test]
async fn den_som_saknar_åtkomst_får_inte_se_logotypen() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;
    posta_fil(
        &db,
        &session,
        &format!("{bas}/installningar/logotyp"),
        "logga.png",
        PNG,
    )
    .await;

    let (response, _) = skicka(
        &db,
        Request::get(format!("{bas}/installningar/logotyp"))
            .header(header::HOST, HOST)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
