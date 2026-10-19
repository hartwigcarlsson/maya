//! Att göra med Följ upp, fakturautkasten och de nya kunderna och leverantörerna i webben,
//! genom routern (#153): en utfärdad faktura som inte är skickad markeras som lämnad på annat
//! sätt från kortet, alla sorters fakturautkast står per period tillsammans med
//! verifikatutkasten, och de nya kunderna och leverantörerna från utkasten står som streckade
//! kort under Att granska. Utan SMTP; Skicka igen prövas i `utskick.rs`.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use http_body_util::BodyExt;
use maya_core::config::Config;
use maya_core::{Databas, Klocka, TillfälligDatabas, extern_kundfaktura, kundfaktura};
use maya_core::{leverantorsfaktura, testhjalp};
use maya_domain::kund::NyKund;
use maya_domain::kundfaktura::{NY, NyKundfaktura};
use maya_domain::leverantor::NyLeverantör;
use maya_domain::leverantorsfaktura::NyLeverantörsfaktura;
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

/// Det första kortet vars id börjar med `början`, från `id="`.
fn kortet_som_börjar<'a>(sida: &'a str, början: &str) -> &'a str {
    let kort = efter(sida, &format!(r#"id="{början}"#));
    match kort[1..].find(r#"<div class="acard"#) {
        Some(slut) => &kort[..slut + 1],
        None => kort,
    }
}

/// Toppmenyns räknare för Att göra.
fn räknaren(sida: &str) -> String {
    let länk = efter(sida, "Att göra");
    let slut = länk.find("</a>").unwrap();
    länk[..slut].chars().filter(char::is_ascii_digit).collect()
}

#[tokio::test]
async fn en_faktura_som_inte_är_skickad_markeras_som_lämnad_från_följ_upp() {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    let session = testhjalp::session(&db, &Klocka::system(), &anna).await;
    let bas = format!("/foretag/{id}");
    let f = testhjalp::utfärdad_kundfaktura(&db, &anna, id, "Kund AB", "2026-10-01").await;
    let nyckel = format!("utskick-{}", f.id);

    let sida = hämta(&db, &session, &bas).await;
    let före = räknaren(&sida);
    let spalten = efter(&sida, "Utan datum");
    assert!(spalten.contains("Följ upp"), "{spalten}");
    let kort = kortet(&sida, &nyckel);
    assert!(
        kort.contains(&format!("Faktura {} · Kund AB", f.nummer)),
        "{kort}"
    );
    assert!(kort.contains("inte skickad"), "{kort}");

    // Utan SMTP visar kortet bara Lämnad på annat sätt, som går tillbaka till Att göra.
    let sida = hämta(&db, &session, &format!("{bas}?oppna={nyckel}")).await;
    let kort = kortet(&sida, &nyckel);
    assert!(kort.contains("Maya har ingen SMTP-server"), "{kort}");
    let action = format!("{bas}/kundfakturor/{}/lamnad", f.id);
    assert!(kort.contains(&format!(r#"action="{action}""#)), "{kort}");
    assert!(kort.contains(r#"name="efter" value="att_gora""#), "{kort}");
    assert!(kort.contains("Öppna fakturan"), "{kort}");

    let klar = posta(&db, &session, &action, "efter=att_gora&satt=post").await;
    assert_eq!(klar, format!("{bas}?lamnad=1"));
    let sida = hämta(&db, &session, &klar).await;
    assert!(
        sida.contains("Fakturan markerades som lämnad på annat sätt."),
        "{sida}"
    );
    assert!(!sida.contains(&format!(r#"id="{nyckel}""#)), "{sida}");
    assert_eq!(
        räknaren(&sida).parse::<usize>().unwrap() + 1,
        före.parse::<usize>().unwrap(),
        "räknaren i toppmenyn följer listan"
    );
    // Fakturans dokument visar markeringen bland utskicken.
    let dokument = hämta(&db, &session, &format!("{bas}/kundfakturor?f={}", f.id)).await;
    assert!(dokument.contains("Med post"), "{dokument}");
}

#[tokio::test]
async fn fakturautkasten_står_per_period_med_verifikatutkasten_och_de_nya_parterna_under_att_granska()
 {
    let db = Databas::tillfällig().await;
    let (anna, id) = testhjalp::fakturaföretag(&db).await;
    let session = testhjalp::session(&db, &Klocka::system(), &anna).await;
    let bas = format!("/foretag/{id}");
    let klocka = Klocka::system();
    // Ett utkast till en ny kund i september och ett externt utkast utan datum.
    let egen = kundfaktura::spara_utkast(
        &db,
        &klocka,
        &anna,
        id,
        None,
        &NyKundfaktura {
            kund: NY.into(),
            ny_kund: NyKund {
                namn: "Nya Kunden AB".into(),
                epost: "ekonomi@nya.se".into(),
                betalningsvillkor: "10".into(),
                ..Default::default()
            },
            fakturadatum: "2026-09-30".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let (externt, _) = extern_kundfaktura::ladda_upp(
        &db,
        &klocka,
        &anna,
        id,
        ("X-9.pdf", "application/pdf", b"%PDF-1.4 extern"),
    )
    .await
    .unwrap();
    // En leverantörsfaktura från en ny leverantör i september.
    let (lev, _) = leverantorsfaktura::ladda_upp(
        &db,
        &klocka,
        &anna,
        id,
        ("el.pdf", "application/pdf", b"%PDF-1.4 el"),
    )
    .await
    .unwrap();
    leverantorsfaktura::spara_utkast(
        &db,
        &klocka,
        &anna,
        id,
        lev,
        &NyLeverantörsfaktura {
            leverantör: maya_domain::leverantorsfaktura::NY.into(),
            ny_leverantör: NyLeverantör {
                namn: "Elbolaget AB".into(),
                bankgiro: "5050-1055".into(),
                land: "SE".into(),
                ..Default::default()
            },
            fakturanummer: "E-77".into(),
            fakturadatum: "2026-09-12".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    // Ett verifikatutkast i september.
    maya_core::verifikat::spara_utkast(
        &db,
        &klocka,
        &anna,
        id,
        None,
        &maya_domain::verifikat::NyttVerifikat {
            datum: "2026-09-05".into(),
            beskrivning: "Hyra".into(),
            rader: Vec::new(),
        },
    )
    .await
    .unwrap();

    let sida = hämta(&db, &session, &bas).await;
    let spalten = efter(&sida, "Utan datum");
    // Att granska först, med de nya parterna.
    let granska = efter(spalten, "Att granska");
    assert!(granska.find("Att granska") < granska.find(">Utkast<"));
    let ny_kund = kortet_som_börjar(&sida, "nykund-");
    assert!(ny_kund.contains("Nya Kunden AB"), "{ny_kund}");
    assert!(ny_kund.contains("ny kund"), "{ny_kund}");
    let ny_lev = kortet_som_börjar(&sida, "nyleverantor-");
    assert!(ny_lev.contains("Elbolaget AB"), "{ny_lev}");
    // September med verifikatutkastet och de två fakturautkasten, och utkasten utan datum sist.
    let utkast = efter(spalten, "September 2026");
    let september = &utkast[..utkast.find("Utan verifikatdatum").unwrap_or(utkast.len())];
    assert!(september.contains("Hyra"), "{september}");
    assert!(
        september.contains(&format!(r#"id="kundfakturautkast-{egen}""#)),
        "{september}"
    );
    assert!(
        september.contains(&format!(r#"id="leverantorsfakturautkast-{lev}""#)),
        "{september}"
    );
    let kund = kortet(&sida, &format!("kundfakturautkast-{egen}"));
    assert!(kund.contains("Kundfaktura · Nya Kunden AB"), "{kund}");
    assert!(kund.contains("badge-ny"), "{kund}");
    let lev_kort = kortet(&sida, &format!("leverantorsfakturautkast-{lev}"));
    assert!(
        lev_kort.contains("Leverantörsfaktura · E-77 · Elbolaget AB"),
        "{lev_kort}"
    );
    assert!(
        utkast.contains(&format!(r#"id="externutkast-{externt}""#)),
        "utan fakturadatum: {utkast}"
    );

    // Ett fakturautkast fälls ut med länken till sitt eget flöde.
    let nyckel = format!("kundfakturautkast-{egen}");
    let sida = hämta(&db, &session, &format!("{bas}?oppna={nyckel}")).await;
    let kort = kortet(&sida, &nyckel);
    assert!(
        kort.contains(&format!(r#"href="{bas}/kundfakturor?f={egen}""#)),
        "{kort}"
    );
    assert!(kort.contains("Granska och utfärda"), "{kort}");
    let nyckel = format!("leverantorsfakturautkast-{lev}");
    let sida = hämta(&db, &session, &format!("{bas}?oppna={nyckel}")).await;
    let kort = kortet(&sida, &nyckel);
    assert!(
        kort.contains(&format!(r#"href="{bas}/leverantorsfakturor?f={lev}""#)),
        "{kort}"
    );
    assert!(kort.contains("Granska och registrera"), "{kort}");

    // Den nya leverantören fälls ut som ett streckat kort med uppgifterna och utkastet.
    let ny = kortet_som_börjar(&sida, "nyleverantor-");
    let nyckel = ny[4..].split('"').next().unwrap().to_string();
    let sida = hämta(&db, &session, &format!("{bas}?oppna={nyckel}")).await;
    let kort = kortet(&sida, &nyckel);
    assert!(kort.contains("kort-streckat"), "{kort}");
    assert!(kort.contains("5050-1055"), "{kort}");
    assert!(kort.contains("Granska utkastet"), "{kort}");
    assert!(
        kort.contains(&format!(r#"href="{bas}/leverantorsfakturor?f={lev}""#)),
        "{kort}"
    );
}
