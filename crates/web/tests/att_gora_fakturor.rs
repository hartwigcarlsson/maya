//! Att göra med fakturorna i webben, genom routern (#152): förfallna kund- och
//! leverantörsfakturor under Försenat, leverantörsfakturor att betala på sin förfallodag och
//! nedtonade längre fram, kopieringschipsen, betalningen från kortet och nyckeltalen
//! Kundfordringar och Leverantörsskulder.
//!
//! Routern använder systemets klocka, så förfallodagarna räknas från i dag.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use http_body_util::BodyExt;
use jiff::ToSpan;
use jiff::civil::Date;
use maya_core::aktor::Aktör;
use maya_core::config::Config;
use maya_core::foretag::FöretagsId;
use maya_core::leverantorsfaktura::{self, LeverantörsfakturaId};
use maya_core::{Databas, Klocka, TillfälligDatabas, testhjalp};
use maya_domain::leverantorsfaktura::{NyLeverantörsfaktura, NyRad};
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
    assert_eq!(response.status(), StatusCode::OK, "{path}: {body}");
    body
}

async fn posta(db: &TillfälligDatabas, session: &str, path: &str, body: &str) -> String {
    let (response, _) = skicka(
        db,
        Request::post(path)
            .header(header::HOST, HOST)
            .header(header::ORIGIN, ORIGIN)
            .header(header::COOKIE, format!("maya_session={session}"))
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(body.to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER, "{path}");
    response.headers()[header::LOCATION]
        .to_str()
        .unwrap()
        .to_string()
}

fn idag() -> Date {
    Klocka::system().idag()
}

fn om(dagar: i64) -> Date {
    idag().checked_add(dagar.days()).unwrap()
}

fn efter<'a>(sida: &'a str, text: &str) -> &'a str {
    &sida[sida
        .find(text)
        .unwrap_or_else(|| panic!("{text} saknas i {sida}"))..]
}

