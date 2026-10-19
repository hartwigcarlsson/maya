//! Kreditfaktura till kund i webben, genom routern (#140): panelen Kreditfaktura…, utkastet med
//! hänvisningen, dokumentens länkar åt båda hållen, märkena och tillgodohavandet som återbetalas.

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

/// Skapar kreditfakturans utkast med `val` i panelen och ger utkastets id.
async fn kreditutkast(
    db: &TillfälligDatabas,
    session: &str,
    bas: &str,
    f: &str,
    val: &str,
) -> String {
    let utkast = plats(
        &posta(
            db,
            session,
            &format!("{bas}/kundfakturor/{f}/kreditera"),
            val,
        )
        .await,
    );
    assert!(utkast.contains("kreditutkast=1"), "{utkast}");
    parameter(&utkast, "f")
}

async fn utfärda(db: &TillfälligDatabas, session: &str, bas: &str, k: &str) {
    let utfärdad = plats(&posta(db, session, &format!("{bas}/kundfakturor/{k}/utfarda"), "").await);
    assert!(utfärdad.contains("utfardad=1"), "{utfärdad}");
}

#[tokio::test]
async fn kreditfaktura_i_panelen_ger_ett_utkast_som_utfärdas_och_länkar_åt_båda_hållen() {
    let db = Databas::tillfällig().await;
    let (session, bas, kund) = fakturaföretag(&db).await;
    let här = utfärdad(&db, &session, &bas, &kund).await;
    let f = parameter(&här, "f");

    let dokument = hämta(&db, &session, &här).await;
    assert!(dokument.contains("Kreditfaktura…"), "{dokument}");
    assert!(dokument.contains(&i_html(&format!("{här}&panel=kreditera"))));
    let panel = hämta(&db, &session, &format!("{här}&panel=kreditera")).await;
    let panel = efter(&panel, "kreditera-rubrik");
    assert!(panel.contains("Hela fakturan"), "{panel}");
    assert!(panel.contains(r#"name="antal_0""#));
    assert!(panel.contains(&format!("{bas}/kundfakturor/{f}/kreditera")));

    let k = kreditutkast(&db, &session, &bas, &f, "hela=1").await;
    let utkast = hämta(
        &db,
        &session,
        &format!("{bas}/kundfakturor?f={k}&kreditutkast=1"),
    )
    .await;
    assert!(
        utkast.contains("Kreditfakturans utkast skapades"),
        "{utkast}"
    );
    assert!(utkast.contains("Krediterar faktura 1007 daterad 2026-03-10"));
    assert!(utkast.contains(r#"value="-10""#));
    utfärda(&db, &session, &bas, &k).await;

    let kredit = hämta(&db, &session, &format!("{bas}/kundfakturor?f={k}")).await;
    assert!(kredit.contains("Kreditfaktura 1008"), "{kredit}");
    assert!(kredit.contains("badge-kreditfaktura"));
    assert!(
        kredit.contains(&format!("{bas}/kundfakturor?f={f}")),
        "länken till originalet"
    );
    assert!(!kredit.contains("Registrera betalning"));
    assert!(!kredit.contains("Kreditfaktura…"));
    let original = hämta(&db, &session, &här).await;
    assert!(original.contains("badge-krediterad"), "{original}");
    let krediteras = efter(&original, "Krediteras av");
    assert!(krediteras.contains(&format!("{bas}/kundfakturor?f={k}")));
    assert!(krediteras.contains("1008"));
    assert!(
        !original.contains("Kreditfaktura…"),
        "inget kvar att kreditera"
    );
    assert!(original.contains("kreditfaktura utfärdad"), "historiken");

    let lista = hämta(&db, &session, &format!("{bas}/kundfakturor")).await;
    assert!(lista.contains("badge-kreditfaktura"));
}

#[tokio::test]
async fn ett_antal_per_rad_krediteras_och_för_många_spärras_vid_raden() {
    let db = Databas::tillfällig().await;
    let (session, bas, kund) = fakturaföretag(&db).await;
    let här = utfärdad(&db, &session, &bas, &kund).await;
    let f = parameter(&här, "f");

    let fel = plats(
        &posta(
            &db,
            &session,
            &format!("{bas}/kundfakturor/{f}/kreditera"),
            "antal_0=11",
        )
        .await,
    );
    assert!(fel.contains("panel=kreditera"), "{fel}");
    let panel = hämta(&db, &session, &fel).await;
    let panel = efter(&panel, "kreditera-rubrik");
    assert!(panel.contains("Ange ett antal från 0 till 10."), "{panel}");
    assert!(panel.contains(r#"value="11""#), "det som skrevs står kvar");

    let k = kreditutkast(&db, &session, &bas, &f, "antal_0=4").await;
    let utkast = hämta(&db, &session, &format!("{bas}/kundfakturor?f={k}")).await;
    assert!(utkast.contains(r#"value="-4""#), "{utkast}");
    utfärda(&db, &session, &bas, &k).await;
    let original = hämta(&db, &session, &här).await;
    assert!(original.contains("7 125,00 kvar"), "{original}");
    assert!(
        original.contains("Kreditfaktura…"),
        "6 timmar finns kvar att kreditera"
    );
}

#[tokio::test]
async fn kredit_av_en_betald_faktura_ger_ett_tillgodohavande_som_återbetalas_i_dokumentet() {
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
    let k = kreditutkast(&db, &session, &bas, &f, "antal_0=4").await;
    utfärda(&db, &session, &bas, &k).await;

    let dokument = hämta(&db, &session, &här).await;
    assert!(dokument.contains("badge-tillgodo"), "{dokument}");
    // Berättelse 71: märket säger att kunden har tillgodo.
    assert!(
        dokument.contains(r#"badge-tillgodo">Kunden har tillgodo<"#),
        "{dokument}"
    );
    assert!(dokument.contains("Kunden har tillgodo 4 750,00 kr"));
    assert!(dokument.contains(&i_html(&format!("{här}&panel=aterbetala"))));
    let sida = hämta(&db, &session, &format!("{bas}/kunder?k={kund}")).await;
    assert!(
        efter(&sida, "kundens-fakturor").contains("Tillgodo 4 750,00 kr"),
        "{sida}"
    );

    let panel = hämta(&db, &session, &format!("{här}&panel=aterbetala")).await;
    let panel = efter(&panel, "betalning-rubrik");
    assert!(panel.contains("Återbetala tillgodohavande"), "{panel}");
    assert!(panel.contains(r#"value="4 750,00""#));
    let återbetald = plats(
        &posta(
            &db,
            &session,
            &format!("{bas}/kundfakturor/{f}/betalning"),
            // Kreditfakturan är daterad i dag, och återbetalningen dateras tidigast då (#151).
            &format!(
                "betaldag={}&belopp=4750&konto=1930&aterbetalning=1",
                Klocka::system().idag()
            ),
        )
        .await,
    );
    assert!(återbetald.contains("aterbetald=1"), "{återbetald}");
    let dokument = hämta(&db, &session, &återbetald).await;
    assert!(
        dokument.contains("Återbetalningen registrerades"),
        "{dokument}"
    );
    assert!(dokument.contains("badge-betald"));
    assert!(efter(&dokument, "<h3>Betalningar").contains("-4 750,00"));
}
