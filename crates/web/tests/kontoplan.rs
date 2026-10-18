//! Kontoplanen under Inställningar, genom routern.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use http_body_util::BodyExt;
use maya_core::aktor::Aktör;
use maya_core::config::Config;
use maya_core::inloggning::{self, NyPasskey};
use maya_core::{Databas, Klocka, TillfälligDatabas, kontoplan};
use maya_domain::kontoplan::Kontonummer;
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

/// Anna med en session och ett företag. Ger sessionen och sökvägen till kontoplanen.
async fn företag(db: &TillfälligDatabas) -> (String, String) {
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
    let session = inloggning::slutför_registrering(db, &Klocka::system(), länk.token(), ny, None)
        .await
        .unwrap()
        .token()
        .to_string();
    let (response, _) = posta(
        db,
        &session,
        "/foretag",
        "orgnr=556036-0793&namn=Exempel+AB&start=2026-01-01&slut=2026-12-31\
         &bokforingsmetod=faktura&momsperiod=kvartal",
    )
    .await;
    let företag = plats(&response);
    (session, format!("{företag}/installningar/kontoplan"))
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

async fn posta(
    db: &TillfälligDatabas,
    session: &str,
    path: &str,
    body: &str,
) -> (Response<()>, String) {
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
}

fn plats(response: &Response<()>) -> String {
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    response.headers()[header::LOCATION]
        .to_str()
        .unwrap()
        .to_string()
}

/// Kontonumren i tabellen, i ordning.
fn rader(body: &str) -> Vec<u16> {
    body.split(r#"data-konto=""#)
        .skip(1)
        .map(|rest| rest[..4].parse().unwrap())
        .collect()
}

/// Sökvägen till kontoplanen och företagets id ur den.
fn företagets_bas(kontoplan: &str) -> String {
    kontoplan
        .trim_end_matches("/installningar/kontoplan")
        .to_string()
}

async fn konton(db: &Databas, session: &str) -> Vec<kontoplan::Konto> {
    let inloggad = inloggning::session(db, &Klocka::system(), session)
        .await
        .unwrap()
        .unwrap();
    let mina = maya_core::foretag::mina_företag(db, &inloggad.aktör)
        .await
        .unwrap();
    kontoplan::konton(db, &inloggad.aktör, mina[0].id)
        .await
        .unwrap()
}

#[tokio::test]
async fn fliken_aktiva_visar_alla_aktiva_konton_per_kontoklass() {
    let db = Databas::tillfällig().await;
    let (session, sida) = företag(&db).await;

    let (response, body) = hämta(&db, &session, &sida).await;

    assert_eq!(response.status(), StatusCode::OK);
    let aktiva: Vec<u16> = konton(&db, &session)
        .await
        .iter()
        .filter(|k| k.aktiv)
        .map(|k| k.nummer.get())
        .collect();
    assert!(aktiva.len() > 300, "{}", aktiva.len());
    assert_eq!(rader(&body), aktiva);
    for rubrik in [
        "Klass 1 · Tillgångar",
        "Klass 8 · ",
        "Konto",
        "Namn",
        "Aktiv",
        "INK2-rad",
    ] {
        assert!(body.contains(rubrik), "{rubrik}");
    }
    assert!(body.contains("2.26"), "INK2-raden för 1930");
}

#[tokio::test]
async fn fliken_alla_visar_hela_kontoplanen_och_inaktiva_bara_de_inaktiva() {
    let db = Databas::tillfällig().await;
    let (session, sida) = företag(&db).await;
    let alla = konton(&db, &session).await;

    let (_, body_alla) = hämta(&db, &session, &format!("{sida}?visa=alla")).await;
    let (_, body_inaktiva) = hämta(&db, &session, &format!("{sida}?visa=inaktiva")).await;

    assert!(alla.len() > 1200);
    assert_eq!(rader(&body_alla).len(), alla.len());
    let inaktiva: Vec<u16> = alla
        .iter()
        .filter(|k| !k.aktiv)
        .map(|k| k.nummer.get())
        .collect();
    assert_eq!(rader(&body_inaktiva), inaktiva);
}

#[tokio::test]
async fn sökningen_matchar_nummerprefix_och_säger_hur_många_inaktiva_som_också_matchar() {
    let db = Databas::tillfällig().await;
    let (session, sida) = företag(&db).await;

    let (_, body) = hämta(&db, &session, &format!("{sida}?q=151")).await;
    let (_, alla) = hämta(&db, &session, &format!("{sida}?q=151&visa=alla")).await;

    // BAS 2026: grundkontona 1510, 1513 och 1519 är aktiva, 1511, 1512, 1516 och 1518 inte.
    assert_eq!(rader(&body), [1510, 1513, 1519]);
    assert!(body.contains("4 inaktiva konton matchar också"), "{body}");
    assert!(body.contains("q=151&amp;visa=alla"), "{body}");
    assert_eq!(rader(&alla), [1510, 1511, 1512, 1513, 1516, 1518, 1519]);
    assert!(!alla.contains("inaktiva konton matchar också"));
}

#[tokio::test]
async fn sökningen_matchar_namnet_och_bas_namnet() {
    let db = Databas::tillfällig().await;
    let (session, sida) = företag(&db).await;
    let bas = företagets_bas(&sida);
    posta(
        &db,
        &session,
        &format!("{bas}/kontoplan/1930/namn"),
        "namn=Bankkonto+SEB",
    )
    .await;

    let (_, på_namn) = hämta(&db, &session, &format!("{sida}?visa=alla&q=mobiltelefon")).await;
    let (_, på_bas_namn) = hämta(&db, &session, &format!("{sida}?q=f%C3%B6retagskonto")).await;

    assert_eq!(rader(&på_namn), [6212]);
    assert!(rader(&på_bas_namn).contains(&1930), "{på_bas_namn}");
}

#[tokio::test]
async fn snedstrecket_är_genvägen_till_sökfältet() {
    let db = Databas::tillfällig().await;
    let (session, sida) = företag(&db).await;

    let (_, body) = hämta(&db, &session, &sida).await;

    assert!(body.contains(r#"id="kontosok""#), "{body}");
    assert!(body.contains("<kbd>/</kbd>"));
    assert!(body.contains("kontosok"), "skriptet som fokuserar fältet");
}

#[tokio::test]
async fn filtret_egna_visar_bara_egna_konton() {
    let db = Databas::tillfällig().await;
    let (session, sida) = företag(&db).await;
    let bas = företagets_bas(&sida);

    let (response, _) = posta(
        &db,
        &session,
        &format!("{bas}/kontoplan"),
        "nummer=6542&namn=Molnt%C3%A4nster",
    )
    .await;
    let tillbaka = plats(&response);
    let (_, egna) = hämta(&db, &session, &format!("{sida}?visa=alla&egna=1")).await;
    let (_, efter) = hämta(&db, &session, &tillbaka).await;

    assert_eq!(rader(&egna), [6542]);
    assert!(
        efter.contains("6542 Molntänster är skapat och aktivt."),
        "{efter}"
    );
}

#[tokio::test]
async fn filtret_nya_i_bas_visar_konton_som_en_ny_bas_version_lade_till() {
    let db = Databas::tillfällig().await;
    let (session, sida) = företag(&db).await;
    kontoplan::bas_tillägg(
        &db,
        &Klocka::system(),
        "BAS 2027",
        &[(Kontonummer::new(5425).unwrap(), "Molntjänster och licenser")],
    )
    .await
    .unwrap();

    let (_, body) = hämta(&db, &session, &format!("{sida}?visa=alla&nya=1")).await;

    assert_eq!(rader(&body), [5425]);
    assert!(body.contains("Nytt i BAS 2027"), "{body}");
}

#[tokio::test]
async fn ett_namnbyte_på_plats_sparas_och_syns_i_historiken() {
    let db = Databas::tillfällig().await;
    let (session, sida) = företag(&db).await;
    let bas = företagets_bas(&sida);

    let (response, _) = posta(
        &db,
        &session,
        &format!("{bas}/kontoplan/1930/namn"),
        "namn=Bankkonto+SEB&tillbaka=q%3D193",
    )
    .await;
    let tillbaka = plats(&response);
    let (_, body) = hämta(&db, &session, &tillbaka).await;
    let (_, panel) = hämta(&db, &session, &format!("{sida}?konto=1930")).await;

    assert!(tillbaka.starts_with(&sida), "{tillbaka}");
    assert!(tillbaka.contains("q=193"), "urvalet följer med: {tillbaka}");
    assert!(body.contains("1930 heter nu Bankkonto SEB."), "{body}");
    assert!(body.contains(r#"value="Bankkonto SEB""#));
    for text in [
        "Behandlingshistorik",
        "Namn i BAS",
        "Företagskonto",
        "konto namnbytt",
        "anna@example.se (webb)",
        "Maya",
        "2.26",
        "Saldo",
    ] {
        assert!(panel.contains(text), "{text}: {panel}");
    }
}

#[tokio::test]
async fn brytaren_aktiverar_och_inaktiverar_och_loggar() {
    let db = Databas::tillfällig().await;
    let (session, sida) = företag(&db).await;
    let bas = företagets_bas(&sida);

    let (på, _) = posta(
        &db,
        &session,
        &format!("{bas}/kontoplan/6212/aktiv"),
        "aktiv=1",
    )
    .await;
    let (_, body) = hämta(&db, &session, &plats(&på)).await;
    assert!(rader(&body).contains(&6212));
    assert!(body.contains("6212 Mobiltelefon är aktivt"), "{body}");

    let (av, _) = posta(
        &db,
        &session,
        &format!("{bas}/kontoplan/6212/aktiv"),
        "aktiv=0",
    )
    .await;
    let (_, body) = hämta(&db, &session, &plats(&av)).await;
    assert!(!rader(&body).contains(&6212));

    let (_, panel) = hämta(&db, &session, &format!("{sida}?visa=alla&konto=6212")).await;
    assert!(panel.contains("konto aktiverat"), "{panel}");
    assert!(panel.contains("konto inaktiverat"), "{panel}");
}

#[tokio::test]
async fn kontot_2099_kan_inte_inaktiveras_och_säger_vilket_flöde_som_använder_det() {
    let db = Databas::tillfällig().await;
    let (session, sida) = företag(&db).await;
    let bas = företagets_bas(&sida);

    let (_, body) = hämta(&db, &session, &format!("{sida}?q=2099")).await;
    assert!(
        body.contains("Används av flödet Preliminär ingående balans"),
        "{body}"
    );

    let (response, _) = posta(
        &db,
        &session,
        &format!("{bas}/kontoplan/2099/aktiv"),
        "aktiv=0",
    )
    .await;
    let (_, body) = hämta(&db, &session, &plats(&response)).await;

    assert!(
        body.contains("2099 kan inte inaktiveras. Används av flödet Preliminär ingående balans"),
        "{body}"
    );
    assert!(rader(&body).contains(&2099));
}

#[tokio::test]
async fn nytt_konto_visar_klass_kontogrupp_och_ink2_rad_ur_numret() {
    let db = Databas::tillfällig().await;
    let (session, sida) = företag(&db).await;

    let (_, body) = hämta(&db, &session, &format!("{sida}?nytt=1&nummer=1937&namn=")).await;

    for text in [
        "Nytt konto",
        "Klass 1",
        "Tillgångar",
        "grupp 19 Kassa och bank",
        "2.26",
    ] {
        assert!(body.contains(text), "{text}: {body}");
    }
}

#[tokio::test]
async fn ett_befintligt_nummer_nekas_och_ett_inaktivt_konto_erbjuds_aktivera() {
    let db = Databas::tillfällig().await;
    let (session, sida) = företag(&db).await;
    let bas = företagets_bas(&sida);

    let (finns, _) = posta(
        &db,
        &session,
        &format!("{bas}/kontoplan"),
        "nummer=1930&namn=X",
    )
    .await;
    let (inaktivt, _) = posta(
        &db,
        &session,
        &format!("{bas}/kontoplan"),
        "nummer=6212&namn=X",
    )
    .await;
    let (klass9, _) = posta(
        &db,
        &session,
        &format!("{bas}/kontoplan"),
        "nummer=9100&namn=X",
    )
    .await;
    let (_, finns) = hämta(&db, &session, &plats(&finns)).await;
    let (_, inaktivt) = hämta(&db, &session, &plats(&inaktivt)).await;
    let (_, klass9) = hämta(&db, &session, &plats(&klass9)).await;

    assert!(
        finns.contains("1930 finns redan: Företagskonto."),
        "{finns}"
    );
    assert!(
        inaktivt.contains("6212 finns redan: Mobiltelefon (inaktivt)."),
        "{inaktivt}"
    );
    assert!(inaktivt.contains("Aktivera 6212"), "{inaktivt}");
    assert!(klass9.contains("Klass 9 används inte i Maya."), "{klass9}");
    assert!(!rader(&klass9).contains(&9100));
}

#[tokio::test]
async fn ett_företag_utan_åtkomst_ger_403() {
    let db = Databas::tillfällig().await;
    let (session, _) = företag(&db).await;

    let (sida, _) = hämta(&db, &session, "/foretag/999/installningar/kontoplan").await;
    let (ändring, _) = posta(
        &db,
        &session,
        "/foretag/999/kontoplan/1930/aktiv",
        "aktiv=0",
    )
    .await;

    assert_eq!(sida.status(), StatusCode::FORBIDDEN);
    assert_eq!(ändring.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn länken_aktivera_från_verifikatet_söker_fram_kontot_bland_alla() {
    let db = Databas::tillfällig().await;
    let (session, sida) = företag(&db).await;

    let (response, body) = hämta(&db, &session, &format!("{sida}?sok=6212")).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(rader(&body), [6212]);
    assert!(body.contains("Aktivera 6212"), "{body}");
    assert!(body.contains(r#"value="6212""#), "sökfältet är förifyllt");
}