/// Kortet med id `nyckel`: från dess början till nästa kort.
fn kortet<'a>(sida: &'a str, nyckel: &str) -> &'a str {
    let kort = efter(sida, &format!(r#"id="{nyckel}""#));
    match kort[1..].find(r#"<div class="acard"#) {
        Some(slut) => &kort[..slut + 1],
        None => kort,
    }
}

/// En registrerad leverantörsfaktura på 1 250 kr från en ny leverantör `namn` med bankgiro
/// 5050-1055, fakturerad 2026-09-20.
async fn leverantörsfaktura(
    db: &Databas,
    anna: &Aktör,
    id: FöretagsId,
    namn: &str,
    förfallodag: Date,
    referens: &str,
) -> LeverantörsfakturaId {
    let leverantör = testhjalp::leverantör(db, anna, id, namn).await;
    let klocka = Klocka::system();
    let (f, _) = leverantorsfaktura::ladda_upp(
        db,
        &klocka,
        anna,
        id,
        ("faktura.pdf", "application/pdf", b"%PDF-1.4 faktura"),
    )
    .await
    .unwrap();
    leverantorsfaktura::registrera(
        db,
        &klocka,
        anna,
        id,
        f,
        &NyLeverantörsfaktura {
            leverantör: leverantör.to_string(),
            fakturanummer: format!("{namn}-1"),
            fakturadatum: "2026-09-20".into(),
            förfallodag: förfallodag.to_string(),
            referens: referens.into(),
            totalbelopp: "1 250,00".into(),
            rader: vec![NyRad {
                konto: String::new(),
                netto: "1 000".into(),
                momssats: "25".into(),
                moms: String::new(),
            }],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    f
}

/// Ett fakturaföretag med en förfallen kundfaktura till Kund AB och tre leverantörsfakturor:
/// Telia förföll för 4 dagar sedan, Kontor förfaller om 3 dagar och Lokaler om 40 dagar.
async fn fakturor(db: &TillfälligDatabas) -> (String, String, String, [LeverantörsfakturaId; 3]) {
    let (anna, id) = testhjalp::fakturaföretag(db).await;
    let session = testhjalp::session(db, &Klocka::system(), &anna).await;
    let kund = testhjalp::utfärdad_kundfaktura(db, &anna, id, "Kund AB", "2026-03-10").await;
    let telia = leverantörsfaktura(db, &anna, id, "Telia", om(-4), "8812 0044 711").await;
    let kontor = leverantörsfaktura(db, &anna, id, "Kontor", om(3), "Order 55120").await;
    let lokaler = leverantörsfaktura(db, &anna, id, "Lokaler", om(40), "").await;
    (
        session,
        format!("/foretag/{id}"),
        kund.id.to_string(),
        [telia, kontor, lokaler],
    )
}

#[tokio::test]
async fn förfallna_fakturor_står_först_och_leverantörsfakturorna_på_sin_förfallodag() {
    let db = Databas::tillfällig().await;
    let (session, bas, kund, [telia, kontor, lokaler]) = fakturor(&db).await;

    let sida = hämta(&db, &session, &bas).await;

    let tidslinje = efter(&sida, r#"class="ag-tidslinje""#);
    let försenat = efter(tidslinje, "Försenat");
    let (k, t, ko, l) = (
        försenat
            .find(&format!(r#"id="kundfaktura-{kund}""#))
            .expect(försenat),
        försenat
            .find(&format!(r#"id="leverantorsfaktura-{telia}""#))
            .expect(försenat),
        försenat
            .find(&format!(r#"id="leverantorsfaktura-{kontor}""#))
            .expect(försenat),
        försenat
            .find(&format!(r#"id="leverantorsfaktura-{lokaler}""#))
            .expect(försenat),
    );
    assert!(k < t && t < ko && ko < l, "{försenat}");
    // Kontor står under sin förfallodag, efter Försenat.
    let dag = försenat[t..ko]
        .rfind("ag-day")
        .map(|i| t + i)
        .expect(försenat);
    assert!(försenat[dag..ko].contains("om 3 dagar"), "{försenat}");
    // Längre fram än 7 dagar är kortet nedtonat.
    assert!(
        sida.contains(&format!(
            r#"id="leverantorsfaktura-{lokaler}" class="acard nedtonad""#
        )),
        "{sida}"
    );
    let kundkortet = kortet(&sida, &format!("kundfaktura-{kund}"));
    assert!(kundkortet.contains("Kund AB"), "{kundkortet}");
    assert!(kundkortet.contains("badge-forfallen"), "{kundkortet}");
    assert!(kundkortet.contains("11 875,00"), "{kundkortet}");
    let teliakortet = kortet(&sida, &format!("leverantorsfaktura-{telia}"));
    assert!(teliakortet.contains("Telia"), "{teliakortet}");
    assert!(
        teliakortet.contains("förföll för 4 dagar sedan"),
        "{teliakortet}"
    );
    // Försenat: kundfakturan och Telia; inom 7 dagar: Kontor.
    assert!(sida.contains("2 försenade · 1 inom 7 dagar"), "{sida}");

    // Nyckeltalen, mellan Företagskontot och Resultat hittills i år.
    let nyckeltal = efter(&sida, r#"class="nyck""#);
    let ordning: Vec<usize> = [
        "Företagskontot",
        "Kundfordringar",
        "Leverantörsskulder",
        "Resultat hittills i år",
    ]
    .iter()
    .map(|n| nyckeltal.find(n).expect(nyckeltal))
    .collect();
    assert!(ordning.is_sorted(), "{nyckeltal}");
    let ruta = |namn: &str| {
        let ruta = efter(nyckeltal, namn);
        ruta[..ruta.find(r#"class="stat""#).unwrap_or(ruta.len())].to_string()
    };
    let kundfordringar = ruta("Kundfordringar");
    assert!(
        kundfordringar.contains("<b>11 875,00 kr</b>"),
        "{kundfordringar}"
    );
    assert!(
        kundfordringar.contains("varav förfallet 11 875,00 kr · 1510 ur bokföringen</span>"),
        "{kundfordringar}"
    );
    let skulder = ruta("Leverantörsskulder");
    assert!(skulder.contains("<b>3 750,00 kr</b>"), "{skulder}");
    assert!(
        skulder.contains("varav förfallet 1 250,00 kr · 2440 ur bokföringen</span>"),
        "{skulder}"
    );
}

#[tokio::test]
async fn kortet_för_en_leverantörsfaktura_har_kopieringschipsen() {
    let db = Databas::tillfällig().await;
    let (session, bas, _, [telia, kontor, _]) = fakturor(&db).await;

    let sida = hämta(
        &db,
        &session,
        &format!("{bas}?oppna=leverantorsfaktura-{telia}"),
    )
    .await;

    let kort = kortet(&sida, &format!("leverantorsfaktura-{telia}"));
    let chips = efter(kort, r#"class="chips""#);
    for text in [
        r#"<span class="k">BG</span>5050-1055"#,
        r#"<span class="k">OCR</span>88120044711"#,
        r#"<span class="k">Belopp</span>1 250,00"#,
    ] {
        assert!(chips.contains(text), "{text}: {chips}");
    }
    assert!(
        kort.contains(&format!("{bas}/leverantorsfakturor?f={telia}")),
        "{kort}"
    );

    let sida = hämta(
        &db,
        &session,
        &format!("{bas}?oppna=leverantorsfaktura-{kontor}"),
    )
    .await;
    let kort = kortet(&sida, &format!("leverantorsfaktura-{kontor}"));
    assert!(
        kort.contains(r#"<span class="k">Meddelande</span>Order 55120"#),
        "{kort}"
    );
}

#[tokio::test]
async fn en_kundbetalning_registreras_i_kortet_och_bokförs_som_från_fakturan() {
    let db = Databas::tillfällig().await;
    let (session, bas, kund, _) = fakturor(&db).await;
    let nyckel = format!("kundfaktura-{kund}");

    let sida = hämta(&db, &session, &format!("{bas}?oppna={nyckel}")).await;
    let kort = kortet(&sida, &nyckel);
    assert!(kort.contains("Kontakta kunden själv"), "{kort}");
    assert!(kort.contains("ekonomi@kund.se"), "{kort}");
    assert!(
        kort.contains(&format!("{bas}?oppna={nyckel}&amp;panel=betalning")),
        "{kort}"
    );

    // Betalningspanelen fälls ut i kortet, förvald med det som återstår.
    let sida = hämta(
        &db,
        &session,
        &format!("{bas}?oppna={nyckel}&panel=betalning"),
    )
    .await;
    let kort = kortet(&sida, &nyckel);
    let action = format!("{bas}/att-gora/kundfakturor/{kund}/betalning");
    assert!(kort.contains(&format!(r#"action="{action}""#)), "{kort}");
    assert!(kort.contains(r#"value="11 875,00""#), "{kort}");

    // En överbetalning spärras och panelen visas igen med beloppet.
    let tillbaka = posta(
        &db,
        &session,
        &action,
        &format!("betaldag={}&belopp=20000&konto=1930", idag()),
    )
    .await;
    assert!(
        tillbaka.starts_with(&format!("{bas}?oppna={nyckel}&panel=betalning")),
        "{tillbaka}"
    );
    let sida = hämta(&db, &session, &tillbaka).await;
    let kort = kortet(&sida, &nyckel);
    assert!(kort.contains(r#"value="20000""#), "{kort}");
    assert!(
        kort.contains("Betalningen kunde inte registreras"),
        "{kort}"
    );

    let klar = posta(
        &db,
        &session,
        &action,
        &format!("betaldag={}&belopp=11+875%2C00&konto=1930", idag()),
    )
    .await;
    let sida = hämta(&db, &session, &klar).await;
    assert!(sida.contains("Betalningen är bokförd som V"), "{sida}");
    assert!(!sida.contains(&format!(r#"id="{nyckel}""#)), "{sida}");
    // Bokförd som från fakturan: dokumentet visar betalningen och att fakturan är betald.
    let dokument = hämta(&db, &session, &format!("{bas}/kundfakturor?f={kund}")).await;
    assert!(dokument.contains("badge-betald"), "{dokument}");
}

#[tokio::test]
async fn en_leverantörsbetalning_registreras_i_kortet_och_bokförs_som_från_fakturan() {
    let db = Databas::tillfällig().await;
    let (session, bas, _, [telia, _, _]) = fakturor(&db).await;
    let nyckel = format!("leverantorsfaktura-{telia}");

    let sida = hämta(&db, &session, &format!("{bas}?oppna={nyckel}")).await;
    let kort = kortet(&sida, &nyckel);
    assert!(
        kort.contains(&format!("{bas}?oppna={nyckel}&amp;panel=betalning")),
        "{kort}"
    );
    assert!(!kort.contains("Kontakta kunden själv"), "{kort}");

    let sida = hämta(
        &db,
        &session,
        &format!("{bas}?oppna={nyckel}&panel=betalning"),
    )
    .await;
    let kort = kortet(&sida, &nyckel);
    let action = format!("{bas}/att-gora/leverantorsfakturor/{telia}/betalning");
    assert!(kort.contains(&format!(r#"action="{action}""#)), "{kort}");
    assert!(kort.contains(r#"value="1 250,00""#), "{kort}");
    // Kopieringschipsen står kvar bredvid panelen.
    assert!(kort.contains(r#"<span class="k">OCR</span>"#), "{kort}");

    let tillbaka = posta(
        &db,
        &session,
        &action,
        &format!("betaldag={}&belopp=2000&konto=1930", idag()),
    )
    .await;
    assert!(
        tillbaka.starts_with(&format!("{bas}?oppna={nyckel}&panel=betalning")),
        "{tillbaka}"
    );

    let klar = posta(
        &db,
        &session,
        &action,
        &format!("betaldag={}&belopp=1250&konto=1930", idag()),
    )
    .await;
    let sida = hämta(&db, &session, &klar).await;
    assert!(sida.contains("Betalningen är bokförd som V"), "{sida}");
    assert!(!sida.contains(&format!(r#"id="{nyckel}""#)), "{sida}");
    // Leverantörsskulderna har inget förfallet kvar.
    let nyckeltal = efter(&sida, "Leverantörsskulder");
    assert!(nyckeltal.contains("inget förfallet"), "{nyckeltal}");
    let dokument = hämta(
        &db,
        &session,
        &format!("{bas}/leverantorsfakturor?f={telia}"),
    )
    .await;
    assert!(dokument.contains("badge-betald"), "{dokument}");
}
