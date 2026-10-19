//! Kundregistret i webben, genom routern: lista och detalj, lägga upp, ändra, arkivera och
//! radera en kund.

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
const KUND_AB: &str = "namn=Kund+AB&identitet=5560360793&adress=Storgatan+1%0D%0A111+22+Stockholm\
    &epost=faktura%40kund.se&momsregnr=SE556036079301&betalningsvillkor=30\
    &omvand_skattskyldighet=bygg";

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

/// Lägger upp Kund AB och ger adressen till kundens detalj.
async fn kund_ab(db: &TillfälligDatabas, session: &str, bas: &str) -> String {
    plats(&posta(db, session, &format!("{bas}/kunder"), KUND_AB).await)
}

/// Delen av sidan från detaljen och framåt.
fn detalj(sida: &str) -> &str {
    &sida[sida.find(r#"class="verifikat-detalj""#).expect(sida)..]
}

/// Listan till vänster.
fn listan(sida: &str) -> &str {
    let start = sida.find(r#"class="verifikatlista""#).expect(sida);
    let slut = sida.find(r#"class="verifikat-detalj""#).expect(sida);
    &sida[start..slut]
}

#[tokio::test]
async fn en_upplagd_kund_visas_i_listan_och_som_detalj() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;

    let tom = hämta(&db, &session, &format!("{bas}/kunder")).await;
    assert!(tom.contains("Kundregistret"), "{tom}");
    assert!(tom.contains("Inga aktiva kunder"), "{tom}");
    let ny = hämta(&db, &session, &format!("{bas}/kunder/ny")).await;
    assert!(ny.contains(&format!(r#"action="{bas}/kunder""#)), "{ny}");
    assert!(ny.contains(r#"name="identitet""#), "{ny}");

    let plats = kund_ab(&db, &session, &bas).await;
    assert!(plats.starts_with(&format!("{bas}/kunder?k=")), "{plats}");
    let sida = hämta(&db, &session, &plats).await;

    assert!(listan(&sida).contains("Kund AB"), "{sida}");
    let d = detalj(&sida);
    assert!(d.contains("Kunden lades upp och loggades."), "{d}");
    assert!(d.contains("556036-0793"), "{d}");
    assert!(d.contains("faktura@kund.se"), "{d}");
    assert!(d.contains("SE556036079301"), "{d}");
    assert!(d.contains("30 dagar"), "{d}");
    assert!(d.contains("Bygg inom Sverige"), "{d}");
    assert!(d.contains("Storgatan 1"), "{d}");
}

#[tokio::test]
async fn en_felaktig_kund_kommer_tillbaka_till_formuläret_med_felet_vid_fältet() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;

    let plats = plats(
        &posta(
            &db,
            &session,
            &format!("{bas}/kunder"),
            "namn=Kund+AB&identitet=556036-0794&epost=faktura%40&betalningsvillkor=30\
             &omvand_skattskyldighet=ingen",
        )
        .await,
    );

    assert!(plats.starts_with(&format!("{bas}/kunder/ny?")), "{plats}");
    let sida = hämta(&db, &session, &plats).await;
    assert!(
        sida.contains(r#"data-kod="identitet_kontrollsiffra""#),
        "{sida}"
    );
    assert!(sida.contains(r#"data-kod="epost_ogiltig""#), "{sida}");
    assert!(sida.contains(r#"value="Kund AB""#), "{sida}");
    let registret = hämta(&db, &session, &format!("{bas}/kunder")).await;
    assert!(registret.contains("Inga aktiva kunder"), "{registret}");
}

#[tokio::test]
async fn en_ändrad_kund_visar_de_nya_uppgifterna() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;
    let k = parameter(&kund_ab(&db, &session, &bas).await, "k");

    let formulär = hämta(&db, &session, &format!("{bas}/kunder?k={k}&andra=1")).await;
    assert!(
        formulär.contains(&format!(r#"action="{bas}/kunder/{k}""#)),
        "{formulär}"
    );
    assert!(
        formulär.contains(r#"value="faktura@kund.se""#),
        "{formulär}"
    );

    let plats = plats(
        &posta(
            &db,
            &session,
            &format!("{bas}/kunder/{k}"),
            &KUND_AB.replace("betalningsvillkor=30", "betalningsvillkor=10"),
        )
        .await,
    );
    let sida = hämta(&db, &session, &plats).await;
    let d = detalj(&sida);
    assert!(d.contains("Sparat."), "{d}");
    assert!(d.contains("10 dagar"), "{d}");

    let fel = plats_för(
        &db,
        &session,
        &format!("{bas}/kunder/{k}"),
        "namn=&betalningsvillkor=30&omvand_skattskyldighet=ingen",
    )
    .await;
    assert!(fel.contains("andra=1"), "{fel}");
    let sida = hämta(&db, &session, &fel).await;
    assert!(sida.contains(r#"data-kod="namn_saknas""#), "{sida}");
}

async fn plats_för(db: &TillfälligDatabas, session: &str, path: &str, body: &str) -> String {
    plats(&posta(db, session, path, body).await)
}

#[tokio::test]
async fn en_arkiverad_kund_visas_bara_under_alla() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;
    let k = parameter(&kund_ab(&db, &session, &bas).await, "k");

    let fråga = hämta(&db, &session, &format!("{bas}/kunder?k={k}&arkivera=1")).await;
    assert!(fråga.contains("Arkivera kunden?"), "{fråga}");
    let plats = plats_för(&db, &session, &format!("{bas}/kunder/{k}/arkivera"), "").await;

    let sida = hämta(&db, &session, &plats).await;
    assert!(detalj(&sida).contains("Kunden arkiverades"), "{sida}");
    assert!(detalj(&sida).contains("Arkiverad"), "{sida}");
    let aktiva = hämta(&db, &session, &format!("{bas}/kunder")).await;
    assert!(!listan(&aktiva).contains("Kund AB"), "{aktiva}");
    let alla = hämta(&db, &session, &format!("{bas}/kunder?visa=alla")).await;
    assert!(listan(&alla).contains("Kund AB"), "{alla}");
    assert!(listan(&alla).contains("Arkiverad"), "{alla}");
}

#[tokio::test]
async fn en_kund_utan_fakturor_raderas_efter_en_fråga() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;
    let k = parameter(&kund_ab(&db, &session, &bas).await, "k");

    let fråga = hämta(&db, &session, &format!("{bas}/kunder?k={k}&radera=1")).await;
    assert!(fråga.contains("Radera kunden?"), "{fråga}");
    assert!(
        fråga.contains(&format!(r#"action="{bas}/kunder/{k}/radera""#)),
        "{fråga}"
    );
    let plats = plats_för(&db, &session, &format!("{bas}/kunder/{k}/radera"), "").await;

    assert_eq!(plats, format!("{bas}/kunder?raderad=1"));
    let sida = hämta(&db, &session, &plats).await;
    assert!(
        sida.contains("Kunden raderades och raderingen loggades."),
        "{sida}"
    );
    assert!(sida.contains("Inga aktiva kunder"), "{sida}");
}

#[tokio::test]
async fn ett_annat_företags_register_nås_inte() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;
    let k = parameter(&kund_ab(&db, &session, &bas).await, "k");

    for (path, body) in [
        ("/foretag/999/kunder".to_string(), KUND_AB),
        (format!("/foretag/999/kunder/{k}"), KUND_AB),
        (format!("/foretag/999/kunder/{k}/arkivera"), ""),
        (format!("/foretag/999/kunder/{k}/radera"), ""),
    ] {
        let response = posta(&db, &session, &path, body).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path}");
    }
    let sida = hämta(&db, &session, &format!("{bas}/kunder?k=999")).await;
    assert!(detalj(&sida).contains("Välj en kund"), "{sida}");
}
