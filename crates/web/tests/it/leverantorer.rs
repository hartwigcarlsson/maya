//! Leverantörsregistret i webben: lista och detalj, lägga upp, ändra, arkivera och radera.

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
    inloggning::slutför_registrering(db, &Klocka::system(), länk.token(), ny, None)
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

const KONTORSVAROR: &str = "namn=Kontorsvaror+AB&orgnr=556036-0793&gatuadress=Lagerv%C3%A4gen+1\
    &postnummer=41101&postort=G%C3%B6teborg&land=SE&momsregnr=SE556036079301\
    &bankgiro=50501055&plusgiro=&iban=&bic=&standardkonto=5410&omvand_skattskyldighet=ingen";

/// Delen av sidan från `början` till nästa `slut`.
fn del<'a>(body: &'a str, början: &str, slut: &str) -> &'a str {
    let start = body.find(början).expect(body);
    &body[start..start + body[start..].find(slut).unwrap()]
}

/// Lägger upp Kontorsvaror AB; adressen till den i registret och dess id.
async fn kontorsvaror(db: &TillfälligDatabas, session: &str, bas: &str) -> (String, String) {
    let plats = plats(&posta(db, session, &format!("{bas}/leverantorer"), KONTORSVAROR).await);
    let l = plats
        .split("l=")
        .nth(1)
        .and_then(|r| r.split('&').next())
        .expect(&plats)
        .to_string();
    (plats, l)
}

#[tokio::test]
async fn en_ny_leverantör_läggs_upp_och_visas_i_listan_och_som_dokument() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;

    let (plats, _) = kontorsvaror(&db, &session, &bas).await;
    assert!(
        plats.starts_with(&format!("{bas}/leverantorer?l=")),
        "{plats}"
    );
    let (response, body) = hämta(&db, &session, &plats).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert!(del(&body, r#"class="verifikatlista""#, "</ul>").contains("Kontorsvaror AB"));
    let dokument = del(&body, r#"class="dokument""#, "</article>");
    for text in [
        "Kontorsvaror AB",
        "556036-0793",
        "411 01 Göteborg",
        "5050-1055",
        "5410",
        "upplagd",
    ] {
        assert!(dokument.contains(text), "{text}: {dokument}");
    }
    assert!(body.contains("Sparat."));
}

#[tokio::test]
async fn ett_formulär_med_fel_kommer_tillbaka_med_det_som_skrevs_och_felet_vid_fältet() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;
    let (_, ny) = hämta(&db, &session, &format!("{bas}/leverantorer/ny")).await;
    assert!(
        ny.contains(&format!(r#"action="{bas}/leverantorer""#)),
        "{ny}"
    );

    let fel = KONTORSVAROR.replace("bankgiro=50501055", "bankgiro=50501056");
    let plats = plats(&posta(&db, &session, &format!("{bas}/leverantorer"), &fel).await);
    assert!(
        plats.starts_with(&format!("{bas}/leverantorer/ny?")),
        "{plats}"
    );
    let (_, body) = hämta(&db, &session, &plats).await;

    assert!(
        body.contains(r#"data-kod="bankgiro_kontrollsiffra""#),
        "{body}"
    );
    assert!(body.contains(r#"value="Kontorsvaror AB""#));
    let (_, lista) = hämta(&db, &session, &format!("{bas}/leverantorer")).await;
    assert!(!lista.contains("Kontorsvaror AB"));
}

#[tokio::test]
async fn en_leverantör_ändras_i_sitt_dokument() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;
    let (_, l) = kontorsvaror(&db, &session, &bas).await;

    let (_, formulär) = hämta(&db, &session, &format!("{bas}/leverantorer?l={l}&andra=1")).await;
    assert!(
        formulär.contains(&format!(r#"action="{bas}/leverantorer/{l}""#)),
        "{formulär}"
    );
    let ändrad = KONTORSVAROR.replace("postort=G%C3%B6teborg", "postort=M%C3%B6lndal");
    let efter = plats(&posta(&db, &session, &format!("{bas}/leverantorer/{l}"), &ändrad).await);
    let (_, body) = hämta(&db, &session, &efter).await;

    let dokument = del(&body, r#"class="dokument""#, "</article>");
    assert!(dokument.contains("411 01 Mölndal"), "{dokument}");
    assert!(
        dokument.contains("postort: ”Göteborg” → ”Mölndal”"),
        "{dokument}"
    );
}

#[tokio::test]
async fn en_arkiverad_leverantör_visas_inte_bland_de_aktiva_men_bland_alla() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;
    let (_, l) = kontorsvaror(&db, &session, &bas).await;

    let (_, fråga) = hämta(
        &db,
        &session,
        &format!("{bas}/leverantorer?l={l}&arkivera=1"),
    )
    .await;
    assert!(fråga.contains(&format!(r#"action="{bas}/leverantorer/{l}/arkivera""#)));
    let efter = plats(
        &posta(
            &db,
            &session,
            &format!("{bas}/leverantorer/{l}/arkivera"),
            "",
        )
        .await,
    );
    let (_, aktiva) = hämta(&db, &session, &format!("{bas}/leverantorer")).await;
    // Samma filter som kundregistret: `visa=alla` tar med de arkiverade.
    let (_, alla) = hämta(&db, &session, &format!("{bas}/leverantorer?visa=alla")).await;
    let (_, dokument) = hämta(&db, &session, &efter).await;

    assert!(!del(&aktiva, r#"class="verifikatlista""#, "</ul>").contains("Kontorsvaror AB"));
    assert!(del(&alla, r#"class="verifikatlista""#, "</ul>").contains("Kontorsvaror AB"));
    assert!(del(&dokument, r#"class="dokument""#, "</article>").contains("Arkiverad"));
}

#[tokio::test]
async fn en_leverantör_utan_fakturor_raderas_efter_en_bekräftelse() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;
    let (_, l) = kontorsvaror(&db, &session, &bas).await;

    let (_, fråga) = hämta(&db, &session, &format!("{bas}/leverantorer?l={l}&radera=1")).await;
    assert!(fråga.contains(&format!(r#"action="{bas}/leverantorer/{l}/radera""#)));
    let efter = plats(&posta(&db, &session, &format!("{bas}/leverantorer/{l}/radera"), "").await);
    let (_, body) = hämta(&db, &session, &efter).await;

    assert!(body.contains("Leverantören raderades"), "{body}");
    assert!(!body.contains("Kontorsvaror AB"));
}

#[tokio::test]
async fn leverantörer_i_ett_annat_företag_nekas() {
    let db = Databas::tillfällig().await;
    let (session, _) = företag(&db).await;

    let (response, _) = hämta(&db, &session, "/foretag/4711/leverantorer").await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = posta(&db, &session, "/foretag/4711/leverantorer", KONTORSVAROR).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = posta(&db, &session, "/foretag/4711/leverantorer/1/radera", "").await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}
