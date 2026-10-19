//! Betalning av kundfaktura i webben, genom routern (#139): panelen Registrera betalning med
//! "Bokförs som", spärrarna vid fälten, delbetalningen, Rätta betalning, märkena och listans
//! grupper med kundfordringarna, och kundens fakturor i kundregistret.

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

/// Utfärdar faktura 1007 på 11 875,00 kr daterad 2026-03-10, som förföll 2026-04-09, och ger
/// dess adress i listan.
async fn utfärdad(db: &TillfälligDatabas, session: &str, bas: &str, kund: &str) -> String {
    let granska = plats(
        &posta(
            db,
            session,
            &format!("{bas}/kundfakturor"),
            &format!("{}&handling=granska", utkast(kund)),
        )
        .await,
    );
    let f = parameter(&granska, "f");
    let utfärdad = plats(&posta(db, session, &format!("{bas}/kundfakturor/{f}/utfarda"), "").await);
    assert!(utfärdad.contains("utfardad=1"), "{utfärdad}");
    format!("{bas}/kundfakturor?f={f}")
}

fn efter(sida: &str, text: &str) -> String {
    sida[sida
        .find(text)
        .unwrap_or_else(|| panic!("{text} saknas i {sida}"))..]
        .to_string()
}

/// Adressen `adress` som den står i ett `href`, med `&` som `&amp;`.
fn i_html(adress: &str) -> String {
    adress.replace('&', "&amp;")
}

