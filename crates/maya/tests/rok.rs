//! Röktest med riktig server och en webbläsare med virtuella autentiserare, specens flöde från
//! början till slut: första användaren via CLI-länken, en passkey till, inloggning med e-post
//! och passkey, borttagning av passkeys utom den sista, ett nytt företag, kontoplanen som PDF,
//! ett uppladdat underlag, ett verifikat bokfört från Att göra, en rättelse med registrera om och
//! exporten av balansräkningen och SIE4.
//!
//! Kräver Chrome eller Chromium (sökvägen kan anges i `CHROME`) och ett `cargo leptos build`,
//! så att sidorna hydreras. Utan WASM:en hoppar testet över sig självt, utom i CI.

mod stod;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use headless_chrome::protocol::cdp::{Browser as Webbläsaren, Page, WebAuthn};
use headless_chrome::{Browser, LaunchOptions, Tab};

const MAYA: &str = env!("CARGO_BIN_EXE_maya");
const VÄNTA: Duration = Duration::from_secs(20);

/// Samlar sidans fel (panik vid hydreringen, oväntade undantag) i `window.__fel`.
const SAMLA_FEL: &str = "window.__fel=[];\
const e=console.error.bind(console);\
console.error=(...a)=>{window.__fel.push(a.map(String).join(' '));e(...a)};\
addEventListener('error',h=>window.__fel.push(String(h.message)));\
addEventListener('unhandledrejection',h=>window.__fel.push(String(h.reason)));";

