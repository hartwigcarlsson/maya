//! Resultat- och balansräkningen under Rapporter, genom routern: urvalet i adressen,
//! banderollerna, panelen med verifikaten och exporten som PDF och CSV.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use http_body_util::BodyExt;
use maya_core::aktor::Aktör;
use maya_core::config::Config;
use maya_core::foretag::FöretagsId;
use maya_core::inloggning::{self, NyPasskey};
use maya_core::{Databas, Klocka, TillfälligDatabas, testhjalp};
use maya_pdf::testhjalp::är_pdfa_2b;
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

async fn hämta_bytes(
    db: &TillfälligDatabas,
    session: &str,
    path: &str,
) -> (Response<()>, Vec<u8>) {
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

async fn hämta(db: &TillfälligDatabas, session: &str, path: &str) -> (Response<()>, String) {
    let (response, body) = hämta_bytes(db, session, path).await;
    (response, String::from_utf8_lossy(&body).into_owned())
}

/// Ett inloggat företag med kalenderåret 2026: sessionen, sökvägen till företaget och
/// aktören.
async fn företag(db: &TillfälligDatabas) -> (String, String, Aktör, FöretagsId) {
    let klocka = Klocka::system();
    let länk = inloggning::skapa_första_användare(db, &klocka, &Aktör::cli(), "anna@example.se")
        .await
        .unwrap();
    let ny = NyPasskey {
        credential_id: b"ett".to_vec(),
        namn: "MacBook".into(),
        passkey: "{}".into(),
    };
    let session = inloggning::slutför_registrering(db, &klocka, länk.token(), ny, None)
        .await
        .unwrap()
        .token()
        .to_string();
    let aktör = inloggning::session(db, &klocka, &session)
        .await
        .unwrap()
        .unwrap()
        .aktör;
    let id = maya_core::foretag::lägg_upp(
        db,
        &klocka,
        &aktör,
        &maya_domain::foretag::NyttFöretag {
            orgnr: "556036-0793".into(),
            namn: "Exempel AB".into(),
            första_år_start: "2026-01-01".into(),
            första_år_slut: "2026-12-31".into(),
            bokföringsmetod: "faktura".into(),
            momsperiod: "kvartal".into(),
        },
    )
    .await
    .unwrap();
    (session, format!("/foretag/{id}"), aktör, id)
}

/// Försäljning i januari och mars 2026, hyra i mars och en försäljning i februari 2027.
async fn bokfört(db: &TillfälligDatabas) -> (String, String) {
    let (session, bas, aktör, id) = företag(db).await;
    for (datum, rader) in [
        ("2026-01-10", [(1930, 1_000_000, 0), (3001, 0, 1_000_000)]),
        ("2026-03-05", [(1510, 500_000, 0), (3001, 0, 500_000)]),
        ("2026-03-20", [(5010, 300_000, 0), (1930, 0, 300_000)]),
        ("2027-02-01", [(1930, 200_000, 0), (3001, 0, 200_000)]),
    ] {
        testhjalp::bokför(db, &aktör, id, datum, &rader).await;
    }
    (session, bas)
}

/// Texten i dokumentet, utan Leptos markörer.
fn dokument(body: &str) -> &str {
    let start = body.find(r#"class="rapportdokument""#).expect("dokumentet");
    let slut = start + body[start..].find("</table>").expect("tabellen");
    &body[start..slut]
}

#[tokio::test]
async fn rapporterna_nås_från_fliken() {
    let db = Databas::tillfällig().await;
    let (session, bas, _, _) = företag(&db).await;

    let (response, body) = hämta(&db, &session, &format!("{bas}/rapporter")).await;

    assert_eq!(response.status(), StatusCode::OK);
    for href in [
        format!("{bas}/rapporter/resultatrakning"),
        format!("{bas}/rapporter/balansrakning"),
        format!("{bas}/installningar/kontoplan"),
    ] {
        assert!(
            body.contains(&format!(r#"href="{href}""#)),
            "{href}: {body}"
        );
    }
}

#[tokio::test]
async fn resultaträkningen_visar_urvalet_i_adressen() {
    let db = Databas::tillfällig().await;
    let (session, bas) = bokfört(&db).await;
    let rr = format!("{bas}/rapporter/resultatrakning?ar=2026-01-01&tom=2026-03");

    let (response, body) = hämta(&db, &session, &format!("{rr}&niva=konton")).await;
    assert_eq!(response.status(), StatusCode::OK);
    let d = dokument(&body);
    for väntat in [
        "Mars 2026",
        "januari–mars 2026",
        "Nettoomsättning",
        "3001",
        "15 000,00",
        "Rörelseresultat",
        "12 000,00",
        "Årets resultat",
    ] {
        assert!(d.contains(väntat), "saknar {väntat:?} i {d}");
    }

    let (_, body) = hämta(&db, &session, &format!("{rr}&visa=manad&niva=rubriker")).await;
    let d = dokument(&body);
    assert!(d.contains("Mars 2026"), "{d}");
    assert!(!d.contains("ackumulerat"), "{d}");
    assert!(d.contains("Rörelsens intäkter"), "{d}");
    assert!(!d.contains("Nettoomsättning"), "{d}");

    // Första året: banderollen, och jämförelsen saknas.
    let (_, body) = hämta(&db, &session, &format!("{rr}&jmf=1")).await;
    assert!(body.contains(r#"data-banderoll="forsta-aret""#), "{body}");
    assert!(dokument(&body).contains("finns inte i Maya"), "{body}");
}

#[tokio::test]
async fn balansräkningen_har_kontrollrad_och_banderoll_för_preliminär_ingående_balans() {
    let db = Databas::tillfällig().await;
    let (session, bas) = bokfört(&db).await;

    let (_, body) = hämta(
        &db,
        &session,
        &format!("{bas}/rapporter/balansrakning?ar=2027-01-01&tom=2027-02"),
    )
    .await;

    let d = dokument(&body);
    assert!(d.contains("Ingående balans"), "{d}");
    assert!(d.contains("Årets resultat (ej bokfört på 2099)"), "{d}");
    assert!(d.contains(r#"class="kontroll""#), "{d}");
    assert!(!body.contains(r#"data-banderoll="forsta-aret""#), "{body}");
    let banderoll = &body[body
        .find(r#"data-banderoll="preliminar""#)
        .expect("banderollen för preliminär ingående balans")..];
    assert!(
        banderoll.contains(&format!(
            r#"href="{bas}/rapporter/balansrakning?ar=2026-01-01"#
        )),
        "{banderoll}"
    );
}

#[tokio::test]
async fn ett_konto_öppnar_panelen_med_verifikaten() {
    let db = Databas::tillfällig().await;
    let (session, bas) = bokfört(&db).await;
    let br = format!("{bas}/rapporter/balansrakning?ar=2026-01-01&tom=2026-03&niva=konton");

    let (_, body) = hämta(&db, &session, &br).await;
    // Kontot länkar till panelen med samma urval.
    assert!(body.contains("konto=1930"), "{body}");

    let (_, body) = hämta(&db, &session, &format!("{br}&konto=1930")).await;
    let panel = &body[body.find(r#"class="sidopanel""#).expect("panelen")..];
    assert!(panel.contains("1930 Företagskonto"), "{panel}");
    assert!(panel.contains("Ingående balans"), "{panel}");
    assert!(panel.contains("V1"), "{panel}");
    assert!(panel.contains("V3"), "{panel}");
    // Löpande saldo: 10 000 in, 3 000 ut.
    assert!(panel.contains("10 000,00"), "{panel}");
    assert!(panel.contains("7 000,00"), "{panel}");
    assert!(
        panel.contains(&format!(r#"href="{bas}/verifikat?v="#)),
        "{panel}"
    );
}

#[tokio::test]
async fn exporten_ger_pdf_och_csv_med_urvalets_filnamn() {
    let db = Databas::tillfällig().await;
    let (session, bas) = bokfört(&db).await;
    let urval = "ar=2026-01-01&tom=2026-03&visa=bada&niva=ink2";

    let (_, body) = hämta(
        &db,
        &session,
        &format!("{bas}/rapporter/resultatrakning?{urval}"),
    )
    .await;
    let pdf = format!("{bas}/rapporter/resultatrakning/pdf?{urval}");
    let csv = format!("{bas}/rapporter/resultatrakning/csv?{urval}");
    assert!(body.contains(&pdf.replace('&', "&amp;")), "{body}");
    assert!(body.contains(&csv.replace('&', "&amp;")), "{body}");

    let (response, innehåll) = hämta_bytes(&db, &session, &pdf).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/pdf");
    assert!(
        response.headers()[header::CONTENT_DISPOSITION]
            .to_str()
            .unwrap()
            .contains("resultatrakning_5560360793_2026_202603.pdf")
    );
    är_pdfa_2b(&innehåll);

    let (response, innehåll) = hämta_bytes(&db, &session, &csv).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "text/csv; charset=utf-8"
    );
    assert!(
        response.headers()[header::CONTENT_DISPOSITION]
            .to_str()
            .unwrap()
            .contains(r#"attachment; filename="resultatrakning_5560360793_2026_202603.csv""#)
    );
    let text = String::from_utf8(innehåll).unwrap();
    assert!(text.starts_with('\u{feff}'));
    assert!(text.contains("3001;Försäljning inom Sverige, 25 % moms;5000,00;15000,00"));
}

#[tokio::test]
async fn ett_annat_företag_visas_inte() {
    let db = Databas::tillfällig().await;
    let (session, _) = bokfört(&db).await;
    let annan = testhjalp::användare(&db, "bo@example.se").await;
    let annat = maya_core::foretag::lägg_upp(
        &db,
        &Klocka::system(),
        &annan,
        &maya_domain::foretag::NyttFöretag {
            orgnr: "559900-0006".into(),
            namn: "Annat AB".into(),
            första_år_start: "2026-01-01".into(),
            första_år_slut: "2026-12-31".into(),
            bokföringsmetod: "faktura".into(),
            momsperiod: "kvartal".into(),
        },
    )
    .await
    .unwrap();

    for sökväg in ["balansrakning", "balansrakning/pdf", "resultatrakning/csv"] {
        let (response, _) = hämta(
            &db,
            &session,
            &format!("/foretag/{annat}/rapporter/{sökväg}"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{sökväg}");
    }
}
