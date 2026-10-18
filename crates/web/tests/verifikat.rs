//! Att registrera och bokföra ett verifikat i fliken Verifikat, genom routern.

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

#[tokio::test]
async fn formuläret_har_datum_beskrivning_och_rader() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;

    let (response, body) = hämta(&db, &session, &format!("{bas}/verifikat/nytt")).await;

    assert_eq!(response.status(), StatusCode::OK);
    for fält in [
        r#"name="datum""#,
        r#"name="beskrivning""#,
        r#"name="konto_0""#,
        r#"name="debet_0""#,
        r#"name="kredit_0""#,
        r#"name="text_0""#,
        r#"name="konto_1""#,
        "Bokför",
    ] {
        assert!(body.contains(fält), "{fält}: {body}");
    }
    assert!(
        body.contains(&format!(r#"action="{bas}/verifikat""#)),
        "{body}"
    );
}

#[tokio::test]
async fn ett_bokfört_verifikat_visas_som_dokument_med_nummer_rader_och_historik() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;

    let response = posta(&db, &session, &format!("{bas}/verifikat"), FÖRSÄLJNING).await;

    let plats = plats(&response);
    assert!(plats.starts_with(&format!("{bas}/verifikat?v=")), "{plats}");
    let (response, body) = hämta(&db, &session, &plats).await;
    assert_eq!(response.status(), StatusCode::OK);
    let dokument = &body[body.find(r#"class="dokument""#).expect(&body)..];
    for text in [
        "V1",
        "Försäljning kontant",
        "1 250,00",
        "2026-03-14",
        "mars 2026",
        "anna@example.se",
        "webb",
        "1930",
        "Företagskonto",
        "Kassan",
        "3001",
        "verifikat bokfört",
    ] {
        assert!(dokument.contains(text), "{text}: {dokument}");
    }
    assert!(
        !dokument.contains("<input"),
        "ett dokument, inte ett låst formulär"
    );
    let lista = &body[body.find(r#"class="verifikatlista""#).unwrap()..];
    assert!(lista.contains("V1"), "{lista}");
}

#[tokio::test]
async fn servern_nekar_med_samma_koder_som_formuläret_visar_vid_fälten() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;
    let body = "datum=2025-12-31&beskrivning=\
        &konto_0=1950&debet_0=100&kredit_0=&text_0=\
        &konto_1=3001&debet_1=&kredit_1=90&text_1=";

    let response = posta(&db, &session, &format!("{bas}/verifikat"), body).await;

    let plats = plats(&response);
    assert!(
        plats.starts_with(&format!("{bas}/verifikat/nytt?")),
        "{plats}"
    );
    let (_, sida) = hämta(&db, &session, &plats).await;
    for kod in [
        "fore_forsta_aret",
        "beskrivning_saknas",
        "konto_inaktivt",
        "differens",
    ] {
        assert!(
            sida.contains(&format!(r#"data-kod="{kod}""#)),
            "{kod}: {sida}"
        );
    }
    assert!(
        sida.contains("Aktivera"),
        "genvägen vid inaktivt konto: {sida}"
    );
    assert!(sida.contains(r#"value="1950""#), "{sida}");
    assert!(sida.contains("4 problem kvar"), "{sida}");
    let (_, lista) = hämta(&db, &session, &format!("{bas}/verifikat")).await;
    assert!(!lista.contains("V1"), "{lista}");
}

#[tokio::test]
async fn filtren_visar_alla_utkast_och_bokförda() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;
    posta(&db, &session, &format!("{bas}/verifikat"), FÖRSÄLJNING).await;

    let (_, alla) = hämta(&db, &session, &format!("{bas}/verifikat")).await;
    let (_, bokförda) = hämta(&db, &session, &format!("{bas}/verifikat?urval=bokforda")).await;
    let (_, utkast) = hämta(&db, &session, &format!("{bas}/verifikat?urval=utkast")).await;

    let lista = |body: &str| {
        let start = body.find(r#"class="verifikatlista""#).expect(body);
        body[start..start + body[start..].find("</ul>").unwrap()].to_string()
    };
    assert!(lista(&alla).contains("V1"));
    assert!(lista(&bokförda).contains("V1"));
    assert!(!lista(&utkast).contains("V1"));
    for filter in ["Alla", "Utkast", "Bokförda"] {
        assert!(alla.contains(filter), "{filter}");
    }
}

#[tokio::test]
async fn ett_verifikat_i_ett_annat_företag_nekas() {
    let db = Databas::tillfällig().await;
    let (session, _) = företag(&db).await;

    let (response, _) = hämta(&db, &session, "/foretag/4711/verifikat").await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let response = posta(&db, &session, "/foretag/4711/verifikat", FÖRSÄLJNING).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

/// Ett obalanserat utkast utan beskrivning, med en rad utan konto.
const HALVFÄRDIGT: &str = "handling=utkast&datum=2026-03-14&beskrivning=\
    &konto_0=6540&debet_0=599&kredit_0=&text_0=\
    &konto_1=&debet_1=&kredit_1=748,75&text_1=Kortet";

/// Sparar `HALVFÄRDIGT` som utkast; sökvägen till utkastet.
async fn spara_utkast(db: &TillfälligDatabas, session: &str, bas: &str) -> String {
    let response = posta(db, session, &format!("{bas}/verifikat"), HALVFÄRDIGT).await;
    plats(&response)
}

fn utkastets_id(plats: &str) -> String {
    let v = &plats[plats.find("v=").expect(plats) + 2..];
    v.chars().take_while(char::is_ascii_digit).collect()
}

#[tokio::test]
async fn ett_obalanserat_utkast_sparas_och_öppnas_igen_i_formuläret() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;

    let plats = spara_utkast(&db, &session, &bas).await;

    assert!(plats.starts_with(&format!("{bas}/verifikat?v=")), "{plats}");
    assert!(plats.contains("sparat=1"), "{plats}");
    let id = utkastets_id(&plats);
    let (response, body) = hämta(&db, &session, &plats).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(body.contains("Utkastet sparades."), "{body}");
    let formulär = &body[body.find(r#"class="verifikatformular""#).expect(&body)..];
    for text in [
        r#"value="2026-03-14""#,
        r#"value="6540""#,
        r#"value="599,00""#,
        r#"value="748,75""#,
        r#"value="Kortet""#,
        &format!(r#"name="utkast" value="{id}""#),
        "badge-utkast",
    ] {
        assert!(formulär.contains(text), "{text}: {formulär}");
    }
    let bokför = formulär.find(r#"value="bokfor""#).expect("Bokför");
    let spara = formulär.find(r#"value="utkast""#).expect("Spara utkast");
    let radera = formulär.find("Radera utkast").expect("Radera utkast");
    assert!(bokför < spara && spara < radera, "{formulär}");
    assert!(!formulär.contains("V1"));

    let (_, utkast) = hämta(&db, &session, &format!("{bas}/verifikat?urval=utkast")).await;
    let lista = &utkast[utkast.find(r#"class="verifikatlista""#).unwrap()..];
    assert!(lista.contains("badge-utkast"), "{lista}");
    assert!(lista.contains(&format!("?v={id}")), "{lista}");
    let (_, bokförda) = hämta(&db, &session, &format!("{bas}/verifikat?urval=bokforda")).await;
    let lista = &bokförda[bokförda.find(r#"class="verifikatlista""#).unwrap()..];
    assert!(!lista.contains(&format!("?v={id}")), "{lista}");
}

#[tokio::test]
async fn ett_utkast_som_inte_kan_sparas_visar_varför_vid_fälten() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;
    let body = "handling=utkast&datum=2029-01-01&beskrivning=\
        &konto_0=6540&debet_0=5x&kredit_0=&text_0=";

    let response = posta(&db, &session, &format!("{bas}/verifikat"), body).await;

    let plats = plats(&response);
    assert!(
        plats.starts_with(&format!("{bas}/verifikat/nytt?")),
        "{plats}"
    );
    let (_, sida) = hämta(&db, &session, &plats).await;
    assert!(sida.contains("Utkastet kunde inte sparas"), "{sida}");
    for kod in ["saknat_rakenskapsar", "belopp_ogiltigt"] {
        assert!(
            sida.contains(&format!(r#"data-kod="{kod}""#)),
            "{kod}: {sida}"
        );
    }
    let (_, utkast) = hämta(&db, &session, &format!("{bas}/verifikat?urval=utkast")).await;
    let lista = &utkast[utkast.find(r#"class="verifikatlista""#).unwrap()..];
    assert!(!lista.contains("?v="), "{lista}");
}

#[tokio::test]
async fn ett_utkast_bokförs_och_får_sitt_nummer_först_då() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;
    let id = utkastets_id(&spara_utkast(&db, &session, &bas).await);

    // Obalanserat: tillbaka till utkastet med spärrarna, utan nummer.
    let obalanserat =
        HALVFÄRDIGT.replace("handling=utkast", &format!("handling=bokfor&utkast={id}"));
    let response = posta(&db, &session, &format!("{bas}/verifikat"), &obalanserat).await;
    let tillbaka = plats(&response);
    assert!(
        tillbaka.starts_with(&format!("{bas}/verifikat?v={id}&")),
        "{tillbaka}"
    );
    let (_, sida) = hämta(&db, &session, &tillbaka).await;
    for kod in ["beskrivning_saknas", "konto_saknas", "differens"] {
        assert!(
            sida.contains(&format!(r#"data-kod="{kod}""#)),
            "{kod}: {sida}"
        );
    }

    let rätt = format!("handling=bokfor&utkast={id}&{}", FÖRSÄLJNING);
    let response = posta(&db, &session, &format!("{bas}/verifikat"), &rätt).await;

    assert_eq!(plats(&response), format!("{bas}/verifikat?v={id}"));
    let (_, body) = hämta(&db, &session, &format!("{bas}/verifikat?v={id}")).await;
    let dokument = &body[body.find(r#"class="dokument""#).expect(&body)..];
    assert!(dokument.contains("V1"), "{dokument}");
    assert!(dokument.contains("Försäljning kontant"), "{dokument}");
}

#[tokio::test]
async fn radera_utkast_frågar_först_med_en_sammanfattning_och_raderingen_loggas() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;
    let id = utkastets_id(&spara_utkast(&db, &session, &bas).await);

    let (_, fråga) = hämta(&db, &session, &format!("{bas}/verifikat?v={id}&radera=1")).await;

    let dialog = &fråga[fråga.find("Radera utkastet?").expect(&fråga)..];
    for text in ["2026-03-14", "599,00 kr", "2 rader", "loggas"] {
        assert!(dialog.contains(text), "{text}: {dialog}");
    }
    let action = format!(r#"action="{bas}/verifikat/{id}/radera""#);
    assert!(dialog.contains(&action), "{dialog}");
    assert!(
        dialog.contains(&format!(r#"href="{bas}/verifikat?v={id}""#)),
        "Avbryt: {dialog}"
    );

    let response = posta(&db, &session, &format!("{bas}/verifikat/{id}/radera"), "").await;

    let plats = plats(&response);
    assert_eq!(plats, format!("{bas}/verifikat?urval=utkast&raderat=1"));
    let (_, body) = hämta(&db, &session, &plats).await;
    assert!(body.contains("Utkastet raderades"), "{body}");
    let lista = &body[body.find(r#"class="verifikatlista""#).unwrap()..];
    assert!(!lista.contains(&format!("?v={id}")), "{lista}");

    // Ett bokfört verifikat raderas inte.
    posta(&db, &session, &format!("{bas}/verifikat"), FÖRSÄLJNING).await;
    let (_, body) = hämta(&db, &session, &format!("{bas}/verifikat?urval=bokforda")).await;
    let lista = &body[body.find(r#"class="verifikatlista""#).unwrap()..];
    let v1 = utkastets_id(lista);
    let response = posta(&db, &session, &format!("{bas}/verifikat/{v1}/radera"), "").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let (_, body) = hämta(&db, &session, &format!("{bas}/verifikat?v={v1}")).await;
    assert!(body.contains(r#"class="dokument""#), "{body}");
}