#[test]
fn forsta_anvandaren_via_cli_lanken_och_inloggning_med_passkey() {
    if !stod::sajten_finns() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let port = ledig_port();
    let bas = format!("http://localhost:{port}");
    std::fs::write(
        dir.path().join(".env"),
        format!(
            "MAYA_PUBLIC_URL={bas}\nMAYA_LISTEN=127.0.0.1:{port}\nMAYA_BACKUP_DIR={backup}\n\
             MAYA_DATA_DIR={data}\n",
            backup = dir.path().display(),
            data = dir.path().join("data").display(),
        ),
    )
    .unwrap();
    let _server = Server::starta(dir.path(), port);

    let browser = Browser::new(
        LaunchOptions::default_builder()
            .sandbox(false)
            .idle_browser_timeout(Duration::from_secs(120))
            .build()
            .unwrap(),
    )
    .expect("röktestet kräver Chrome eller Chromium; ange sökvägen i CHROME");
    let tab = browser.new_tab().unwrap();
    // Nedladdningar hamnar i testets katalog, inte i ~/Downloads.
    let nedladdningar = dir.path().join("nedladdningar");
    std::fs::create_dir(&nedladdningar).unwrap();
    tab.call_method(Webbläsaren::SetDownloadBehavior {
        behavior: Webbläsaren::SetDownloadBehaviorBehaviorOption::Allow,
        browser_context_id: None,
        download_path: Some(nedladdningar.display().to_string()),
        events_enabled: None,
    })
    .unwrap();
    tab.call_method(Page::AddScriptToEvaluateOnNewDocument {
        source: SAMLA_FEL.to_string(),
        world_name: None,
        include_command_line_api: None,
        run_immediately: None,
    })
    .unwrap();
    tab.call_method(WebAuthn::Enable {
        enable_ui: Some(false),
    })
    .unwrap();
    let första_nyckeln = autentiserare(&tab);

    // Innan någon användare finns: bara hänvisningen till CLI:t.
    tab.navigate_to(&bas).unwrap();
    vänta_på_text(&tab, "maya user create");
    assert!(tab.find_element("form[data-passkey]").is_err());

    // Driften skapar den första användaren och öppnar länken.
    let länk = skapa_användare(dir.path(), &bas);
    tab.navigate_to(&länk).unwrap();
    vänta_på_shimmen(&tab);
    tab.wait_for_element("#namn")
        .unwrap()
        .type_into("Virtuell nyckel")
        .unwrap();
    tab.find_element("form[data-passkey='registrera'] button")
        .unwrap()
        .click()
        .unwrap();
    vänta_på_text(&tab, "Lägg till en passkey till");

    let kaka = tab
        .get_cookies()
        .unwrap()
        .into_iter()
        .find(|c| c.name == "maya_session")
        .expect("sessionscookien");
    assert!(kaka.http_only, "HttpOnly");
    assert!(kaka.secure, "Secure");
    assert_eq!(format!("{:?}", kaka.same_site), "Some(Lax)", "SameSite=Lax");

    // Länken går inte att använda igen.
    let ny_flik = browser.new_tab().unwrap();
    ny_flik.navigate_to(&länk).unwrap();
    vänta_på_shimmen(&ny_flik);
    ny_flik
        .wait_for_element("form[data-passkey='registrera'] button")
        .unwrap()
        .click()
        .unwrap();
    vänta_på_text(&ny_flik, "länken är ogiltig");
    ny_flik.close(true).unwrap();

    // En passkey till, på en annan enhet. Sessionen är nyss inloggad och har en färsk passkey.
    tab.call_method(WebAuthn::RemoveVirtualAuthenticator {
        authenticator_id: första_nyckeln,
    })
    .unwrap();
    autentiserare(&tab);
    tab.wait_for_element("form[data-passkey='lagg-till'] #namn")
        .unwrap()
        .type_into("Andra nyckeln")
        .unwrap();
    tab.find_element("form[data-passkey='lagg-till'] button")
        .unwrap()
        .click()
        .unwrap();
    vänta_på_text(&tab, "Andra nyckeln");

    // Logga ut och in igen med den andra nyckeln: e-post först, sedan passkey.
    tab.find_element("form[action='/auth/utloggning'] button")
        .unwrap()
        .click()
        .unwrap();
    vänta_på_text(&tab, "Logga in");
    vänta_på_shimmen(&tab);
    tab.wait_for_element("#epost")
        .unwrap()
        .type_into("anna@example.se")
        .unwrap();
    tab.find_element("form[data-passkey='logga-in'] button")
        .unwrap()
        .click()
        .unwrap();
    vänta_på_text(&tab, "Lägg upp ett företag");

    // Den första nyckeln kan tas bort, men inte den sista. Sidorna når man från användarmenyn.
    tab.find_element("a[href='/passkeys']")
        .unwrap()
        .click()
        .unwrap();
    vänta_på_text(&tab, "Virtuell nyckel");
    inga_sidfel(&tab);
    ta_bort_passkey(&tab, "Virtuell nyckel");
    vänta_på_text_borta(&tab, "Virtuell nyckel");
    ta_bort_passkey(&tab, "Andra nyckeln");
    vänta_på_text(&tab, "Den sista passkeyn går inte att ta bort");
    vänta_på_text(&tab, "Andra nyckeln");

    // Sessionslistan visar den här sessionen.
    tab.find_element("a[href='/sessioner']")
        .unwrap()
        .click()
        .unwrap();
    vänta_på_text(&tab, "Den här sessionen");
    inga_sidfel(&tab);
    tab.navigate_to(&bas).unwrap();
    vänta_på_text(&tab, "Lägg upp ett företag");

    // Ett nytt företag: först med fel kontrollsiffra, sedan rätt.
    tab.find_element("a[href='/foretag/nytt']").unwrap();
    tab.navigate_to(&format!("{bas}/foretag/nytt")).unwrap();
    tab.wait_for_element("#orgnr")
        .unwrap()
        .type_into("556036-0794")
        .unwrap();
    tab.find_element("#namn")
        .unwrap()
        .type_into("Exempel AB")
        .unwrap();
    tab.evaluate(
        "document.querySelector('#start').value = '2026-01-01';\
         document.querySelector('#slut').value = '2026-12-31';",
        false,
    )
    .unwrap();
    tab.find_element("form[action='/foretag'] button")
        .unwrap()
        .click()
        .unwrap();
    vänta_på_text(&tab, "Kontrollsiffran stämmer inte");
    tab.evaluate(
        "document.querySelector('#orgnr').value = '556036-0793';\
         document.querySelector('#start').value = '2026-01-01';\
         document.querySelector('#slut').value = '2026-12-31';",
        false,
    )
    .unwrap();
    tab.find_element("form[action='/foretag'] button")
        .unwrap()
        .click()
        .unwrap();
    vänta_på_text(&tab, "Företagskontot");
    vänta_på_text(&tab, "556036-0793");
    assert!(tab.get_url().contains("/foretag/"), "{}", tab.get_url());
    let företaget = tab.get_url();

    // Kontoplanen som PDF: Skriv ut och Exportera är samma PDF, och routern på klienten tar
    // inte över länkarna.
    let kontoplan = format!("{}/installningar/kontoplan", tab.get_url());
    tab.navigate_to(&kontoplan).unwrap();
    vänta_på_text(&tab, "Exportera PDF");
    inga_sidfel(&tab);
    let svar = tab
        .evaluate(
            "(async () => {\
               const skriv = document.querySelector('a.skriv-ut');\
               const exp = document.querySelector('a.exportera-pdf');\
               const r = await fetch(exp.href);\
               const b = new Uint8Array(await r.arrayBuffer());\
               return [skriv.href === exp.href, skriv.target, exp.download, r.status,\
                       r.headers.get('content-type'), String.fromCharCode(...b.slice(0, 5))]\
                       .join(' ');\
             })()",
            true,
        )
        .unwrap()
        .value
        .unwrap();
    assert_eq!(
        svar.as_str().unwrap(),
        "true _blank kontoplan.pdf 200 application/pdf %PDF-"
    );
    tab.find_element("a.exportera-pdf")
        .unwrap()
        .click()
        .unwrap();
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(
        tab.get_url(),
        kontoplan,
        "Exportera laddar ned, sidan står kvar"
    );
    let pdf = nedladdningar.join("kontoplan.pdf");
    for _ in 0..50 {
        if pdf.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(
        pdf.exists(),
        "Exportera sparar kontoplan.pdf i testets katalog"
    );
    inga_sidfel(&tab);

    // Ett underlag laddas upp på sidan Underlag.
    let kvitto = dir.path().join("kvitto.pdf");
    std::fs::write(&kvitto, KVITTO).unwrap();
    tab.navigate_to(&format!("{företaget}/underlag")).unwrap();
    vänta_på_hydreringen(&tab);
    tab.wait_for_element("form.uppladdning input[type='file']")
        .unwrap()
        .set_input_files(&[kvitto.to_str().unwrap()])
        .unwrap();
    klicka(&tab, "form.uppladdning button[type='submit']");
    vänta_på_text(&tab, "kvitto.pdf");
    inga_sidfel(&tab);

    // Att göra: underlaget blir ett kvitto, och utkastet bokförs därifrån.
    klicka(&tab, "nav.flikar a:first-child");
    vänta_på_text(&tab, "okopplat");
    klicka(&tab, ".acard[id^='underlag-'] a.ah");
    klicka(&tab, "form[action$='/kvitto'] button");
    vänta_på_text(&tab, "Ett verifikatutkast skapades med underlaget kopplat.");
    vänta_på_hydreringen(&tab);
    fyll(&tab, "#verifikatformular #datum", "2026-03-14");
    fyll(
        &tab,
        "#verifikatformular #beskrivning",
        "Kaffe till kontoret",
    );
    fyll(&tab, "#verifikatformular [name='konto_0']", "1930");
    fyll(&tab, "#verifikatformular [name='debet_0']", "125");
    fyll(&tab, "#verifikatformular [name='konto_1']", "3001");
    fyll(&tab, "#verifikatformular [name='kredit_1']", "125");
    klicka(
        &tab,
        "#verifikatformular button[value='bokfor']:not([disabled])",
    );
    vänta_på_text(&tab, "Bokförd som V1.");
    inga_sidfel(&tab);

    // Rätta V1 med registrera om: rättelsen blir V2 och kopian bokförs med rätt belopp som V3.
    klicka(&tab, "nav.flikar a[href$='/verifikat']");
    klicka(&tab, "a[href*='/verifikat?v=']");
    vänta_på_text(&tab, "kvitto.pdf");
    klicka(&tab, "a[href*='ratta=1']");
    vänta_på_text(&tab, "Rättelse av V1");
    fyll(&tab, ".rattelse-formular [name='datum']", "2026-03-20");
    fyll(&tab, ".rattelse-formular [name='orsak']", "fel belopp");
    klicka(
        &tab,
        ".rattelse-formular button[value='registrera_om']:not([disabled])",
    );
    vänta_på_text(&tab, "Rättelsen bokfördes som V2.");
    vänta_på_hydreringen(&tab);
    fyll(&tab, "#verifikatformular [name='debet_0']", "150");
    fyll(&tab, "#verifikatformular [name='kredit_1']", "150");
    klicka(
        &tab,
        "#verifikatformular button[value='bokfor']:not([disabled])",
    );
    vänta_på_text(&tab, "V1 (kopia av dess rader)");
    vänta_på_text(&tab, "Bokfört 150,00 kr");
    // Originalet står kvar och visar att det är rättat.
    klicka(&tab, "a[href$='/verifikat?v=1']");
    vänta_på_text(&tab, "Rättad av V2");
    inga_sidfel(&tab);

    // Balansräkningen visar det rättade beloppet och exporteras som PDF och CSV.
    klicka(&tab, "nav.flikar a[href$='/rapporter']");
    klicka(&tab, ".rapportmeny a[href$='/rapporter/balansrakning']");
    vänta_på_text(&tab, "Balansräkning");
    vänta_på_text(&tab, "150,00");
    inga_sidfel(&tab);
    let pdf = hämta(&tab, "a.exportera-pdf");
    assert_eq!(
        (pdf.status, pdf.typ.as_str(), &pdf.text[..5]),
        (200, "application/pdf", "%PDF-")
    );
    let csv = hämta(&tab, "a.exportera-csv");
    assert_eq!(csv.status, 200);
    assert!(csv.typ.starts_with("text/csv"), "{}", csv.typ);
    assert!(
        csv.text.contains("1930") && csv.text.contains("150,00"),
        "{}",
        csv.text
    );

    // SIE4 för året: tre verifikat, och filen avslutas med sin kontrollsumma.
    klicka(&tab, "nav.flikar a[href$='/rapporter']");
    let sie = hämta(&tab, "a[href*='/sie4?ar=2026-01-01']");
    assert_eq!(sie.status, 200, "{}", sie.text);
    assert!(sie.text.starts_with("#FLAGGA 0"), "{}", sie.text);
    assert_eq!(sie.text.matches("#VER ").count(), 3, "{}", sie.text);
    assert!(sie.text.contains("#KSUMMA "), "{}", sie.text);
    inga_sidfel(&tab);
}

/// Ett litet PDF-dokument som underlag.
const KVITTO: &[u8] = b"%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n\
2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n\
3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]>>endobj\n\
trailer<</Root 1 0 R>>\n%%EOF\n";

/// Klickar på elementet med ett DOM-klick, som når elementet även om något annat ligger över
/// dess mittpunkt (ett musklick på koordinater kan träffa grannen).
fn klicka(tab: &Tab, selektor: &str) {
    tab.wait_for_element(selektor)
        .unwrap_or_else(|e| panic!("{selektor} finns inte på {}: {e}", tab.get_url()));
    tab.evaluate(
        &format!("document.querySelector({selektor:?}).click()"),
        false,
    )
    .unwrap();
}

/// Skriver ett värde i ett fält som användaren skulle, så att sidans kontroller ser det.
fn fyll(tab: &Tab, selektor: &str, värde: &str) {
    tab.wait_for_element(selektor)
        .unwrap_or_else(|e| panic!("{selektor} finns inte på {}: {e}", tab.get_url()));
    tab.evaluate(
        &format!(
            "(() => {{ const e = document.querySelector({selektor:?}); e.value = {värde:?}; \
             e.dispatchEvent(new Event('input', {{bubbles: true}})); }})()"
        ),
        false,
    )
    .unwrap();
}

struct Svar {
    status: u16,
    typ: String,
    text: String,
}

/// Hämtar länkens adress från sidan, med sessionscookien, som en nedladdning gör.
fn hämta(tab: &Tab, selektor: &str) -> Svar {
    tab.wait_for_element(selektor)
        .unwrap_or_else(|e| panic!("{selektor} finns inte på {}: {e}", tab.get_url()));
    let svar = tab
        .evaluate(
            &format!(
                "(async () => {{ const r = await fetch(document.querySelector({selektor:?}).href); \
                 return [r.status, r.headers.get('content-type'), await r.text()].join('\\n'); \
                 }})()"
            ),
            true,
        )
        .unwrap()
        .value
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| panic!("{selektor} gav inget svar"));
    let mut delar = svar.splitn(3, '\n');
    let mut del = || delar.next().unwrap_or_default().to_string();
    Svar {
        status: del().parse().unwrap(),
        typ: del(),
        text: del(),
    }
}

