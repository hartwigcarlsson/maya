//! Sökningen med ⌘K och sökfältet, genom routern.

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
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    response.headers()[header::LOCATION]
        .to_str()
        .unwrap()
        .to_string()
}

async fn lägg_upp(db: &TillfälligDatabas, session: &str, orgnr: &str, namn: &str) -> String {
    posta(
        db,
        session,
        "/foretag",
        &format!(
            "orgnr={orgnr}&namn={namn}&start=2026-01-01&slut=2026-12-31\
             &bokforingsmetod=faktura&momsperiod=kvartal"
        ),
    )
    .await
}

async fn bokför(db: &TillfälligDatabas, session: &str, bas: &str, beskrivning: &str) -> String {
    posta(
        db,
        session,
        &format!("{bas}/verifikat"),
        &format!(
            "datum=2026-03-14&beskrivning={beskrivning}\
             &konto_0=1930&debet_0=1+250&kredit_0=&text_0=\
             &konto_1=3001&debet_1=&kredit_1=1250&text_1="
        ),
    )
    .await
}

/// Anna med Exempel AB (verifikatet "Kaffe") och Annat AB (verifikatet "Hyra"); sökvägarna
/// till företagen och till Exempels verifikat.
async fn två_företag(db: &TillfälligDatabas) -> (String, String, String, String) {
    let session = inloggad(db).await;
    let exempel = lägg_upp(db, &session, "556036-0793", "Exempel+AB").await;
    let annat = lägg_upp(db, &session, "559900-0006", "Annat+AB").await;
    let kaffe = bokför(db, &session, &exempel, "Kaffe").await;
    bokför(db, &session, &annat, "Hyra").await;
    (session, exempel, annat, kaffe)
}

