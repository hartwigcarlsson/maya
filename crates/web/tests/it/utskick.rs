//! Utskick av kundfakturor genom routern och webbens e-postadapter mot en SMTP-server i testet
//! (#138): mejlet kommer fram med rätt mottagare, svarsadress och den arkiverade PDF:en som
//! bilaga; ett avvisat utskick loggas som misslyckat utan att utfärdandet rullas tillbaka och
//! kan skickas igen; utan SMTP visas ingen kryssruta och fakturan markeras som skickad för hand.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use http_body_util::BodyExt;
use maya_core::aktor::Aktör;
use maya_core::config::Config;
use maya_core::inloggning::{self, NyPasskey};
use maya_core::{Databas, Klocka, TillfälligDatabas};
use maya_pdf::testhjalp::{innehåller, sidor, är_pdfa_2b};
use tower::ServiceExt;

const HOST: &str = "bokforing.example.se";
const ORIGIN: &str = "https://bokforing.example.se";

/// Ett mejl som SMTP-servern tog emot.
#[derive(Debug, Clone)]
struct Mejl {
    till: Vec<String>,
    data: String,
}

impl Mejl {
    fn huvud(&self, namn: &str) -> Option<String> {
        let (huvud, _) = self.data.split_once("\r\n\r\n")?;
        huvud
            .lines()
            .find_map(|r| r.strip_prefix(&format!("{namn}: ")))
            .map(str::to_string)
    }

    /// Delarna i `multipart/mixed`: huvud och avkodad kropp.
    fn delar(&self) -> Vec<(String, Vec<u8>)> {
        let typ = self.huvud("Content-Type").expect("Content-Type");
        let gräns = typ
            .split("boundary=\"")
            .nth(1)
            .and_then(|g| g.split('"').next())
            .unwrap_or_else(|| panic!("ingen gräns i {typ}"));
        self.data
            .split(&format!("--{gräns}"))
            .skip(1)
            .filter(|d| !d.starts_with("--"))
            .map(|del| {
                let (huvud, kropp) = del
                    .trim_start()
                    .split_once("\r\n\r\n")
                    .expect("delens huvud");
                let base64: String = kropp.split_whitespace().collect();
                (huvud.to_string(), avkoda_base64(&base64))
            })
            .collect()
    }
}

fn avkoda_base64(text: &str) -> Vec<u8> {
    const ALFABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut ut = Vec::new();
    let (mut buffert, mut bitar) = (0u32, 0);
    for c in text.bytes().filter(|&c| c != b'=') {
        let v = ALFABET.iter().position(|&a| a == c).expect("base64") as u32;
        buffert = (buffert << 6) | v;
        bitar += 6;
        if bitar >= 8 {
            bitar -= 8;
            ut.push((buffert >> bitar) as u8);
        }
    }
    ut
}

/// En SMTP-server på 127.0.0.1 som sparar mejlen, i klartext utan STARTTLS och inloggning.
struct FalskSmtp {
    port: u16,
    mejl: Arc<Mutex<Vec<Mejl>>>,
}

impl FalskSmtp {
    /// En server som avvisar de `antal` första mejlen med 554 efter DATA.
    fn som_avvisar(antal: usize) -> Self {
        let lyssnare = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = lyssnare.local_addr().unwrap().port();
        let mejl = Arc::new(Mutex::new(Vec::new()));
        let lådan = mejl.clone();
        let avvisa = Arc::new(AtomicUsize::new(antal));
        std::thread::spawn(move || {
            for ström in lyssnare.incoming() {
                let Ok(ström) = ström else { return };
                let (lådan, avvisa) = (lådan.clone(), avvisa.clone());
                std::thread::spawn(move || samtal(ström, &lådan, &avvisa));
            }
        });
        FalskSmtp { port, mejl }
    }

    fn mejl(&self) -> Vec<Mejl> {
        self.mejl.lock().unwrap().clone()
    }

    fn config(&self) -> Config {
        konfiguration(&[
            ("MAYA_SMTP_HOST", "127.0.0.1"),
            ("MAYA_SMTP_PORT", &self.port.to_string()),
            ("MAYA_SMTP_FROM", "maya@example.se"),
        ])
    }
}