/// Väntar tills `hydrate()` har kört klart (den sätter `<html data-hydrerad>`).
fn vänta_på_hydreringen(tab: &Tab) {
    tab.wait_for_element("html[data-hydrerad]").unwrap();
}

fn skapa_användare(dir: &Path, bas: &str) -> String {
    let output = Command::new(MAYA)
        .current_dir(dir)
        .env_clear()
        .args(["user", "create", "anna@example.se"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    stdout
        .split_whitespace()
        .find(|ord| ord.starts_with(&format!("{bas}/registrera/")))
        .unwrap_or_else(|| panic!("ingen länk i {stdout}"))
        .to_string()
}

/// Väntar tills sidan har laddat och JS-shimmen tar emot formulären.
fn vänta_på_shimmen(tab: &Tab) {
    tab.wait_for_element("html[data-passkey-shim]").unwrap();
}

fn vänta_på_text(tab: &Tab, text: &str) {
    let start = Instant::now();
    loop {
        let innehåll = tab
            .evaluate("document.body.innerText", false)
            .ok()
            .and_then(|svar| svar.value)
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default();
        if innehåll.contains(text) {
            return;
        }
        assert!(
            start.elapsed() < VÄNTA,
            "sidan {} visade aldrig \"{text}\":\n{innehåll}",
            tab.get_url()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

struct Server(Child);

impl Server {
    fn starta(dir: &Path, port: u16) -> Self {
        let server = Server(
            Command::new(MAYA)
                .current_dir(dir)
                .env_clear()
                .arg("serve")
                .stdout(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let start = Instant::now();
        loop {
            if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) {
                write!(
                    stream,
                    "GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
                )
                .unwrap();
                let mut svar = String::new();
                stream.read_to_string(&mut svar).unwrap();
                assert!(svar.starts_with("HTTP/1.1 200"), "{svar}");
                return server;
            }
            assert!(start.elapsed() < VÄNTA, "servern startade inte");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
    }
}

fn ledig_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Lägger till en virtuell autentiserare med användarverifiering och ger dess id.
fn autentiserare(tab: &Tab) -> String {
    tab.call_method(WebAuthn::AddVirtualAuthenticator {
        options: WebAuthn::VirtualAuthenticatorOptions {
            protocol: WebAuthn::AuthenticatorProtocol::Ctap2,
            ctap_2_version: Some(WebAuthn::Ctap2Version::Ctap21),
            transport: WebAuthn::AuthenticatorTransport::Internal,
            has_resident_key: Some(true),
            has_user_verification: Some(true),
            is_user_verified: Some(true),
            automatic_presence_simulation: Some(true),
            has_large_blob: None,
            has_cred_blob: None,
            has_min_pin_length: None,
            has_prf: None,
            default_backup_eligibility: None,
            default_backup_state: None,
        },
    })
    .unwrap()
    .authenticator_id
}

/// Skickar Ta bort-formuläret på raden för passkeyn `namn`.
fn ta_bort_passkey(tab: &Tab, namn: &str) {
    let skript = format!(
        "[...document.querySelectorAll('li')]\
         .find(li => li.querySelector('strong')?.textContent === {namn:?})\
         .querySelector(\"form[action='/konto/passkeys/ta-bort'] button\").click()"
    );
    tab.evaluate(&skript, false).unwrap();
}

fn vänta_på_text_borta(tab: &Tab, text: &str) {
    let start = Instant::now();
    loop {
        let innehåll = tab
            .evaluate("document.body.innerText", false)
            .ok()
            .and_then(|svar| svar.value)
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default();
        if !innehåll.is_empty() && !innehåll.contains(text) {
            return;
        }
        assert!(
            start.elapsed() < VÄNTA,
            "\"{text}\" försvann aldrig:\n{innehåll}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Sidan har inte loggat något fel och har hydrerats.
fn inga_sidfel(tab: &Tab) {
    let start = Instant::now();
    loop {
        let fel = tab
            .evaluate("(window.__fel || []).join('\\n')", false)
            .ok()
            .and_then(|svar| svar.value)
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default();
        assert!(
            fel.is_empty(),
            "sidan {} loggade fel:\n{fel}",
            tab.get_url()
        );
        if tab.find_element("html[data-hydrerad]").is_ok() {
            return;
        }
        assert!(
            start.elapsed() < VÄNTA,
            "sidan {} hydrerades aldrig",
            tab.get_url()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}