#[tokio::test]
async fn sökfältet_i_toppmenyn_leder_till_sökningen_och_visar_kortkommandot() {
    let db = Databas::tillfällig().await;
    let (session, exempel, _, _) = två_företag(&db).await;

    let (_, body) = hämta(&db, &session, &exempel).await;

    let meny = &body[body.find(r#"class="toppmeny""#).unwrap()..body.find("</header>").unwrap()];
    assert!(
        meny.contains(&format!(r#"action="{exempel}/sok""#)),
        "{meny}"
    );
    assert!(meny.contains(r#"name="q""#), "{meny}");
    assert!(meny.contains("⌘K"), "{meny}");
}

#[tokio::test]
async fn sökningen_hittar_verifikat_i_det_valda_företaget_och_länkar_till_det() {
    let db = Databas::tillfällig().await;
    let (session, exempel, annat, kaffe) = två_företag(&db).await;

    let (response, body) = hämta(&db, &session, &format!("{exempel}/sok?q=V1")).await;

    assert_eq!(response.status(), StatusCode::OK);
    let träffar = &body[body.find(r#"class="sokning""#).expect(&body)..];
    assert!(träffar.contains(&format!(r#"href="{kaffe}""#)), "{träffar}");
    assert!(träffar.contains("Kaffe"), "{träffar}");
    assert!(träffar.contains("1 250,00"), "{träffar}");
    assert!(!träffar.contains("Hyra"), "{träffar}");

    let (_, body) = hämta(&db, &session, &format!("{annat}/sok?q=kaffe")).await;
    let träffar = &body[body.find(r#"class="sokning""#).unwrap()..];
    assert!(!träffar.contains("Kaffe"), "{träffar}");
}

#[tokio::test]
async fn sidor_och_handlingar_nås_från_sökningen() {
    let db = Databas::tillfällig().await;
    let (session, exempel, _, _) = två_företag(&db).await;

    let (_, body) = hämta(&db, &session, &format!("{exempel}/sok")).await;
    let träffar = &body[body.find(r#"class="sokning""#).unwrap()..];
    for (href, text) in [
        (format!("{exempel}/verifikat/nytt"), "Nytt verifikat"),
        (exempel.clone(), "Att göra"),
        (format!("{exempel}/verifikat"), "Verifikat"),
        (format!("{exempel}/rapporter"), "Rapporter"),
        (format!("{exempel}/rapporter/grundbok"), "Grundbok"),
        (format!("{exempel}/rapporter/huvudbok"), "Huvudbok"),
        (format!("{exempel}/installningar"), "Inställningar"),
        (format!("{exempel}/installningar/kontoplan"), "Kontoplan"),
        (format!("{exempel}/underlag"), "Ladda upp underlag"),
        (
            format!("{exempel}/installningar/behandlingshistorik"),
            "Behandlingshistorik",
        ),
        (format!("{exempel}/installningar/atkomst"), "Åtkomst"),
        (
            format!("{exempel}/installningar/systemdokumentation"),
            "Systemdokumentation",
        ),
        ("/foretag/nytt".to_string(), "Lägg upp företag"),
    ] {
        assert!(
            träffar.contains(&format!(r#"href="{href}""#)),
            "{href}: {träffar}"
        );
        assert!(träffar.contains(text), "{text}: {träffar}");
    }

    let (_, body) = hämta(&db, &session, &format!("{exempel}/sok?q=nytt+ver")).await;
    let träffar = &body[body.find(r#"class="sokning""#).unwrap()..];
    assert!(träffar.contains("Nytt verifikat"), "{träffar}");
    assert!(!träffar.contains("Rapporter"), "{träffar}");
}

#[tokio::test]
async fn företagsbyte_görs_från_sökningen() {
    let db = Databas::tillfällig().await;
    let (session, exempel, annat, _) = två_företag(&db).await;

    let (_, body) = hämta(&db, &session, &format!("{exempel}/sok?q=annat")).await;

    let träffar = &body[body.find(r#"class="sokning""#).unwrap()..];
    assert!(träffar.contains("Byt företag"), "{träffar}");
    assert!(träffar.contains(&format!(r#"href="{annat}""#)), "{träffar}");
    assert!(träffar.contains("Annat AB"), "{träffar}");
}

#[tokio::test]
async fn sökningen_i_ett_företag_utan_åtkomst_nekas() {
    let db = Databas::tillfällig().await;
    let (session, _, _, _) = två_företag(&db).await;

    let (response, body) = hämta(&db, &session, "/foretag/4711/sok?q=kaffe").await;

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(!body.contains("Kaffe"), "{body}");
}

#[tokio::test]
async fn paletten_hämtar_verifikaten_från_samma_sökning() {
    let db = Databas::tillfällig().await;
    let (session, exempel, _, kaffe) = två_företag(&db).await;
    let id = exempel.trim_start_matches("/foretag/");

    let (response, body) = hämta(&db, &session, &format!("/api/sok?foretag={id}&q=kaffe")).await;

    assert_eq!(response.status(), StatusCode::OK, "{body}");
    let träffar: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(träffar.as_array().unwrap().len(), 1, "{body}");
    assert_eq!(träffar[0]["href"], kaffe.as_str(), "{body}");

    let (response, body) = hämta(&db, &session, "/api/sok?foretag=4711&q=kaffe").await;
    assert!(!response.status().is_success(), "{body}");
    assert!(!body.contains("Kaffe"), "{body}");
}

// #154 Fakturor, kunder och leverantörer

/// Exempel AB med kundfakturan till Bergström Bygg AB, leverantörsfakturan LF-2026-17 från
/// Kontorsvaror i Norr AB och den arkiverade kunden och leverantören Gamla …: sessionen,
/// sökvägen till företaget och id:n.
struct Fakturor {
    session: String,
    bas: String,
    nummer: String,
    kundfaktura: String,
    kund: String,
    leverantörsfaktura: String,
    gammal_kund: String,
    gammal_leverantör: String,
}

async fn fakturor(db: &TillfälligDatabas) -> Fakturor {
    use maya_core::{kund, leverantor, testhjalp};
    let klocka = Klocka::system();
    let (anna, företag) = testhjalp::fakturaföretag(db).await;
    let session = testhjalp::session(db, &klocka, &anna).await;
    let f = testhjalp::utfärdad_kundfaktura(db, &anna, företag, "Bergström Bygg AB", "2026-03-10")
        .await;
    let kunder = kund::kunder(db, &anna, företag).await.unwrap();
    let norr = testhjalp::leverantör(db, &anna, företag, "Kontorsvaror i Norr AB").await;
    let lf =
        testhjalp::registrerad_leverantörsfaktura(db, &anna, företag, norr, "LF-2026-17", "").await;
    let gammal_kund = testhjalp::kund(db, &anna, företag, "Gamla Kunden AB").await;
    kund::arkivera(db, &klocka, &anna, företag, gammal_kund)
        .await
        .unwrap();
    let gammal_leverantör = testhjalp::leverantör(db, &anna, företag, "Gamla Tryckeriet").await;
    leverantor::arkivera(db, &klocka, &anna, företag, gammal_leverantör)
        .await
        .unwrap();
    Fakturor {
        session,
        bas: format!("/foretag/{företag}"),
        nummer: f.nummer.to_string(),
        kundfaktura: f.id.to_string(),
        kund: kunder[0].id.to_string(),
        leverantörsfaktura: lf.to_string(),
        gammal_kund: gammal_kund.to_string(),
        gammal_leverantör: gammal_leverantör.to_string(),
    }
}

#[tokio::test]
async fn sökningen_hittar_kundfakturan_och_kunden_och_öppnar_dem_i_sina_flikar() {
    let db = Databas::tillfällig().await;
    let f = fakturor(&db).await;

    let (response, body) = hämta(&db, &f.session, &format!("{}/sok?q=bergstr%C3%B6m", f.bas)).await;

    assert_eq!(response.status(), StatusCode::OK);
    let träffar = &body[body.find(r#"class="sokning""#).expect(&body)..];
    assert!(träffar.contains("Kundfakturor"), "{träffar}");
    assert!(
        träffar.contains(&format!(
            r#"href="{}/kundfakturor?f={}""#,
            f.bas, f.kundfaktura
        )),
        "{träffar}"
    );
    assert!(
        träffar.contains(&format!("Faktura {} · Bergström Bygg AB", f.nummer)),
        "{träffar}"
    );
    assert!(träffar.contains("11 875,00"), "{träffar}");
    assert!(
        träffar.contains(&format!(r#"href="{}/kunder?k={}""#, f.bas, f.kund)),
        "{träffar}"
    );
    assert!(!träffar.contains("Kontorsvaror"), "{träffar}");
}

#[tokio::test]
async fn sökningen_hittar_leverantörsfakturan_på_nummer_och_belopp() {
    let db = Databas::tillfällig().await;
    let f = fakturor(&db).await;

    for fråga in ["LF-2026-17", "1+250"] {
        let (_, body) = hämta(&db, &f.session, &format!("{}/sok?q={fråga}", f.bas)).await;
        let träffar = &body[body.find(r#"class="sokning""#).expect(&body)..];
        assert!(
            träffar.contains(&format!(
                r#"href="{}/leverantorsfakturor?f={}""#,
                f.bas, f.leverantörsfaktura
            )),
            "{fråga}: {träffar}"
        );
        assert!(
            träffar.contains("Faktura LF-2026-17 · Kontorsvaror i Norr AB"),
            "{fråga}: {träffar}"
        );
        assert!(!träffar.contains("Bergström"), "{fråga}: {träffar}");
    }
}

#[tokio::test]
async fn arkiverade_kunder_och_leverantörer_hittas_och_märks() {
    let db = Databas::tillfällig().await;
    let f = fakturor(&db).await;

    let (_, body) = hämta(&db, &f.session, &format!("{}/sok?q=gamla", f.bas)).await;

    let träffar = &body[body.find(r#"class="sokning""#).expect(&body)..];
    assert!(
        träffar.contains(&format!(
            r#"href="{}/kunder?k={}&amp;visa=alla""#,
            f.bas, f.gammal_kund
        )),
        "{träffar}"
    );
    assert!(
        träffar.contains(&format!(
            r#"href="{}/leverantorer?l={}&amp;visa=arkiverade""#,
            f.bas, f.gammal_leverantör
        )),
        "{träffar}"
    );
    assert_eq!(träffar.matches(">Arkiverad<").count(), 2, "{träffar}");

    let (_, body) = hämta(&db, &f.session, &format!("{}/sok?q=bergstr%C3%B6m", f.bas)).await;
    let träffar = &body[body.find(r#"class="sokning""#).expect(&body)..];
    assert!(!träffar.contains(">Arkiverad<"), "{träffar}");
}

#[tokio::test]
async fn paletten_hämtar_fakturorna_kunderna_och_leverantörerna_från_samma_sökning() {
    let db = Databas::tillfällig().await;
    let f = fakturor(&db).await;
    let id = f.bas.trim_start_matches("/foretag/");

    let (response, body) = hämta(&db, &f.session, &format!("/api/sok?foretag={id}&q=gamla")).await;

    assert_eq!(response.status(), StatusCode::OK, "{body}");
    let träffar: serde_json::Value = serde_json::from_str(&body).unwrap();
    let grupper: Vec<&str> = träffar
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["grupp"].as_str().unwrap())
        .collect();
    assert_eq!(grupper, ["Kunder", "Leverantörer"], "{body}");
    assert_eq!(träffar[0]["märke"], "Arkiverad", "{body}");
}

#[tokio::test]
async fn fakturornas_och_registrens_sidor_nås_från_sökningen() {
    let db = Databas::tillfällig().await;
    let (session, exempel, _, _) = två_företag(&db).await;

    let (_, body) = hämta(&db, &session, &format!("{exempel}/sok")).await;
    let träffar = &body[body.find(r#"class="sokning""#).unwrap()..];
    for (href, text) in [
        (format!("{exempel}/kundfakturor"), "Kundfakturor"),
        (format!("{exempel}/kundfakturor/ny"), "Ny kundfaktura"),
        (
            format!("{exempel}/kundfakturor/extern/ny"),
            "Registrera extern kundfaktura",
        ),
        (
            format!("{exempel}/leverantorsfakturor"),
            "Leverantörsfakturor",
        ),
        (
            format!("{exempel}/leverantorsfakturor/ny"),
            "Registrera leverantörsfaktura",
        ),
        (format!("{exempel}/kunder"), "Kunder"),
        (format!("{exempel}/kunder/ny"), "Ny kund"),
        (format!("{exempel}/leverantorer"), "Leverantörer"),
        (format!("{exempel}/leverantorer/ny"), "Ny leverantör"),
    ] {
        assert!(
            träffar.contains(&format!(r#"href="{href}""#)),
            "{href}: {träffar}"
        );
        assert!(träffar.contains(text), "{text}: {träffar}");
    }
}
