//! Kundförlust och återkallelse i kundfakturans dokument, genom routern (#141): panelen
//! Konstaterad kundförlust med "Bokförs som", spärrarna för skäl och datum, märket kundförlust i
//! dokumentet och listan, och Återkalla kundförlust som gör fakturan betalbar igen.

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
async fn en_kundförlust_bokförs_i_panelen_och_fakturan_får_märket_kundförlust() {
    let db = Databas::tillfällig().await;
    let (session, bas, kund) = fakturaföretag(&db).await;
    let här = utfärdad(&db, &session, &bas, &kund).await;
    let f = parameter(&här, "f");

    let dokument = hämta(&db, &session, &här).await;
    assert!(dokument.contains(&i_html(&format!("{här}&panel=kundforlust"))));
    assert!(dokument.contains("Kundförlust…"));

    let panel = hämta(&db, &session, &format!("{här}&panel=kundforlust")).await;
    let panel = efter(&panel, "kundforlust-rubrik");
    assert!(panel.contains("Konstaterad kundförlust"));
    for skäl in ["konkurs", "ackord", "utmatning", "annat"] {
        assert!(panel.contains(&format!(r#"value="{skäl}""#)), "{skäl}");
    }
    assert!(panel.contains("Utmätning utan resultat"));
    assert!(panel.contains("11 875,00 kr skrivs bort"), "{panel}");
    let bokförs = efter(&panel, "Bokförs som");
    assert!(bokförs.contains("6351"), "{bokförs}");
    assert!(bokförs.contains("9 500,00"));
    assert!(bokförs.contains("2611"));
    assert!(bokförs.contains("2 375,00"));
    assert!(bokförs.contains("1510"));
    assert!(panel.contains(&format!("{bas}/kundfakturor/{f}/kundforlust")));

    let bokförd = plats(
        &posta(
            &db,
            &session,
            &format!("{bas}/kundfakturor/{f}/kundforlust"),
            "skal=konkurs&annat=&datum=2026-09-30",
        )
        .await,
    );
    assert!(bokförd.contains("kundforlust=1"), "{bokförd}");
    let dokument = hämta(&db, &session, &bokförd).await;
    assert!(dokument.contains("Kundförlusten bokfördes"), "{dokument}");
    assert!(dokument.contains("badge-kundforlust"));
    assert!(
        !dokument.contains("Registrera betalning"),
        "inget kvar att betala"
    );
    assert!(!dokument.contains("Kundförlust…"));
    assert!(dokument.contains("Återkalla kundförlust"));
    let metadata = efter(&dokument, "<dt>Kundförlust</dt>");
    assert!(metadata.contains("2026-09-30"));
    assert!(metadata.contains("Konkurs"));

    let lista = hämta(&db, &session, &format!("{bas}/kundfakturor")).await;
    assert!(lista.contains("badge-kundforlust"), "{lista}");
}

#[tokio::test]
async fn skäl_och_datum_som_saknas_visas_vid_fälten() {
    let db = Databas::tillfällig().await;
    let (session, bas, kund) = fakturaföretag(&db).await;
    let här = utfärdad(&db, &session, &bas, &kund).await;
    let f = parameter(&här, "f");
    let tillbaka = plats(
        &posta(
            &db,
            &session,
            &format!("{bas}/kundfakturor/{f}/kundforlust"),
            "skal=annat&annat=&datum=",
        )
        .await,
    );
    assert!(tillbaka.contains("panel=kundforlust"), "{tillbaka}");
    assert!(tillbaka.contains("fel=kundforlust"), "{tillbaka}");
    let panel = hämta(&db, &session, &tillbaka).await;
    let panel = efter(&panel, "kundforlust-rubrik");
    assert!(panel.contains(r#"data-kod="annat_saknas""#), "{panel}");
    assert!(panel.contains(r#"data-kod="datum_saknas""#), "{panel}");
    assert!(panel.contains(r#"value="annat" checked"#), "{panel}");
    let dokument = hämta(&db, &session, &här).await;
    assert!(!dokument.contains("badge-kundforlust"), "inget bokfördes");
}

#[tokio::test]
async fn återkallelsen_vänder_förlusten_och_fakturan_kan_betalas_igen() {
    let db = Databas::tillfällig().await;
    let (session, bas, kund) = fakturaföretag(&db).await;
    let här = utfärdad(&db, &session, &bas, &kund).await;
    let f = parameter(&här, "f");
    posta(
        &db,
        &session,
        &format!("{bas}/kundfakturor/{f}/kundforlust"),
        "skal=ackord&annat=&datum=2026-09-30",
    )
    .await;
    let dokument = hämta(&db, &session, &här).await;
    assert!(dokument.contains(&i_html(&format!("{här}&panel=aterkalla-kundforlust"))));

    let panel = hämta(&db, &session, &format!("{här}&panel=aterkalla-kundforlust")).await;
    let panel = efter(&panel, "aterkalla-rubrik");
    assert!(panel.contains("Återkalla kundförlust"));
    let bokförs = efter(&panel, "Bokförs som");
    assert!(bokförs.contains("6351"));
    assert!(bokförs.contains("11 875,00"));
    let action = efter(&panel, &format!("{bas}/kundfakturor/{f}/kundforlust/"));
    let action = &action[..action.find('"').unwrap()];
    assert!(action.ends_with("/aterkalla"), "{action}");

    let återkallad = plats(&posta(&db, &session, action, "").await);
    assert!(
        återkallad.contains("kundforlust_aterkallad=1"),
        "{återkallad}"
    );
    let dokument = hämta(&db, &session, &återkallad).await;
    assert!(
        dokument.contains("Kundförlusten återkallades"),
        "{dokument}"
    );
    assert!(!dokument.contains("badge-kundforlust"));
    assert!(dokument.contains("badge-forfallen"));
    assert!(dokument.contains("Registrera betalning"));
    assert!(efter(&dokument, "<dt>Kundförlust</dt>").contains("återkallad"));

    // En andra återkallelse visar felet i dokumentet.
    let igen = plats(&posta(&db, &session, action, "").await);
    assert!(igen.contains("fel=redan_aterkallad"), "{igen}");
}