fn samtal(ström: TcpStream, lådan: &Mutex<Vec<Mejl>>, avvisa: &AtomicUsize) {
    let mut ut = ström.try_clone().unwrap();
    let mut in_ = BufReader::new(ström);
    let mut svara = |text: &str| ut.write_all(format!("{text}\r\n").as_bytes()).unwrap();
    svara("220 falsk.example.se ESMTP");
    let mut till = Vec::new();
    let mut rad = String::new();
    loop {
        rad.clear();
        if in_.read_line(&mut rad).unwrap_or(0) == 0 {
            return;
        }
        let kommando = rad.trim_end();
        let versaler = kommando.to_ascii_uppercase();
        if versaler.starts_with("EHLO") {
            svara("250-falsk.example.se");
            svara("250 8BITMIME");
        } else if let Some(a) = versaler.strip_prefix("RCPT TO:") {
            till.push(kommando[kommando.len() - a.len()..].to_string());
            svara("250 OK");
        } else if versaler == "DATA" {
            svara("354 Skicka");
            let mut data = String::new();
            loop {
                let mut r = String::new();
                in_.read_line(&mut r).unwrap();
                if r == ".\r\n" {
                    break;
                }
                data.push_str(&r);
            }
            if avvisa
                .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                .is_ok()
            {
                svara("554 Avvisat av mottagaren");
                continue;
            }
            lådan.lock().unwrap().push(Mejl {
                till: std::mem::take(&mut till),
                data,
            });
            svara("250 Mottaget");
        } else if versaler == "QUIT" {
            svara("221 Hej då");
            return;
        } else {
            svara("250 OK");
        }
    }
}

fn konfiguration(extra: &[(&str, &str)]) -> Config {
    Config::from_env(
        [("MAYA_PUBLIC_URL", ORIGIN), ("MAYA_BACKUP_DIR", "/backup")]
            .iter()
            .chain(extra)
            .map(|(k, v)| (k.to_string(), v.to_string())),
    )
    .unwrap()
}

struct Webb {
    db: TillfälligDatabas,
    config: Arc<Config>,
    session: String,
}

impl Webb {
    async fn skicka(&self, request: Request<Body>) -> (Response<()>, Vec<u8>) {
        let app: Router = maya_web::router(self.config.clone(), (*self.db).clone());
        let response = app.oneshot(request).await.unwrap();
        let (parts, body) = response.into_parts();
        let body = body.collect().await.unwrap().to_bytes();
        (Response::from_parts(parts, ()), body.to_vec())
    }

    async fn hämta_svar(&self, path: &str) -> (Response<()>, Vec<u8>) {
        self.skicka(
            Request::get(path)
                .header(header::HOST, HOST)
                .header(header::COOKIE, format!("maya_session={}", self.session))
                .body(Body::empty())
                .unwrap(),
        )
        .await
    }

    async fn hämta(&self, path: &str) -> String {
        let (response, body) = self.hämta_svar(path).await;
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        String::from_utf8(body).unwrap()
    }