#[tokio::test]
async fn en_betalning_registreras_i_panelen_och_visas_i_dokumentet() {
    let db = Databas::tillfällig().await;
    let (session, bas, kund) = fakturaföretag(&db).await;
    let här = utfärdad(&db, &session, &bas, &kund).await;
    let f = parameter(&här, "f");

    // Förföll 2026-04-09 och är obetald.
    let dokument = hämta(&db, &session, &här).await;
    assert!(dokument.contains("badge-forfallen"), "{dokument}");
    assert!(dokument.contains("11 875,00 kvar"));
    assert!(dokument.contains("Inga betalningar än."));
    assert!(dokument.contains(&i_html(&format!("{här}&panel=betalning"))));

    let panel = hämta(&db, &session, &format!("{här}&panel=betalning")).await;
    let panel = efter(&panel, "betalning-rubrik");
    assert!(
        panel.contains(r#"value="11 875,00""#),
        "förvalt det som återstår"
    );
    assert!(panel.contains(r#"value="1930" selected"#), "{panel}");
    // Till konto: bara kassa och bank (19xx), inte vilken tillgång som helst.
    assert!(panel.contains(r#"<option value="1930""#), "{panel}");
    assert!(!panel.contains(r#"<option value="1030""#), "{panel}");
    assert!(!panel.contains(r#"<option value="1510""#), "{panel}");
    assert!(panel.contains("Bokförs som verifikat"));
    assert!(panel.contains("1510"));
    assert!(panel.contains(&format!("{bas}/kundfakturor/{f}/betalning")));

    let betald = plats(
        &posta(
            &db,
            &session,
            &format!("{bas}/kundfakturor/{f}/betalning"),
            "betaldag=2026-04-01&belopp=5000&konto=1930",
        )
        .await,
    );
    assert!(betald.contains("betald=1"), "{betald}");
    let dokument = hämta(&db, &session, &betald).await;
    assert!(dokument.contains("Betalningen registrerades"));
    assert!(
        dokument.contains("badge-forfallen"),
        "fortfarande förfallen"
    );
    let betalningar = efter(&dokument, "<h3>Betalningar");
    assert!(betalningar.contains("2026-04-01"));
    assert!(betalningar.contains("5 000,00"));
    assert!(betalningar.contains("6 875,00"), "det som återstår");
    assert!(betalningar.contains("Rätta betalning…"));

    // Panelen förväljer det som återstår efter delbetalningen.
    let panel = hämta(&db, &session, &format!("{här}&panel=betalning")).await;
    assert!(efter(&panel, "betalning-rubrik").contains(r#"value="6 875,00""#));
}

#[tokio::test]
async fn en_överbetalning_visar_spärren_vid_beloppet() {
    let db = Databas::tillfällig().await;
    let (session, bas, kund) = fakturaföretag(&db).await;
    let här = utfärdad(&db, &session, &bas, &kund).await;
    let f = parameter(&här, "f");
    let tillbaka = plats(
        &posta(
            &db,
            &session,
            &format!("{bas}/kundfakturor/{f}/betalning"),
            "betaldag=2026-04-01&belopp=20000&konto=1930",
        )
        .await,
    );
    assert!(tillbaka.contains("panel=betalning"), "{tillbaka}");
    assert!(tillbaka.contains("fel=betalning"), "{tillbaka}");
    let sida = hämta(&db, &session, &tillbaka).await;
    assert!(sida.contains(r#"data-kod="overbetalning""#), "{sida}");
    assert!(sida.contains("manuellt verifikat"));
    assert!(
        sida.contains(r#"value="20000""#),
        "det som användaren skrev"
    );
}

#[tokio::test]
async fn en_rest_på_några_kronor_kan_stängas_mot_öresutjämning() {
    let db = Databas::tillfällig().await;
    let (session, bas, kund) = fakturaföretag(&db).await;
    let här = utfärdad(&db, &session, &bas, &kund).await;
    let f = parameter(&här, "f");
    let betald = plats(
        &posta(
            &db,
            &session,
            &format!("{bas}/kundfakturor/{f}/betalning"),
            "betaldag=2026-04-01&belopp=11870&konto=1930&oresutjamning=1",
        )
        .await,
    );
    assert!(betald.contains("betald=1"), "{betald}");
    let dokument = hämta(&db, &session, &betald).await;
    assert!(dokument.contains("badge-betald"), "{dokument}");
    assert!(efter(&dokument, "<h3>Betalningar").contains("5,00 öresutjämning"));
    assert!(
        !dokument.contains("panel=betalning"),
        "inget kvar att betala"
    );
}

#[tokio::test]
async fn rätta_betalning_vänder_verifikatet_och_det_som_återstår_blir_som_före() {
    let db = Databas::tillfällig().await;
    let (session, bas, kund) = fakturaföretag(&db).await;
    let här = utfärdad(&db, &session, &bas, &kund).await;
    let f = parameter(&här, "f");
    posta(
        &db,
        &session,
        &format!("{bas}/kundfakturor/{f}/betalning"),
        "betaldag=2026-04-01&belopp=11875&konto=1930",
    )
    .await;
    let dokument = hämta(&db, &session, &här).await;
    assert!(dokument.contains("badge-betald"));
    let b = dokument
        .split("panel=ratta-betalning&amp;b=")
        .nth(1)
        .expect("länken Rätta betalning")
        .split('"')
        .next()
        .unwrap()
        .to_string();

    let panel = hämta(&db, &session, &format!("{här}&panel=ratta-betalning&b={b}")).await;
    let panel = efter(&panel, "ratta-betalning-rubrik");
    assert!(panel.contains("vänds med en rättelse"));
    assert!(panel.contains(&format!("{bas}/kundfakturor/{f}/betalningar/{b}/ratta")));

    // Ett datum före betaldagen spärras, med texten i panelen.
    let tillbaka = plats(
        &posta(
            &db,
            &session,
            &format!("{bas}/kundfakturor/{f}/betalningar/{b}/ratta"),
            "datum=2026-03-01&orsak=fel",
        )
        .await,
    );
    assert!(tillbaka.contains("fel=ratta"), "{tillbaka}");
    let sida = hämta(&db, &session, &tillbaka).await;
    assert!(sida.contains("Tidigast 2026-04-01"), "{sida}");

    let rättad = plats(
        &posta(
            &db,
            &session,
            &format!("{bas}/kundfakturor/{f}/betalningar/{b}/ratta"),
            "datum=2026-10-01&orsak=fel+kund",
        )
        .await,
    );
    assert!(rättad.contains("rattad=1"), "{rättad}");
    let dokument = hämta(&db, &session, &rättad).await;
    assert!(dokument.contains("badge-forfallen"), "{dokument}");
    assert!(dokument.contains("11 875,00 kvar"));
    let betalningar = efter(&dokument, "<h3>Betalningar");
    assert!(betalningar.contains("Rättad"));
    assert!(betalningar.contains("fel kund"));
}

#[tokio::test]
async fn listan_grupperas_och_rubriken_har_kundfordringarna() {
    let db = Databas::tillfällig().await;
    let (session, bas, kund) = fakturaföretag(&db).await;
    let här = utfärdad(&db, &session, &bas, &kund).await;
    let f = parameter(&här, "f");
    posta(
        &db,
        &session,
        &format!("{bas}/kundfakturor"),
        &format!(
            "{}&handling=spara",
            utkast(&kund).replace("startnummer=1007", "startnummer=")
        ),
    )
    .await;
    let lista = hämta(&db, &session, &format!("{bas}/kundfakturor")).await;
    assert!(lista.contains("Kundfordringar 11 875,00 kr"), "{lista}");
    let förfallna = lista.find(">Förfallna<").expect("Förfallna");
    let utkast = lista.find(">Utkast<").expect("Utkast");
    assert!(förfallna < utkast);
    assert!(lista.contains("förföll för"));
    // Berättelse 70: gruppens summa av fordringarna står i rubriken.
    assert!(
        efter(&lista, ">Förfallna<").contains(" · 1 · 11 875,00 kr</span>"),
        "{lista}"
    );

    posta(
        &db,
        &session,
        &format!("{bas}/kundfakturor/{f}/betalning"),
        "betaldag=2026-04-01&belopp=11875&konto=1930",
    )
    .await;
    let lista = hämta(&db, &session, &format!("{bas}/kundfakturor")).await;
    assert!(lista.contains("Kundfordringar 0,00 kr"), "{lista}");
    assert!(lista.contains(">Avslutade<"));
    assert!(!lista.contains(">Förfallna<"));
}

#[tokio::test]
async fn kundens_detalj_visar_fakturorna_och_det_obetalda() {
    let db = Databas::tillfällig().await;
    let (session, bas, kund) = fakturaföretag(&db).await;
    let här = utfärdad(&db, &session, &bas, &kund).await;
    let f = parameter(&här, "f");
    posta(
        &db,
        &session,
        &format!("{bas}/kundfakturor/{f}/betalning"),
        "betaldag=2026-04-01&belopp=1875&konto=1930",
    )
    .await;
    let sida = hämta(&db, &session, &format!("{bas}/kunder?k={kund}")).await;
    let fakturor = efter(&sida, "kundens-fakturor");
    assert!(fakturor.contains("Obetalt 10 000,00 kr"), "{fakturor}");
    assert!(fakturor.contains(&format!("{bas}/kundfakturor?f={f}")));
    assert!(fakturor.contains("1007"));
    assert!(fakturor.contains("badge-forfallen"));
}

#[tokio::test]
async fn den_som_saknar_åtkomst_får_inte_registrera_eller_rätta_en_betalning() {
    let db = Databas::tillfällig().await;
    let (session, _, _) = fakturaföretag(&db).await;
    let svar = posta(
        &db,
        &session,
        "/foretag/4711/kundfakturor/1/betalning",
        "betaldag=2026-04-01&belopp=100&konto=1930",
    )
    .await;
    assert_eq!(svar.status(), StatusCode::FORBIDDEN);
    let svar = posta(
        &db,
        &session,
        "/foretag/4711/kundfakturor/1/betalningar/1/ratta",
        "datum=2026-10-01",
    )
    .await;
    assert_eq!(svar.status(), StatusCode::FORBIDDEN);
}
