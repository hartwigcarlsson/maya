//! Att lägga upp ett företag och byta företag i webben, genom routern.

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

fn formulär(orgnr: &str, namn: &str) -> String {
    format!(
        "orgnr={orgnr}&namn={}&start=2026-01-01&slut=2026-12-31&bokforingsmetod=faktura&momsperiod=kvartal",
        namn.replace(' ', "+")
    )
}

async fn lägg_upp(db: &TillfälligDatabas, session: &str, body: String) -> Response<()> {
    skicka(
        db,
        Request::post("/foretag")
            .header(header::HOST, HOST)
            .header(header::ORIGIN, ORIGIN)
            .header(header::COOKIE, format!("maya_session={session}"))
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
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

#[tokio::test]
async fn utan_företag_leder_startsidan_till_formuläret() {
    let db = Databas::tillfällig().await;
    let session = inloggad(&db).await;

    let (_, body) = hämta(&db, &session, "/").await;

    assert!(body.contains(r#"href="/foretag/nytt""#), "{body}");
}

#[tokio::test]
async fn formuläret_har_fälten_för_ett_nytt_företag() {
    let db = Databas::tillfällig().await;
    let session = inloggad(&db).await;

    let (response, body) = hämta(&db, &session, "/foretag/nytt").await;

    assert_eq!(response.status(), StatusCode::OK);
    for fält in [
        r#"name="orgnr""#,
        r#"name="namn""#,
        r#"name="start""#,
        r#"name="slut""#,
        r#"name="bokforingsmetod""#,
        r#"name="momsperiod""#,
        r#"value="ej_momsregistrerad""#,
        "Ej momsregistrerad",
        r#"value="kontant""#,
    ] {
        assert!(body.contains(fält), "{fält}: {body}");
    }
}

#[tokio::test]
async fn ett_giltigt_formulär_lägger_upp_företaget_och_användaren_hamnar_i_det() {
    let db = Databas::tillfällig().await;
    let session = inloggad(&db).await;

    let response = lägg_upp(&db, &session, formulär("556036-0793", "Exempel AB")).await;

    let plats = plats(&response);
    assert!(plats.starts_with("/foretag/"), "{plats}");
    let (response, body) = hämta(&db, &session, &plats).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(body.contains("Exempel AB"), "{body}");
    for flik in ["Att göra", "Verifikat", "Rapporter", "Inställningar"] {
        assert!(body.contains(flik), "{flik}: {body}");
    }
    let (_, start) = hämta(&db, &session, "/").await;
    assert!(
        start.contains(&plats),
        "startsidan leder till företaget: {start}"
    );
}

#[tokio::test]
async fn ett_felaktigt_organisationsnummer_nekas_av_servern_med_samma_kod_som_formuläret() {
    let db = Databas::tillfällig().await;
    let session = inloggad(&db).await;

    for (orgnr, kod) in [
        ("556036-079", "orgnr_langd"),
        ("556036-0794", "orgnr_kontrollsiffra"),
        ("800101-0019", "orgnr_tredje_siffran"),
    ] {
        let response = lägg_upp(&db, &session, formulär(orgnr, "Exempel AB")).await;

        let plats = plats(&response);
        assert!(plats.starts_with("/foretag/nytt?"), "{plats}");
        let (_, body) = hämta(&db, &session, &plats).await;
        assert!(
            body.contains(&format!(r#"data-kod="{kod}""#)),
            "{kod}: {body}"
        );
        assert!(body.contains(r#"value="Exempel AB""#), "{body}");
    }
    let (_, start) = hämta(&db, &session, "/").await;
    assert!(start.contains(r#"href="/foretag/nytt""#), "{start}");
}

#[tokio::test]
async fn ett_organisationsnummer_som_redan_finns_visas_med_hänvisning_till_inbjudan() {
    let db = Databas::tillfällig().await;
    let session = inloggad(&db).await;
    lägg_upp(&db, &session, formulär("556036-0793", "Exempel AB")).await;

    let response = lägg_upp(&db, &session, formulär("5560360793", "Exempel AB")).await;

    let (_, body) = hämta(&db, &session, &plats(&response)).await;
    assert!(body.contains(r#"data-kod="orgnr_finns""#), "{body}");
    assert!(body.contains("556036-0793 finns redan"), "{body}");
    assert!(body.contains("inbjudan"), "{body}");
}

#[tokio::test]
async fn företagsväljaren_listar_företagen_och_byter_företag() {
    let db = Databas::tillfällig().await;
    let session = inloggad(&db).await;
    let a = plats(&lägg_upp(&db, &session, formulär("556036-0793", "Exempel AB")).await);
    let b = plats(&lägg_upp(&db, &session, formulär("212000-0142", "Annat AB")).await);

    let (_, body) = hämta(&db, &session, &a).await;

    let väljare = &body[body.find("foretagsvaljare").expect(&body)..];
    assert!(väljare.contains(&format!(r#"href="{a}""#)), "{väljare}");
    assert!(väljare.contains(&format!(r#"href="{b}""#)), "{väljare}");
    assert!(väljare.contains("Annat AB"), "{väljare}");
    let (_, body) = hämta(&db, &session, &b).await;
    let rubrik = &body[body.find("<main").unwrap()..body.find("</main>").unwrap()];
    assert!(rubrik.contains("Annat AB"), "{rubrik}");
    assert!(!rubrik.contains("Exempel AB"), "{rubrik}");
}

#[tokio::test]
async fn ett_företag_utan_åtkomst_nekas() {
    let db = Databas::tillfällig().await;
    let session = inloggad(&db).await;

    let (response, body) = hämta(&db, &session, "/foretag/4711").await;

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(body.contains("inte åtkomst"), "{body}");
}

#[tokio::test]
async fn en_utloggad_kan_inte_lägga_upp_ett_företag() {
    let db = Databas::tillfällig().await;
    inloggad(&db).await;

    let response = lägg_upp(&db, "fel", formulär("556036-0793", "Exempel AB")).await;

    assert_eq!(plats(&response), "/");
}

#[tokio::test]
async fn ett_formulär_från_ett_annat_origin_avvisas() {
    let db = Databas::tillfällig().await;
    let session = inloggad(&db).await;

    let (response, _) = skicka(
        &db,
        Request::post("/foretag")
            .header(header::HOST, HOST)
            .header(header::ORIGIN, "https://angripare.example.com")
            .header(header::COOKIE, format!("maya_session={session}"))
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(formulär("556036-0793", "Exempel AB")))
            .unwrap(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}
