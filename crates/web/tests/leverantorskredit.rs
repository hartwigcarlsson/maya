//! Kreditfaktura från leverantör och Rätta avskrift i webben, genom routern (#148): hänvisningen
//! i formuläret och åt båda hållen i dokumenten, tillgodohavandet med Återbetala…, och panelen
//! Rätta avskrift… som låser upp fakturan för att registreras om.

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
const GRÄNS: &str = "gransgransgrans";

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

fn parameter(plats: &str, namn: &str) -> String {
    let (_, fråga) = plats
        .split_once('?')
        .unwrap_or_else(|| panic!("ingen fråga i {plats}"));
    url::form_urlencoded::parse(fråga.as_bytes())
        .find(|(k, _)| k == namn)
        .map(|(_, v)| v.into_owned())
        .unwrap_or_else(|| panic!("{namn} saknas i {plats}"))
}

fn efter(sida: &str, text: &str) -> String {
    sida[sida
        .find(text)
        .unwrap_or_else(|| panic!("{text} saknas i {sida}"))..]
        .to_string()
}

/// Laddar upp ett original i fliken och ger utkastets id.
async fn ladda_upp(db: &TillfälligDatabas, session: &str, bas: &str, namn: &str) -> String {
    let body = format!(
        "--{GRÄNS}\r\nContent-Disposition: form-data; name=\"fil\"; filename=\"{namn}.pdf\"\r\n\
         Content-Type: application/pdf\r\n\r\n%PDF-1.7\n{namn}\n%%EOF\n\r\n--{GRÄNS}--\r\n"
    );
    let (r, _) = skicka(
        db,
        Request::post(format!("{bas}/leverantorsfakturor"))
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
    parameter(&plats(&r), "f")
}

/// Ett företag med leverantören Kontorsvaror AB och dess registrerade faktura 8812 på
/// 1 250,00 kr: sessionen, basen, leverantören och fakturan.
async fn original(db: &TillfälligDatabas) -> (String, String, String, String) {
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
    let l = parameter(
        &plats(
            &posta(
                db,
                &session,
                &format!("{bas}/leverantorer"),
                "namn=Kontorsvaror+AB&land=SE&bankgiro=5050-1055&standardkonto=5410",
            )
            .await,
        ),
        "l",
    );
    let f = ladda_upp(db, &session, &bas, "f").await;
    registrera(db, &session, &bas, &f, &l, "8812", "1250", "1000", "").await;
    (session, bas, l, f)
}

/// Registrerar fakturan `f` med en rad på 5410 och 25 % moms; `krediterar` kan vara tom.
#[allow(clippy::too_many_arguments)]
async fn registrera(
    db: &TillfälligDatabas,
    session: &str,
    bas: &str,
    f: &str,
    l: &str,
    nummer: &str,
    total: &str,
    netto: &str,
    krediterar: &str,
) -> String {
    plats(
        &posta(
            db,
            session,
            &format!("{bas}/leverantorsfakturor/{f}"),
            &format!(
                "handling=registrera&leverantor={l}&fakturanummer={nummer}\
                 &fakturadatum=2026-03-14&forfallodag=2026-04-13&referens=4455667788\
                 &totalbelopp={total}&originalvaluta=&krediterar={krediterar}\
                 &konto_0=&netto_0={netto}&momssats_0=25&moms_0="
            ),
        )
        .await,
    )
}

#[tokio::test]
async fn en_kreditfaktura_registreras_med_negativ_total_och_hänvisning_till_originalet() {
    let db = Databas::tillfällig().await;
    let (session, bas, l, f) = original(&db).await;
    let k = ladda_upp(&db, &session, &bas, "kredit").await;

    // Med leverantören vald kan utkastet hänvisa till leverantörens registrerade fakturor.
    posta(
        &db,
        &session,
        &format!("{bas}/leverantorsfakturor/{k}"),
        &format!("handling=utkast&leverantor={l}&konto_0=&netto_0=&momssats_0=25&moms_0="),
    )
    .await;
    let utkast = hämta(&db, &session, &format!("{bas}/leverantorsfakturor?f={k}")).await;
    let val = efter(&utkast, r#"name="krediterar""#);
    assert!(val.contains(&format!(r#"value="{f}""#)), "{val}");
    assert!(val.contains("8812"));
    assert!(utkast.contains("Kontera som originalet"));

    let registrerad = registrera(&db, &session, &bas, &k, &l, "K-17", "-250", "-200", &f).await;
    assert!(registrerad.contains("registrerad="), "{registrerad}");
    let kredit = hämta(&db, &session, &registrerad).await;
    let dokument = efter(&kredit, "class=\"dokument\"");
    assert!(dokument.contains("Kreditfaktura K-17"), "{dokument}");
    assert!(dokument.contains("badge-kreditfaktura"));
    assert!(dokument.contains("-250,00"));
    let krediterar = efter(&dokument, "<dt>Krediterar");
    assert!(
        krediterar.contains(&format!("{bas}/leverantorsfakturor?f={f}")),
        "{krediterar}"
    );
    assert!(!dokument.contains("Registrera betalning"));

    let originalet = hämta(&db, &session, &format!("{bas}/leverantorsfakturor?f={f}")).await;
    let dokument = efter(&originalet, "class=\"dokument\"");
    assert!(dokument.contains("1 000,00 kvar"), "{dokument}");
    let av = efter(&dokument, "<dt>Krediteras av");
    assert!(av.contains("K-17"), "{av}");
    assert!(av.contains(&format!("{bas}/leverantorsfakturor?f={k}")));
}

#[tokio::test]
async fn kredit_av_en_betald_faktura_ger_tillgodo_som_återbetalas_i_dokumentet() {
    let db = Databas::tillfällig().await;
    let (session, bas, l, f) = original(&db).await;
    let betald = plats(
        &posta(
            &db,
            &session,
            &format!("{bas}/leverantorsfakturor/{f}/betalning"),
            "betaldag=2026-04-01&belopp=1250&konto=1930",
        )
        .await,
    );
    assert!(betald.contains("betald=1"), "{betald}");
    let k = ladda_upp(&db, &session, &bas, "kredit").await;
    registrera(&db, &session, &bas, &k, &l, "K-17", "-250", "-200", &f).await;

    let här = format!("{bas}/leverantorsfakturor?f={f}");
    let dokument = hämta(&db, &session, &här).await;
    let dokument = efter(&dokument, "class=\"dokument\"");
    assert!(dokument.contains("badge-tillgodo"), "{dokument}");
    assert!(
        dokument.contains(r#"badge-tillgodo">Tillgodo<"#),
        "{dokument}"
    );
    assert!(dokument.contains("250,00 tillgodo"), "{dokument}");
    assert!(
        dokument.contains(&format!("{här}&amp;panel=aterbetala")),
        "{dokument}"
    );

    let panel = hämta(&db, &session, &format!("{här}&panel=aterbetala")).await;
    let panel = efter(&panel, "betalning-rubrik");
    assert!(panel.contains("Återbetala tillgodohavande"), "{panel}");
    assert!(panel.contains("Till konto"));
    let åter = plats(
        &posta(
            &db,
            &session,
            &format!("{bas}/leverantorsfakturor/{f}/betalning"),
            "aterbetalning=1&betaldag=2026-04-20&belopp=250&konto=1930",
        )
        .await,
    );
    assert!(åter.contains("aterbetald=1"), "{åter}");
    let dokument = hämta(&db, &session, &åter).await;
    assert!(
        dokument.contains("Återbetalningen registrerades"),
        "{dokument}"
    );
    assert!(efter(&dokument, "class=\"dokument\"").contains("badge-betald"));
}

#[tokio::test]
async fn rätta_avskrift_låser_upp_fakturan_som_registreras_om_utan_total_under_det_betalda() {
    let db = Databas::tillfällig().await;
    let (session, bas, l, f) = original(&db).await;
    posta(
        &db,
        &session,
        &format!("{bas}/leverantorsfakturor/{f}/betalning"),
        "betaldag=2026-04-01&belopp=500&konto=1930",
    )
    .await;
    let här = format!("{bas}/leverantorsfakturor?f={f}");
    let dokument = hämta(&db, &session, &här).await;
    assert!(
        efter(&dokument, "class=\"dokument\"").contains(&format!("{här}&amp;panel=ratta-avskrift")),
        "{dokument}"
    );
    let panel = hämta(&db, &session, &format!("{här}&panel=ratta-avskrift")).await;
    let panel = efter(&panel, "avskrift-rubrik");
    assert!(panel.contains(r#"name="orsak""#), "{panel}");
    assert!(panel.contains(&format!("{bas}/leverantorsfakturor/{f}/ratta-avskrift")));
    assert!(panel.contains("2440"), "rättelsen visas: {panel}");

    let rättad = plats(
        &posta(
            &db,
            &session,
            &format!("{bas}/leverantorsfakturor/{f}/ratta-avskrift"),
            "orsak=",
        )
        .await,
    );
    assert!(rättad.contains("avskrift_rattad="), "{rättad}");
    let upplåst = hämta(&db, &session, &rättad).await;
    assert!(upplåst.contains("Avskriften rättades"), "{upplåst}");
    let formulär = efter(&upplåst, "lf-formular");
    assert!(formulär.contains("badge-upplast"), "{formulär}");
    assert!(!formulär.contains("Radera utkast"));
    assert!(formulär.contains("500,00"), "det betalda syns: {formulär}");

    // En total under det som redan har betalats spärras vid totalen.
    let tillbaka = registrera(&db, &session, &bas, &f, &l, "8812", "400", "320", "").await;
    assert!(tillbaka.contains("fel=sparrat"), "{tillbaka}");
    let sida = hämta(&db, &session, &tillbaka).await;
    assert!(sida.contains(r#"data-kod="under_betalt""#), "{sida}");

    let omregistrerad = registrera(&db, &session, &bas, &f, &l, "8812", "1500", "1200", "").await;
    assert!(omregistrerad.contains("registrerad="), "{omregistrerad}");
    let dokument = hämta(&db, &session, &omregistrerad).await;
    let dokument = efter(&dokument, "class=\"dokument\"");
    assert!(dokument.contains("1 000,00 kvar"), "{dokument}");
    assert!(dokument.contains("Rätta avskrift"), "{dokument}");
}
