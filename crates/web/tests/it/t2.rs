//! T2-utkastet i webben, genom routern (#150): det står under Utkast i Att göra med länken till
//! fliken Verifikat, visas där som ett dokument utan formulär, och användaren bokför det med
//! Bokför T2. Det går inte att ändra eller radera för hand.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use http_body_util::BodyExt;
use maya_core::config::Config;
use maya_core::{Databas, Klocka, TillfälligDatabas, t2, testhjalp};
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

fn efter<'a>(sida: &'a str, text: &str) -> &'a str {
    &sida[sida
        .find(text)
        .unwrap_or_else(|| panic!("{text} saknas i {sida}"))..]
}

#[tokio::test]
async fn t2_utkastet_står_under_utkast_och_bokförs_från_dokumentet() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag_med_metod(&db, "kontant").await;
    testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Kund AB", "2026-03-10").await;
    let nyårsdagen = Klocka::fast("2027-01-01T03:00:00Z".parse().unwrap());
    let [(_, v)] = t2::kör(&db, &nyårsdagen).await.unwrap()[..] else {
        panic!("ett T2-utkast");
    };
    let session = testhjalp::session(&db, &Klocka::system(), &anna).await;
    let bas = format!("/foretag/{id}");

    // Att göra: under Utkast, i december, med länken till fliken Verifikat.
    let sida = hämta(&db, &session, &bas).await;
    let kort = efter(&sida, &format!(r#"id="t2-{v}""#));
    assert!(kort.contains("T2-utkast"), "{kort}");
    assert!(kort.contains("Obetalda fakturor vid årets slut"), "{kort}");
    assert!(kort.contains("11 875,00"), "{kort}");
    let sida = hämta(&db, &session, &format!("{bas}?oppna=t2-{v}")).await;
    let länk = format!("{bas}/verifikat?v={v}");
    assert!(sida.contains(&format!(r#"href="{länk}""#)), "{sida}");
    assert!(sida.contains("Granska och bokför"), "{sida}");

    // Fliken Verifikat visar utkastet som ett dokument med Bokför T2 och inget formulär.
    let sida = hämta(&db, &session, &länk).await;
    let action = format!("{bas}/verifikat/{v}/bokfor-t2");
    assert!(sida.contains(&format!(r#"action="{action}""#)), "{sida}");
    assert!(sida.contains("kan inte ändras eller raderas"), "{sida}");
    assert!(!sida.contains("Spara utkast"), "{sida}");
    assert!(!sida.contains("Radera utkast"), "{sida}");

    // Att radera det för hand nekas.
    let r = posta(&db, &session, &format!("{bas}/verifikat/{v}/radera"), "").await;
    assert_eq!(r.status(), StatusCode::NOT_FOUND);

    let plats = plats(&posta(&db, &session, &action, "").await);
    assert!(
        plats.starts_with(&format!("{bas}/verifikat?v={v}&t2_bokfort=V")),
        "{plats}"
    );
    let sida = hämta(&db, &session, &plats).await;
    assert!(sida.contains("T2-verifikatet bokfördes som V"), "{sida}");
    assert!(sida.contains("vänds inte i det nya året"), "{sida}");
    assert!(!sida.contains("Rätta…"), "{sida}");
    assert!(!sida.contains(r#"id="t2-"#), "{sida}");
}
