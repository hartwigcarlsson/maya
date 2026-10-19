//! Rätta ett bokfört verifikat i dokumentet (variant C), genom routern.

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

const FÖRSÄLJNING: &str = "datum=2026-03-14&beskrivning=F%C3%B6rs%C3%A4ljning+kontant\
    &konto_0=1930&debet_0=1+250&kredit_0=&text_0=Kassan\
    &konto_1=3001&debet_1=&kredit_1=1250&text_1=\
    &konto_2=&debet_2=&kredit_2=&text_2=";

/// Bokför försäljningen som V1; sökvägen till dokumentet och verifikatets id.
async fn bokfört(db: &TillfälligDatabas, session: &str, bas: &str) -> (String, String) {
    let response = posta(db, session, &format!("{bas}/verifikat"), FÖRSÄLJNING).await;
    let plats = plats(&response);
    let id = plats.rsplit_once("v=").unwrap().1.to_string();
    (plats, id)
}

fn dokumentet(body: &str) -> &str {
    &body[body.find(r#"class="dokument""#).expect(body)..]
}

#[tokio::test]
async fn rätta_visar_de_vända_raderna_datum_orsak_och_två_val() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;
    let (plats, id) = bokfört(&db, &session, &bas).await;

    let (_, body) = hämta(&db, &session, &plats).await;
    let rätta = format!("{bas}/verifikat?v={id}&amp;ratta=1");
    assert!(body.contains(&rätta), "{body}");
    assert!(body.contains("Rätta…"), "{body}");

    let (response, body) = hämta(&db, &session, &format!("{bas}/verifikat?v={id}&ratta=1")).await;
    assert_eq!(response.status(), StatusCode::OK);
    let panel = &body[body.find(r#"class="rattelse""#).expect(&body)..];
    assert!(
        panel.contains(&format!(r#"action="{bas}/verifikat/{id}/ratta""#)),
        "{panel}"
    );
    // 1930 i kredit och 3001 i debet, med radtexten.
    let rad_1930 = &panel[panel.find("<td>1930</td>").expect(panel)..];
    assert!(rad_1930.contains("Kassan"), "{rad_1930}");
    let rad_1930 = &rad_1930[..rad_1930.find("</tr>").unwrap()];
    assert!(
        rad_1930.ends_with(r#"<td class="belopp">1 250,00</td>"#),
        "{rad_1930}"
    );
    assert_eq!(rad_1930.matches("1 250,00").count(), 1, "{rad_1930}");
    for text in [
        r#"name="datum""#,
        r#"min="2026-03-14""#,
        r#"name="orsak""#,
        r#"value="registrera_om""#,
        "Bokför rättelse och registrera om",
        r#"value="bokfor""#,
        "Bara bokför rättelse",
    ] {
        assert!(panel.contains(text), "{text}: {panel}");
    }
}

#[tokio::test]
async fn bara_bokför_rättelse_bokför_den_och_länkarna_visar_vem_och_när() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;
    let (original, id) = bokfört(&db, &session, &bas).await;

    let response = posta(
        &db,
        &session,
        &format!("{bas}/verifikat/{id}/ratta"),
        "datum=2026-03-20&orsak=fel+konto&handling=bokfor",
    )
    .await;
    let plats = plats(&response);
    assert!(plats.ends_with("&rattad=V2"), "{plats}");

    let (_, body) = hämta(&db, &session, &plats).await;
    assert!(body.contains("Rättelsen bokfördes som V2."), "{body}");
    let rättelse = dokumentet(&body);
    for text in [
        "Rättelse av V1, fel konto",
        "Rättar",
        &format!(r#"href="{bas}/verifikat?v={id}""#),
        "Det här är en rättelse av V1 och kan inte rättas.",
    ] {
        assert!(rättelse.contains(text), "{text}: {rättelse}");
    }
    assert!(!rättelse.contains("Rätta…"), "{rättelse}");

    let (_, body) = hämta(&db, &session, &original).await;
    let original = dokumentet(&body);
    let badge = &original[original.find("badge-rattad").expect(original)..];
    assert!(
        badge[..badge.find("</span>").unwrap()].contains("V2"),
        "{badge}"
    );
    for text in [
        "<dt>Rättad</dt>",
        &format!(r#"href="{bas}/verifikat?v=2">V2</a>"#),
        "anna@example.se",
        "Redan rättat av V2. Ett verifikat kan bara rättas en gång.",
        "verifikat rättat",
    ] {
        assert!(original.contains(text), "{text}: {original}");
    }
    assert!(!original.contains("Rätta…"), "{original}");
}

#[tokio::test]
async fn bokför_rättelse_och_registrera_om_öppnar_ett_utkast_märkt_kopia_av() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;
    let (_, id) = bokfört(&db, &session, &bas).await;

    let response = posta(
        &db,
        &session,
        &format!("{bas}/verifikat/{id}/ratta"),
        "datum=2026-03-20&orsak=&handling=registrera_om",
    )
    .await;
    let plats = plats(&response);
    assert!(!plats.contains(&format!("v={id}&")), "{plats}");

    let (_, body) = hämta(&db, &session, &plats).await;
    assert!(
        body.contains(
            "Rättelsen bokfördes som V2. Här är ett utkast med V1s rader att rätta och bokföra."
        ),
        "{body}"
    );
    assert!(body.contains("kopia av"), "{body}");
    assert!(body.contains(r#"value="Kassan""#), "{body}");
    assert!(body.contains(r#"name="datum""#), "ett formulär: {body}");
}

#[tokio::test]
async fn ett_datum_före_originalets_skickas_tillbaka_och_visar_spärren() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;
    let (_, id) = bokfört(&db, &session, &bas).await;

    let response = posta(
        &db,
        &session,
        &format!("{bas}/verifikat/{id}/ratta"),
        "datum=2026-03-01&orsak=fel+konto&handling=bokfor",
    )
    .await;
    let plats = plats(&response);
    assert!(plats.contains("ratta=1"), "{plats}");

    let (_, body) = hämta(&db, &session, &plats).await;
    let panel = &body[body.find(r#"class="rattelse""#).expect(&body)..];
    assert!(
        panel.contains("Tidigast 2026-03-14 (originalets datum)."),
        "{panel}"
    );
    assert!(panel.contains(r#"value="2026-03-01""#), "{panel}");
    assert!(panel.contains(r#"value="fel konto""#), "{panel}");
}

#[tokio::test]
async fn ett_redan_rättat_verifikat_rättas_inte_igen() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;
    let (_, id) = bokfört(&db, &session, &bas).await;
    let rätta = format!("{bas}/verifikat/{id}/ratta");
    posta(&db, &session, &rätta, "datum=2026-03-20&handling=bokfor").await;

    let response = posta(&db, &session, &rätta, "datum=2026-03-20&handling=bokfor").await;

    assert_eq!(plats(&response), format!("{bas}/verifikat?v={id}"));
    let (_, body) = hämta(&db, &session, &format!("{bas}/verifikat?urval=bokforda")).await;
    // Med `>` framför: sidans nonce kan innehålla "V3".
    assert!(body.contains(">V2") && !body.contains(">V3"), "{body}");
}