    async fn posta(&self, path: &str, body: &str) -> Response<()> {
        self.skicka(
            Request::post(path)
                .header(header::HOST, HOST)
                .header(header::ORIGIN, ORIGIN)
                .header(header::COOKIE, format!("maya_session={}", self.session))
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .0
    }
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

/// Inloggad, ett företag med fakturauppgifter, svarsadress och standardmeddelande, och ett
/// utkast till Kund AB (ekonomi@kund.se) som är granskat: webben, företagets adress och
/// granskningens adress.
async fn granskat_utkast(config: Config) -> (Webb, String, String) {
    let db = Databas::tillfällig().await;
    let länk = inloggning::skapa_första_användare(
        &db,
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
    let session = inloggning::slutför_registrering(&db, &Klocka::system(), länk.token(), ny, None)
        .await
        .unwrap()
        .token()
        .to_string();
    let webb = Webb {
        db,
        config: Arc::new(config),
        session,
    };
    let bas = plats(
        &webb
            .posta(
                "/foretag",
                "orgnr=556036-0793&namn=Exempel+AB&start=2026-01-01&slut=2026-12-31\
                 &bokforingsmetod=faktura&momsperiod=kvartal",
            )
            .await,
    );
    webb.posta(
        &format!("{bas}/installningar/uppgifter"),
        "namn=Exempel+AB&gatuadress=Exempelgatan+1&postnummer=12345&postort=Exempelstad\
         &epost=&telefon=&sate=Stockholm&momsregnr=SE556036079301&f_skatt=ja",
    )
    .await;
    webb.posta(
        &format!("{bas}/installningar/fakturauppgifter"),
        "bankgiro=50501055&plusgiro=&iban=&bic=&svarsadress=faktura%40exempel.se\
         &standardmeddelande=Hej%21+H%C3%A4r+%C3%A4r+fakturan.",
    )
    .await;
    let kund = parameter(
        &plats(
            &webb
                .posta(
                    &format!("{bas}/kunder"),
                    "namn=Kund+AB&identitet=&adress=Storgatan+1%0D%0A111+22+Stockholm\
                     &epost=ekonomi%40kund.se&momsregnr=&betalningsvillkor=30\
                     &omvand_skattskyldighet=ingen",
                )
                .await,
        ),
        "k",
    );
    let granska = plats(
        &webb
            .posta(
                &format!("{bas}/kundfakturor"),
                &format!(
                    "kund={kund}&fakturadatum=2026-03-10&leveransdatum=&forfallodag=\
                     &er_referens=&startnummer=1007\
                     &beskrivning_0=Konsulttimmar&antal_0=10&enhet_0=tim&a_pris_0=950\
                     &rabatt_0=&momssats_0=25&konto_0=3001&handling=granska"
                ),
            )
            .await,
    );
    (webb, bas, granska)
}

/// Den arkiverade PDF:en, hämtad via fakturans länk.
async fn arkiverad_pdf(webb: &Webb, bas: &str, sida: &str) -> Vec<u8> {
    let underlag = sida
        .split(&format!("{bas}/underlag/"))
        .nth(1)
        .unwrap()
        .split('/')
        .next()
        .unwrap();
    let (svar, pdf) = webb
        .hämta_svar(&format!("{bas}/underlag/{underlag}/fil"))
        .await;
    assert_eq!(svar.status(), StatusCode::OK);
    pdf
}

#[tokio::test]
async fn kryssrutan_skickar_mejlet_efter_utfärdandet_med_mottagare_svarsadress_och_pdf_bilagan() {
    let smtp = FalskSmtp::som_avvisar(0);
    let (webb, bas, granska) = granskat_utkast(smtp.config()).await;
    let f = parameter(&granska, "f");

    let dialog = webb.hämta(&granska).await;
    assert!(dialog.contains(r#"name="skicka""#), "kryssrutan");
    assert!(dialog.contains("ekonomi@kund.se"));
    assert!(dialog.contains("när den utfärdats"));

    let utfärdad = plats(
        &webb
            .posta(&format!("{bas}/kundfakturor/{f}/utfarda"), "skicka=1")
            .await,
    );
    assert!(utfärdad.contains("utfardad=1"), "{utfärdad}");
    assert!(utfärdad.contains("skickat=1"), "{utfärdad}");

    let mejl = smtp.mejl();
    assert_eq!(mejl.len(), 1);
    let m = &mejl[0];
    assert_eq!(m.till, ["<ekonomi@kund.se>"]);
    assert_eq!(m.huvud("To").as_deref(), Some("ekonomi@kund.se"));
    assert_eq!(m.huvud("Reply-To").as_deref(), Some("faktura@exempel.se"));
    assert_eq!(
        m.huvud("From").as_deref(),
        Some("\"Exempel AB\" <maya@example.se>")
    );
    assert_eq!(
        m.huvud("Subject").as_deref(),
        Some("=?UTF-8?B?RmFrdHVyYSAxMDA3IGZyw6VuIEV4ZW1wZWwgQUI=?=")
    );
    let delar = m.delar();
    assert_eq!(delar.len(), 2, "text och bilaga");
    assert!(delar[0].0.contains("text/plain"));
    assert_eq!(
        String::from_utf8_lossy(&delar[0].1),
        "Hej! Här är fakturan."
    );
    let (huvud, pdf) = &delar[1];
    assert!(huvud.contains("Content-Type: application/pdf"), "{huvud}");
    assert!(
        huvud.contains("Content-Disposition: attachment; filename=\"Kundfaktura_1007.pdf\""),
        "{huvud}"
    );

    let sida = webb.hämta(&utfärdad).await;
    assert_eq!(
        *pdf,
        arkiverad_pdf(&webb, &bas, &sida).await,
        "den arkiverade PDF:en"
    );
    är_pdfa_2b(pdf);
    assert!(innehåller(&sidor(pdf)[0], "Kund AB"));

    let utskick = &sida[sida.find("<h3>Utskick</h3>").expect("avsnittet Utskick")..];
    assert!(utskick.contains("ekonomi@kund.se"), "{utskick}");
    assert!(utskick.contains("skickad"), "{utskick}");
    assert!(utskick.contains("anna@example.se"), "{utskick}");
    assert!(sida.contains("utskick gjort"), "historiken");
}

#[tokio::test]
async fn ett_avvisat_utskick_loggas_som_misslyckat_utan_att_utfärdandet_rullas_tillbaka() {
    let smtp = FalskSmtp::som_avvisar(1);
    let (webb, bas, granska) = granskat_utkast(smtp.config()).await;
    let f = parameter(&granska, "f");

    let utfärdad = plats(
        &webb
            .posta(&format!("{bas}/kundfakturor/{f}/utfarda"), "skicka=1")
            .await,
    );
    assert!(utfärdad.contains("utfardad=1"), "{utfärdad}");
    assert!(utfärdad.contains("fel=skickfel"), "{utfärdad}");
    assert!(smtp.mejl().is_empty());

    let sida = webb.hämta(&utfärdad).await;
    assert!(sida.contains("Kundfaktura 1007"), "fakturan är utfärdad");
    assert!(sida.contains("Utskicket misslyckades och loggades"));
    let utskick = &sida[sida.find("<h3>Utskick</h3>").unwrap()..];
    assert!(utskick.contains("misslyckades"), "{utskick}");
    assert!(utskick.contains("554 Avvisat av mottagaren"), "{utskick}");
    assert!(sida.contains("utskick misslyckat"), "historiken");

    // Skicka igen från panelen, med den förvalda adressen.
    let panel = webb
        .hämta(&format!("{bas}/kundfakturor?f={f}&panel=skicka"))
        .await;
    assert!(panel.contains("Förra utskicket misslyckades"), "{panel}");
    assert!(panel.contains("Skicka igen"));
    assert!(panel.contains(r#"value="ekonomi@kund.se""#));
    assert!(
        panel.contains(r#"value="faktura@exempel.se""#),
        "svarsadressen"
    );
    assert!(panel.contains("Kundfaktura_1007.pdf"));
    let skickat = plats(
        &webb
            .posta(
                &format!("{bas}/kundfakturor/{f}/skicka"),
                "till=ekonomi%40kund.se&meddelande=Andra+f%C3%B6rs%C3%B6ket",
            )
            .await,
    );
    assert!(skickat.contains("skickat=1"), "{skickat}");
    let mejl = smtp.mejl();
    assert_eq!(mejl.len(), 1);
    assert_eq!(
        String::from_utf8_lossy(&mejl[0].delar()[0].1),
        "Andra försöket"
    );

    // En ogiltig adress skickas inte, och formuläret kommer tillbaka.
    let tillbaka = plats(
        &webb
            .posta(
                &format!("{bas}/kundfakturor/{f}/skicka"),
                "till=ingen+adress&meddelande=Hej",
            )
            .await,
    );
    assert!(tillbaka.contains("fel=mottagare"), "{tillbaka}");
    assert!(tillbaka.contains("panel=skicka"), "{tillbaka}");
    assert_eq!(smtp.mejl().len(), 1);
}

#[tokio::test]
async fn utan_smtp_visas_ingen_kryssruta_och_fakturan_markeras_som_skickad_för_hand() {
    let (webb, bas, granska) = granskat_utkast(konfiguration(&[])).await;
    let f = parameter(&granska, "f");

    let dialog = webb.hämta(&granska).await;
    assert!(dialog.contains("Granska och utfärda"));
    assert!(
        !dialog.contains(r#"name="skicka""#),
        "ingen kryssruta utan SMTP"
    );

    // Rutan ignoreras utan SMTP.
    let utfärdad = plats(
        &webb
            .posta(&format!("{bas}/kundfakturor/{f}/utfarda"), "skicka=1")
            .await,
    );
    assert!(!utfärdad.contains("skick"), "{utfärdad}");
    let sida = webb.hämta(&utfärdad).await;
    assert!(sida.contains("Markera som skickad…"));
    assert!(sida.contains("Inte skickad än."));

    let panel = webb
        .hämta(&format!("{bas}/kundfakturor?f={f}&panel=skicka"))
        .await;
    assert!(panel.contains("Maya har ingen SMTP-server"), "{panel}");
    assert!(!panel.contains(r#"name="till""#), "inget e-postformulär");
    assert!(panel.contains("Uppladdad i kundens portal"));

    let svar = webb
        .posta(
            &format!("{bas}/kundfakturor/{f}/skicka"),
            "till=ekonomi%40kund.se&meddelande=Hej",
        )
        .await;
    assert_eq!(svar.status(), StatusCode::CONFLICT);

    let lämnad = plats(
        &webb
            .posta(&format!("{bas}/kundfakturor/{f}/lamnad"), "satt=post")
            .await,
    );
    assert!(lämnad.contains("lamnad=1"), "{lämnad}");
    let sida = webb.hämta(&lämnad).await;
    let utskick = &sida[sida.find("<h3>Utskick</h3>").unwrap()..];
    assert!(utskick.contains("Med post"), "{utskick}");
    assert!(utskick.contains("markerad"), "{utskick}");
    assert!(sida.contains("utskick lämnat på annat sätt"), "historiken");
}

// #153 Följ upp i Att göra

/// Kortet med id `nyckel` i Att göra: från dess början till nästa kort.
fn kortet<'a>(sida: &'a str, nyckel: &str) -> &'a str {
    let kort = &sida[sida
        .find(&format!(r#"id="{nyckel}""#))
        .unwrap_or_else(|| panic!("{nyckel} saknas i {sida}"))..];
    match kort[1..].find(r#"<div class="acard"#) {
        Some(slut) => &kort[..slut + 1],
        None => kort,
    }
}

#[tokio::test]
async fn ett_misslyckat_utskick_följs_upp_och_skickas_igen_från_att_göra() {
    let smtp = FalskSmtp::som_avvisar(2);
    let (webb, bas, granska) = granskat_utkast(smtp.config()).await;
    let f = parameter(&granska, "f");
    let utfärdad = plats(
        &webb
            .posta(&format!("{bas}/kundfakturor/{f}/utfarda"), "skicka=1")
            .await,
    );
    assert!(utfärdad.contains("fel=skickfel"), "{utfärdad}");

    // Under Följ upp med felet och mottagaren.
    let nyckel = format!("utskick-{f}");
    let sida = webb.hämta(&bas).await;
    let spalten = &sida[sida.find("Utan datum").unwrap()..];
    assert!(spalten.contains("Följ upp"), "{spalten}");
    let kort = kortet(&sida, &nyckel);
    assert!(kort.contains("Faktura 1007 · Kund AB"), "{kort}");
    assert!(kort.contains("studsade"), "{kort}");
    assert!(kort.contains("till ekonomi@kund.se misslyckades"), "{kort}");

    // Kortet fälls ut med panelen Skicka, som går tillbaka till Att göra.
    let sida = webb.hämta(&format!("{bas}?oppna={nyckel}")).await;
    let kort = kortet(&sida, &nyckel);
    assert!(kort.contains("Förra utskicket misslyckades"), "{kort}");
    assert!(kort.contains("554 Avvisat av mottagaren"), "{kort}");
    assert!(kort.contains("Skicka igen"), "{kort}");
    assert!(kort.contains(r#"name="efter" value="att_gora""#), "{kort}");
    assert!(kort.contains("Markera som skickad"), "{kort}");

    // Ett nytt försök som också avvisas visar kortet igen med felet.
    let igen = plats(
        &webb
            .posta(
                &format!("{bas}/kundfakturor/{f}/skicka"),
                "efter=att_gora&till=ekonomi%40kund.se&meddelande=Hej",
            )
            .await,
    );
    assert_eq!(igen, format!("{bas}?oppna={nyckel}&fel=skickfel"));
    let sida = webb.hämta(&igen).await;
    assert!(
        sida.contains("Utskicket misslyckades och loggades"),
        "{sida}"
    );

    // En ogiltig adress skickas inte, och kortet kommer tillbaka med den.
    let tillbaka = plats(
        &webb
            .posta(
                &format!("{bas}/kundfakturor/{f}/skicka"),
                "efter=att_gora&till=ingen+adress&meddelande=Hej",
            )
            .await,
    );
    assert!(
        tillbaka.starts_with(&format!("{bas}?oppna={nyckel}&fel=mottagare")),
        "{tillbaka}"
    );
    let sida = webb.hämta(&tillbaka).await;
    assert!(sida.contains(r#"value="ingen adress""#), "{sida}");

    let skickat = plats(
        &webb
            .posta(
                &format!("{bas}/kundfakturor/{f}/skicka"),
                "efter=att_gora&till=ekonomi%40kund.se&meddelande=Tredje+f%C3%B6rs%C3%B6ket",
            )
            .await,
    );
    assert_eq!(skickat, format!("{bas}?skickat=1"));
    assert_eq!(smtp.mejl().len(), 1);
    let sida = webb.hämta(&skickat).await;
    assert!(
        sida.contains("Fakturan skickades med e-post, och utskicket loggades."),
        "{sida}"
    );
    assert!(!sida.contains(&format!(r#"id="{nyckel}""#)), "{sida}");
}
