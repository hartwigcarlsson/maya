//! Kundfakturor i webben, genom routern: fliken, utkastet, spärrarna vid fälten, Granska och
//! utfärda, den utfärdade fakturan med sin arkiverade PDF och länken från verifikatet.

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

async fn hämta_svar(
    db: &TillfälligDatabas, session: &str, path: &str
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

async fn hämta(db: &TillfälligDatabas, session: &str, path: &str) -> String {
    let (response, body) = hämta_svar(db, session, path).await;
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

/// Ett företag med fakturauppgifterna ifyllda och kunden Kund AB: sessionen, företagets
/// adress och kundens id.
async fn fakturaföretag(db: &TillfälligDatabas) -> (String, String, String) {
    let session = inloggad(db).await;
    let bas = plats(
        &posta(
            db,
            &session,
            "/foretag",
            "orgnr=556036-0793&namn=Exempel+AB&start=2026-01-01&slut=2026-12-31\
             &bokforingsmetod=faktura&momsperiod=kvartal",
        )
        .await,
    );
    posta(
        db,
        &session,
        &format!("{bas}/installningar/uppgifter"),
        "namn=Exempel+AB&gatuadress=Exempelgatan+1&postnummer=12345&postort=Exempelstad\
         &epost=&telefon=&sate=Stockholm&momsregnr=SE556036079301&f_skatt=ja",
    )
    .await;
    posta(
        db,
        &session,
        &format!("{bas}/installningar/fakturauppgifter"),
        "bankgiro=50501055&plusgiro=&iban=&bic=&svarsadress=&standardmeddelande=",
    )
    .await;
    let kund = plats(
        &posta(
            db,
            &session,
            &format!("{bas}/kunder"),
            "namn=Kund+AB&identitet=&adress=Storgatan+1%0D%0A111+22+Stockholm&epost=\
             &momsregnr=&betalningsvillkor=30&omvand_skattskyldighet=ingen",
        )
        .await,
    );
    (session, bas, parameter(&kund, "k"))
}

fn utkast(kund: &str) -> String {
    format!(
        "kund={kund}&fakturadatum=2026-03-10&leveransdatum=&forfallodag=&er_referens=Bo+Ek\
         &startnummer=1007\
         &beskrivning_0=Konsulttimmar&antal_0=10&enhet_0=tim&a_pris_0=950&rabatt_0=\
         &momssats_0=25&konto_0=3001\
         &beskrivning_1=&antal_1=&enhet_1=&a_pris_1=&rabatt_1=&momssats_1=25&konto_1=3001"
    )
}

#[tokio::test]
async fn kundfakturor_är_en_flik_med_lista_och_länk_till_kundregistret() {
    let db = Databas::tillfällig().await;
    let (session, bas, _) = fakturaföretag(&db).await;

    let sida = hämta(&db, &session, &format!("{bas}/kundfakturor")).await;
    assert!(
        sida.contains(&format!(r#"href="{bas}/kundfakturor""#)),
        "fliken i toppmenyn"
    );
    assert!(sida.contains("Kundfakturor"));
    assert!(
        sida.contains(&format!(r#"href="{bas}/kunder""#)),
        "kundregistret"
    );
    assert!(sida.contains(&format!(r#"href="{bas}/kundfakturor/ny""#)));
    assert!(sida.contains("Inga kundfakturor"));

    let ny = hämta(&db, &session, &format!("{bas}/kundfakturor/ny")).await;
    assert!(ny.contains("Ny kundfaktura"));
    assert!(ny.contains("Kund AB"), "kunden i väljaren");
    assert!(ny.contains("Första fakturanummer"));
}

#[tokio::test]
async fn ett_utkast_sparas_och_visas_som_formulär_med_summan() {
    let db = Databas::tillfällig().await;
    let (session, bas, kund) = fakturaföretag(&db).await;

    let svar = posta(
        &db,
        &session,
        &format!("{bas}/kundfakturor"),
        &format!("{}&handling=spara", utkast(&kund)),
    )
    .await;
    let plats = plats(&svar);
    assert!(plats.contains("sparat=1"), "{plats}");
    let f = parameter(&plats, "f");

    let sida = hämta(&db, &session, &plats).await;
    assert!(sida.contains("Sparat."));
    assert!(sida.contains(r#"value="Konsulttimmar""#));
    assert!(sida.contains("11 875,00"), "totalen");
    assert!(sida.contains("Granska och utfärda"));
    // Listan har utkastet.
    let lista = &sida[sida.find(r#"class="verifikatlista""#).unwrap()..];
    assert!(lista.contains(&format!("?f={f}")));
    assert!(lista.contains("Kund AB"));
}

#[tokio::test]
async fn spärrarna_visas_vid_fälten_och_granska_visar_antalet_problem() {
    let db = Databas::tillfällig().await;
    let (session, bas, kund) = fakturaföretag(&db).await;
    let body = utkast(&kund).replace("antal_0=10", "antal_0=");
    let plats = plats(
        &posta(
            &db,
            &session,
            &format!("{bas}/kundfakturor"),
            &format!("{body}&handling=spara"),
        )
        .await,
    );

    let sida = hämta(&db, &session, &plats).await;
    assert!(sida.contains(r#"data-kod="rad_antal_saknas""#), "{sida}");
    assert!(sida.contains("Granska och utfärda · 1 problem kvar"));
}

#[tokio::test]
async fn granska_visar_numret_och_konteringen_och_utfärda_ger_den_utfärdade_fakturan() {
    let db = Databas::tillfällig().await;
    let (session, bas, kund) = fakturaföretag(&db).await;

    let granska = plats(
        &posta(
            &db,
            &session,
            &format!("{bas}/kundfakturor"),
            &format!("{}&handling=granska", utkast(&kund)),
        )
        .await,
    );
    assert!(granska.contains("granska=1"), "{granska}");
    let f = parameter(&granska, "f");
    let dialog = hämta(&db, &session, &granska).await;
    let dialog = &dialog[dialog.find("Granska och utfärda").unwrap()..];
    assert!(dialog.contains("nummer <strong>1007</strong>"), "{dialog}");
    assert!(dialog.contains(&format!("{bas}/kundfakturor/{f}/forhandsvisning")));
    for konto in ["1510", "3001", "2611"] {
        assert!(dialog.contains(konto), "{konto}");
    }
    assert!(dialog.contains("11 875,00"));

    // Förhandsvisningen är PDF:en som fakturan får.
    let (svar, pdf) = hämta_svar(
        &db,
        &session,
        &format!("{bas}/kundfakturor/{f}/forhandsvisning"),
    )
    .await;
    assert_eq!(svar.status(), StatusCode::OK);
    assert_eq!(svar.headers()[header::CONTENT_TYPE], "application/pdf");
    assert!(pdf.starts_with(b"%PDF"));

    let utfärdad = plats(
        &posta(
            &db,
            &session,
            &format!("{bas}/kundfakturor/{f}/utfarda"),
            "",
        )
        .await,
    );
    assert!(utfärdad.contains("utfardad=1"), "{utfärdad}");
    let sida = hämta(&db, &session, &utfärdad).await;
    let dokument = &sida[sida.find(r#"class="dokument"#).unwrap()..];
    assert!(dokument.contains("Kundfaktura 1007"), "{dokument}");
    assert!(dokument.contains("Kundfaktura_1007.pdf"), "filnamnet");
    assert!(dokument.contains("SHA-256"));
    assert!(dokument.contains("Arkiverad"));
    let underlag = dokument
        .split(&format!("{bas}/underlag/"))
        .nth(1)
        .unwrap()
        .split('/')
        .next()
        .unwrap()
        .to_string();
    assert!(dokument.contains(&format!("{bas}/underlag/{underlag}/fil")));
    // Verifikatet länkar till fakturan och fakturan till verifikatet.
    let v = dokument
        .split(&format!("{bas}/verifikat?v="))
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_string();
    let verifikat = hämta(&db, &session, &format!("{bas}/verifikat?v={v}")).await;
    assert!(verifikat.contains(&format!("{bas}/kundfakturor?f={f}")));
    assert!(verifikat.contains("kundfaktura 1007 till Kund AB"));
    assert!(verifikat.contains("Rättas i fakturan"));

    // Arkiverad PDF hämtas som den är.
    let (svar, arkiverad) =
        hämta_svar(&db, &session, &format!("{bas}/underlag/{underlag}/fil")).await;
    assert_eq!(svar.status(), StatusCode::OK);
    assert!(arkiverad.starts_with(b"%PDF"));
}

#[tokio::test]
async fn ett_utkast_raderas_efter_en_fråga() {
    let db = Databas::tillfällig().await;
    let (session, bas, kund) = fakturaföretag(&db).await;
    let plats_ = plats(
        &posta(
            &db,
            &session,
            &format!("{bas}/kundfakturor"),
            &format!("{}&handling=spara", utkast(&kund)),
        )
        .await,
    );
    let f = parameter(&plats_, "f");
    let fråga = hämta(&db, &session, &format!("{bas}/kundfakturor?f={f}&radera=1")).await;
    assert!(fråga.contains("Radera utkastet?"));

    let svar = posta(&db, &session, &format!("{bas}/kundfakturor/{f}/radera"), "").await;
    assert_eq!(plats(&svar), format!("{bas}/kundfakturor?raderad=1"));
    let lista = hämta(&db, &session, &format!("{bas}/kundfakturor?raderad=1")).await;
    assert!(lista.contains("Inga kundfakturor"));
}

#[tokio::test]
async fn den_som_saknar_åtkomst_får_inte_se_eller_utfärda_ett_annat_företags_fakturor() {
    let db = Databas::tillfällig().await;
    let (session, _, _) = fakturaföretag(&db).await;
    let svar = posta(&db, &session, "/foretag/4711/kundfakturor/1/utfarda", "").await;
    assert_eq!(svar.status(), StatusCode::FORBIDDEN);
    let (svar, _) = hämta_svar(
        &db,
        &session,
        "/foretag/4711/kundfakturor/1/forhandsvisning",
    )
    .await;
    assert_eq!(svar.status(), StatusCode::FORBIDDEN);
}

// #137 Ny kund från fakturautkastet

#[tokio::test]
async fn en_ny_kund_läggs_upp_på_utkastet_och_hamnar_i_kundregistret_när_fakturan_utfärdas() {
    let db = Databas::tillfällig().await;
    let (session, bas, _) = fakturaföretag(&db).await;

    let ny = hämta(&db, &session, &format!("{bas}/kundfakturor/ny")).await;
    assert!(ny.contains("+ Ny kund…"), "valet i kundväljaren");
    assert!(ny.contains("kort-streckat ny-kund"), "det streckade kortet");

    let body = format!(
        "{}&ny_namn=Nya+Kunden+AB&ny_identitet=&ny_adress=Lillgatan+2%0D%0A222+33+Malm%C3%B6\
         &ny_epost=&ny_momsregnr=&ny_betalningsvillkor=10&handling=spara",
        utkast("ny")
    );
    let sparat = plats(&posta(&db, &session, &format!("{bas}/kundfakturor"), &body).await);
    assert!(sparat.contains("sparat=1"), "{sparat}");
    let f = parameter(&sparat, "f");
    let sida = hämta(&db, &session, &sparat).await;
    assert!(
        sida.contains(r#"value="Nya Kunden AB""#),
        "kortet med kunden"
    );
    let lista = &sida[sida.find(r#"class="verifikatlista""#).unwrap()..];
    let lista = &lista[..lista.find("</ul>").unwrap()];
    assert!(
        lista.contains("Nya Kunden AB") && lista.contains("badge-ny"),
        "{lista}"
    );
    // Kunden finns inte i kundregistret än.
    let register = hämta(&db, &session, &format!("{bas}/kunder")).await;
    assert!(!register.contains("Nya Kunden AB"));

    let utfärdad = plats(
        &posta(
            &db,
            &session,
            &format!("{bas}/kundfakturor/{f}/utfarda"),
            "",
        )
        .await,
    );
    assert!(utfärdad.contains("utfardad=1"), "{utfärdad}");
    let register = hämta(&db, &session, &format!("{bas}/kunder")).await;
    assert!(register.contains("Nya Kunden AB"));
}
