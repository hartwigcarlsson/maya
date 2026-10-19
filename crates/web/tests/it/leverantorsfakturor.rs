//! Leverantörsfakturorna i webben: vägen in genom "Vad är det här?" och genom uppladdning i
//! fliken, utkastet med originalet bredvid, spärrarna och registreringen.

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

const GRÄNS: &str = "gransgransgrans";

fn parameter(plats: &str, namn: &str) -> String {
    let (_, fråga) = plats
        .split_once('?')
        .unwrap_or_else(|| panic!("ingen fråga i {plats}"));
    url::form_urlencoded::parse(fråga.as_bytes())
        .find(|(k, _)| k == namn)
        .map(|(_, v)| v.into_owned())
        .unwrap_or_else(|| panic!("{namn} saknas i {plats}"))
}

/// Laddar upp `filnamn` till `sökväg` med multipart och ger adressen som svaret ledde till.
async fn ladda_upp(
    db: &TillfälligDatabas, session: &str, sökväg: &str, filnamn: &str
) -> String {
    let body = format!(
        "--{GRÄNS}\r\nContent-Disposition: form-data; name=\"fil\"; filename=\"{filnamn}\"\r\n\
         Content-Type: application/pdf\r\n\r\n%PDF-1.7\n{filnamn}\n%%EOF\n\r\n--{GRÄNS}--\r\n"
    );
    let (response, _) = skicka(
        db,
        Request::post(sökväg)
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
    .await;
    plats(&response)
}

/// Företaget med leverantören Kontorsvaror AB; sessionen, basen och leverantörens id.
async fn med_leverantör(db: &TillfälligDatabas) -> (String, String, String) {
    let (session, bas) = företag(db).await;
    let r = posta(
        db,
        &session,
        &format!("{bas}/leverantorer"),
        "namn=Kontorsvaror+AB&land=SE&bankgiro=5050-1055&standardkonto=5410",
    )
    .await;
    let l = parameter(&plats(&r), "l");
    (session, bas, l)
}

fn faktura(l: &str, total: &str) -> String {
    format!(
        "leverantor={l}&fakturanummer=8812&fakturadatum=2026-03-14&forfallodag=2026-04-13\
         &referens=4455667788&totalbelopp={total}&originalvaluta=\
         &konto_0=&netto_0=1000&momssats_0=25&moms_0=\
         &konto_1=&netto_1=&momssats_1=25&moms_1="
    )
}

#[tokio::test]
async fn uppladdning_i_fliken_skapar_ett_utkast_med_originalet_bredvid_formuläret() {
    let db = Databas::tillfällig().await;
    let (session, bas, _) = med_leverantör(&db).await;

    let (response, body) = hämta(&db, &session, &format!("{bas}/leverantorsfakturor/ny")).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(body.contains("Ny leverantörsfaktura"), "{body}");
    assert!(
        body.contains(&format!("href=\"{bas}/leverantorer\"")),
        "länken till registret"
    );

    let adress = ladda_upp(
        &db,
        &session,
        &format!("{bas}/leverantorsfakturor"),
        "faktura.pdf",
    )
    .await;
    assert!(adress.contains("uppladdat=1"), "{adress}");
    let (response, body) = hämta(&db, &session, &adress).await;
    assert_eq!(response.status(), StatusCode::OK);
    let detalj = &body[body.find("med-underlag").expect(&body)..];
    assert!(detalj.contains("/underlag/"), "originalet visas: {detalj}");
    assert!(detalj.contains("Registrera och bokför"), "{detalj}");
    assert!(detalj.contains("Bokförs som"), "{detalj}");
    // Underlaget är inte okopplat längre.
    let (_, underlag) = hämta(&db, &session, &format!("{bas}/underlag")).await;
    assert!(!underlag.contains("faktura.pdf"), "{underlag}");
}

#[tokio::test]
async fn vad_är_det_här_ger_valet_leverantörsfaktura() {
    let db = Databas::tillfällig().await;
    let (session, bas, _) = med_leverantör(&db).await;
    let u = parameter(
        &ladda_upp(&db, &session, &format!("{bas}/underlag"), "faktura.pdf").await,
        "u",
    );
    let (_, body) = hämta(&db, &session, &format!("{bas}?oppna=underlag-{u}")).await;
    assert!(
        body.contains(&format!("{bas}/att-gora/underlag/{u}/leverantorsfaktura")),
        "{body}"
    );
    let r = posta(
        &db,
        &session,
        &format!("{bas}/att-gora/underlag/{u}/leverantorsfaktura"),
        "",
    )
    .await;
    let adress = plats(&r);
    assert!(
        adress.starts_with(&format!("{bas}/leverantorsfakturor?f=")),
        "{adress}"
    );
    let (_, body) = hämta(&db, &session, &adress).await;
    assert!(body.contains(&format!("{bas}/underlag/{u}/fil")), "{body}");
}

#[tokio::test]
async fn en_spärr_skickar_tillbaka_till_formuläret_och_registreringen_bokför() {
    let db = Databas::tillfällig().await;
    let (session, bas, l) = med_leverantör(&db).await;
    let adress = ladda_upp(
        &db,
        &session,
        &format!("{bas}/leverantorsfakturor"),
        "faktura.pdf",
    )
    .await;
    let f = parameter(&adress, "f");

    let r = posta(
        &db,
        &session,
        &format!("{bas}/leverantorsfakturor/{f}"),
        &format!("handling=registrera&{}", faktura(&l, "1200")),
    )
    .await;
    let tillbaka = plats(&r);
    assert!(tillbaka.contains("fel=sparrat"), "{tillbaka}");
    let (_, body) = hämta(&db, &session, &tillbaka).await;
    assert!(body.contains("data-kod=\"summerar_inte\""), "{body}");
    assert!(body.contains("problem kvar"), "{body}");

    let r = posta(
        &db,
        &session,
        &format!("{bas}/leverantorsfakturor/{f}"),
        &format!("handling=registrera&{}", faktura(&l, "1250")),
    )
    .await;
    let registrerad = plats(&r);
    assert!(registrerad.contains("registrerad=V1"), "{registrerad}");
    let (_, body) = hämta(&db, &session, &registrerad).await;
    assert!(body.contains("Registrerad och bokförd som V1."), "{body}");
    let dokument = &body[body.find("class=\"dokument\"").expect(&body)..];
    for väntat in [
        "Leverantörsfaktura 8812",
        "Kontorsvaror AB",
        "2440",
        "2641",
        "5410",
        "1 250,00",
    ] {
        assert!(dokument.contains(väntat), "{väntat}: {dokument}");
    }
    // Verifikatet länkar tillbaka till fakturan.
    let v = dokument
        .split("/verifikat?v=")
        .nth(1)
        .and_then(|r| r.split('"').next())
        .expect("länken till verifikatet");
    let (_, verifikat) = hämta(&db, &session, &format!("{bas}/verifikat?v={v}")).await;
    assert!(
        verifikat.contains(&format!("{bas}/leverantorsfakturor?f={f}")),
        "{verifikat}"
    );
    assert!(verifikat.contains("Rättas i fakturan."), "{verifikat}");
}

#[tokio::test]
async fn ett_utkast_sparas_och_raderas() {
    let db = Databas::tillfällig().await;
    let (session, bas, l) = med_leverantör(&db).await;
    let adress = ladda_upp(
        &db,
        &session,
        &format!("{bas}/leverantorsfakturor"),
        "faktura.pdf",
    )
    .await;
    let f = parameter(&adress, "f");
    let r = posta(
        &db,
        &session,
        &format!("{bas}/leverantorsfakturor/{f}"),
        &format!("handling=utkast&{}", faktura(&l, "1200")),
    )
    .await;
    assert!(plats(&r).contains("sparat=1"));
    let (_, body) = hämta(&db, &session, &format!("{bas}/leverantorsfakturor?f={f}")).await;
    assert!(body.contains("value=\"8812\""), "{body}");
    let r = posta(
        &db,
        &session,
        &format!("{bas}/leverantorsfakturor/{f}/radera"),
        "",
    )
    .await;
    assert!(plats(&r).contains("raderat=1"));
    let (_, underlag) = hämta(&db, &session, &format!("{bas}/underlag")).await;
    assert!(
        underlag.contains("faktura.pdf"),
        "originalet är okopplat igen"
    );
}

// #146 omvänd skattskyldighet

#[tokio::test]
async fn leverantörens_förval_för_omvänd_skattskyldighet_föreslås_och_bokförs_med_fiktiv_moms() {
    let db = Databas::tillfällig().await;
    let (session, bas) = företag(&db).await;
    let r = posta(
        &db,
        &session,
        &format!("{bas}/leverantorer"),
        "namn=Byggfirman+AB&land=SE&bankgiro=5050-1055&standardkonto=5410\
         &omvand_skattskyldighet=bygg",
    )
    .await;
    let l = parameter(&plats(&r), "l");
    let adress = ladda_upp(
        &db,
        &session,
        &format!("{bas}/leverantorsfakturor"),
        "faktura.pdf",
    )
    .await;
    let f = parameter(&adress, "f");
    let r = posta(
        &db,
        &session,
        &format!("{bas}/leverantorsfakturor/{f}"),
        &format!("handling=utkast&{}", faktura(&l, "1000")),
    )
    .await;
    assert!(plats(&r).contains("sparat=1"));

    let (_, body) = hämta(&db, &session, &format!("{bas}/leverantorsfakturor?f={f}")).await;
    let val = &body[body.find("name=\"omvand_skattskyldighet\"").expect(&body)..];
    let val = &val[..val.find("</select>").unwrap()];
    assert!(
        val.contains("<option value=\"bygg\" selected"),
        "förvalet föreslås: {val}"
    );
    assert!(body.contains("Fiktiv moms"), "{body}");

    let r = posta(
        &db,
        &session,
        &format!("{bas}/leverantorsfakturor/{f}"),
        &format!(
            "handling=registrera&omvand_skattskyldighet=bygg&{}",
            faktura(&l, "1000")
        ),
    )
    .await;
    let registrerad = plats(&r);
    assert!(registrerad.contains("registrerad=V1"), "{registrerad}");
    let (_, body) = hämta(&db, &session, &registrerad).await;
    let dokument = &body[body.find("class=\"dokument\"").expect(&body)..];
    let dokument = &dokument[..dokument.find("Historik").expect(dokument)];
    for väntat in ["Omvänd skattskyldighet", "Bygg", "2647", "2614", "1 000,00"] {
        assert!(dokument.contains(väntat), "{väntat}: {dokument}");
    }
    assert!(!dokument.contains("2641"), "{dokument}");
}

#[tokio::test]
async fn ett_företag_utan_momsregistrering_har_inget_val_för_omvänd_skattskyldighet() {
    let db = Databas::tillfällig().await;
    let session = inloggad(&db).await;
    let r = posta(
        &db,
        &session,
        "/foretag",
        "orgnr=556036-0793&namn=Exempel+AB&start=2026-01-01&slut=2026-12-31\
         &bokforingsmetod=faktura&momsperiod=ej_momsregistrerad",
    )
    .await;
    let bas = plats(&r);
    let adress = ladda_upp(
        &db,
        &session,
        &format!("{bas}/leverantorsfakturor"),
        "faktura.pdf",
    )
    .await;
    let (_, body) = hämta(&db, &session, &adress).await;
    assert!(body.contains("Registrera och bokför"), "{body}");
    assert!(!body.contains("name=\"omvand_skattskyldighet\""), "{body}");
}

// #149 Kontantmetoden
#[tokio::test]
async fn under_kontantmetoden_heter_knappen_registrera_och_inget_bokförs() {
    let db = Databas::tillfällig().await;
    let session = inloggad(&db).await;
    let r = posta(
        &db,
        &session,
        "/foretag",
        "orgnr=556036-0793&namn=Exempel+AB&start=2026-01-01&slut=2026-12-31\
         &bokforingsmetod=kontant&momsperiod=kvartal",
    )
    .await;
    let bas = plats(&r);
    let r = posta(
        &db,
        &session,
        &format!("{bas}/leverantorer"),
        "namn=Kontorsvaror+AB&land=SE&bankgiro=5050-1055&standardkonto=5410",
    )
    .await;
    let l = parameter(&plats(&r), "l");
    let adress = ladda_upp(
        &db,
        &session,
        &format!("{bas}/leverantorsfakturor"),
        "faktura.pdf",
    )
    .await;
    let f = parameter(&adress, "f");
    let (_, body) = hämta(&db, &session, &adress).await;
    let detalj = &body[body.find("med-underlag").expect(&body)..];
    assert!(!detalj.contains("Registrera och bokför"), "{detalj}");
    assert!(detalj.contains("Registrera"), "{detalj}");
    assert!(detalj.contains("Bokförs vid betalningen"), "{detalj}");

    let r = posta(
        &db,
        &session,
        &format!("{bas}/leverantorsfakturor/{f}"),
        &format!("handling=registrera&{}", faktura(&l, "1250")),
    )
    .await;
    let registrerad = plats(&r);
    assert!(registrerad.contains("registrerad=0"), "{registrerad}");
    let (_, body) = hämta(&db, &session, &registrerad).await;
    assert!(
        body.contains("Under kontantmetoden bokförs fakturan när den betalas."),
        "{body}"
    );
    let (_, verifikat) = hämta(&db, &session, &format!("{bas}/verifikat")).await;
    assert!(
        !verifikat.contains("Leverantörsfaktura 8812"),
        "{verifikat}"
    );
}
