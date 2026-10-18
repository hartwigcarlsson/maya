//! Klientnavigering: efter hydreringen navigerar routern på klienten mellan sidorna via
//! menylänkarna, utan omladdning, och varje sida visar sina data (inte en tom eller gammal sida).
//!
//! Kräver Chrome eller Chromium (sökvägen kan anges i `CHROME`) och ett `cargo leptos build`,
//! så att `target/site/pkg/maya.wasm` finns. Utan WASM:en hoppar testet över sig självt.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use headless_chrome::protocol::cdp::{Page, WebAuthn};
use headless_chrome::{Browser, LaunchOptions, Tab};

const MAYA: &str = env!("CARGO_BIN_EXE_maya");
const VÄNTA: Duration = Duration::from_secs(20);

/// Samlar sidans fel (panik via `console_error_panic_hook`, oväntade undantag) i `window.__fel`.
const SAMLA_FEL: &str = "window.__fel=[];\
const e=console.error.bind(console);\
console.error=(...a)=>{window.__fel.push(a.map(String).join(' '));e(...a)};\
addEventListener('error',h=>window.__fel.push(String(h.message)));\
addEventListener('unhandledrejection',h=>window.__fel.push(String(h.reason)));";

#[test]
fn menylankarna_navigerar_pa_klienten_och_sidorna_visar_sina_data() {
    let wasm = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/site/pkg/maya.wasm");
    if !wasm.exists() {
        eprintln!(
            "hoppar över: {} saknas, kör `cargo leptos build`",
            wasm.display()
        );
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let bas = format!("http://localhost:{port}");
    let _server = Server::starta(dir.path(), port);

    let browser = Browser::new(
        LaunchOptions::default_builder()
            .sandbox(false)
            .idle_browser_timeout(Duration::from_secs(120))
            .build()
            .unwrap(),
    )
    .expect("testet kräver Chrome eller Chromium; ange sökvägen i CHROME");
    let tab = browser.new_tab().unwrap();
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
    autentiserare(&tab);

    // Första användaren registrerar sin passkey via CLI-länken och är sedan inloggad.
    tab.navigate_to(&skapa_användare(dir.path(), &bas)).unwrap();
    tab.wait_for_element("html[data-passkey-shim]").unwrap();
    tab.wait_for_element("#namn")
        .unwrap()
        .type_into("Virtuell nyckel")
        .unwrap();
    tab.find_element("form[data-passkey='registrera'] button")
        .unwrap()
        .click()
        .unwrap();
    vänta_på_text(&tab, "Lägg till en passkey till");

    // Ett företag, ett bokfört verifikat och en ingående balans, med vanliga formulärposter.
    let företag = posta(
        &tab,
        "/foretag",
        "orgnr=556036-0793&namn=Exempel+AB&start=2026-01-01&slut=2026-12-31\
         &bokforingsmetod=faktura&momsperiod=kvartal",
    );
    let id = företag
        .strip_prefix(&format!("{bas}/foretag/"))
        .unwrap_or_else(|| panic!("inget företag: {företag}"))
        .to_string();
    posta(
        &tab,
        &format!("/foretag/{id}/verifikat"),
        "handling=bokfor&utkast=&datum=2026-03-14&beskrivning=Kaffe+till+kontoret\
         &konto_0=1930&debet_0=125&kredit_0=&text_0=&konto_1=3001&debet_1=&kredit_1=125&text_1=",
    );
    posta(
        &tab,
        &format!("/foretag/{id}/ingaende-balans"),
        "ar=2026-01-01&konto_0=1930&debet_0=1+000&kredit_0=\
         &konto_1=2081&debet_1=&kredit_1=1000",
    );

    // En enda sidladdning; resten är klick på länkar som routern tar på klienten.
    tab.navigate_to(&format!("{bas}/foretag/{id}")).unwrap();
    vänta_på_hydreringen(&tab);
    tab.evaluate("window.__samma_sida = true", false).unwrap();

    klicka(&tab, "nav.flikar a[href$='/verifikat']");
    vänta_på_text(&tab, "Kaffe till kontoret");
    vänta_på_text(&tab, "Välj ett verifikat i listan.");
    // Samma route med en annan fråga (`?v=`): dokumentet för det valda verifikatet.
    klicka(&tab, "a[href*='/verifikat?v=']");
    vänta_på_text(&tab, "Historik");

    klicka(&tab, "a.kugghjul");
    klicka(&tab, "a[href$='/installningar/kontoplan']");
    vänta_på_text(&tab, "2.26 Kassa, bank och redovisningsmedel");

    klicka(&tab, "a.kugghjul");
    klicka(&tab, "a[href$='/installningar/ingaende-balans']");
    vänta_på_text(&tab, "Aktiekapital");

    klicka(&tab, "a[href='/passkeys']");
    vänta_på_text(&tab, "Virtuell nyckel");
    klicka(&tab, "a[href='/sessioner']");
    vänta_på_text(&tab, "Den här sessionen");

    // Tillbaka till företaget: Att göra och sedan Verifikat igen, med listan som valt verifikat.
    tab.navigate_to(&format!("{bas}/foretag/{id}")).unwrap();
    vänta_på_hydreringen(&tab);
    tab.evaluate("window.__samma_sida = true", false).unwrap();
    klicka(&tab, "nav.flikar a[href$='/verifikat']");
    vänta_på_text(&tab, "Kaffe till kontoret");
    klicka(&tab, "nav.flikar a[href$='/rapporter']");
    klicka(&tab, "nav.flikar a[href$='/verifikat']");
    vänta_på_text(&tab, "Kaffe till kontoret");

    let samma_sida = tab
        .evaluate("window.__samma_sida === true", false)
        .unwrap()
        .value;
    assert_eq!(samma_sida, Some(true.into()), "en länk laddade om sidan");
    assert_eq!(fel(&tab), Vec::<String>::new());
}

/// Postar ett formulär från sidan (samma origin, sessionscookien följer med) och ger adressen
/// som svaret ledde till.
fn posta(tab: &Tab, sökväg: &str, kropp: &str) -> String {
    let skript = format!(
        "fetch({sökväg:?}, {{method: 'POST', \
         headers: {{'content-type': 'application/x-www-form-urlencoded'}}, \
         body: {kropp:?}}}).then(r => r.ok ? r.url : Promise.reject(r.status + ' ' + r.url))"
    );
    tab.evaluate(&skript, true)
        .unwrap()
        .value
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| panic!("{sökväg} gav inget svar"))
}

fn klicka(tab: &Tab, selektor: &str) {
    tab.wait_for_element(selektor)
        .unwrap_or_else(|e| panic!("{selektor} finns inte på {}: {e}", tab.get_url()))
        .click()
        .unwrap();
}

/// Väntar tills `hydrate()` har kört klart (den sätter `<html data-hydrerad>`).
fn vänta_på_hydreringen(tab: &Tab) {
    let start = Instant::now();
    loop {
        let fel = fel(tab);
        assert!(fel.is_empty(), "hydreringen misslyckades: {fel:#?}");
        if tab.find_element("html[data-hydrerad]").is_ok() {
            return;
        }
        assert!(start.elapsed() < VÄNTA, "sidan hydrerades aldrig");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn fel(tab: &Tab) -> Vec<String> {
    tab.evaluate("(window.__fel || []).join('\\u0000')", false)
        .ok()
        .and_then(|svar| svar.value)
        .and_then(|v| v.as_str().map(str::to_string))
        .filter(|s| !s.is_empty())
        .map(|s| s.split('\0').map(str::to_string).collect())
        .unwrap_or_default()
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
            "sidan {} visade aldrig \"{text}\":\n{innehåll}\nfel: {:#?}",
            tab.get_url(),
            fel(tab)
        );
        std::thread::sleep(Duration::from_millis(100));
    }
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

/// Lägger till en virtuell autentiserare med användarverifiering.
fn autentiserare(tab: &Tab) {
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
    .unwrap();
}

struct Server(Child);

impl Server {
    /// Startar `maya serve` på `port`; den publika URL:en måste ha samma port.
    fn starta(dir: &Path, port: u16) -> Self {
        std::fs::write(
            dir.join(".env"),
            format!(
                "MAYA_PUBLIC_URL=http://localhost:{port}\nMAYA_LISTEN=127.0.0.1:{port}\n\
                 MAYA_BACKUP_DIR={backup}\nMAYA_DATA_DIR={data}\n",
                backup = dir.display(),
                data = dir.join("data").display(),
            ),
        )
        .unwrap();
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
                if stream.read_to_string(&mut svar).is_ok() && svar.starts_with("HTTP/1.1 200") {
                    return server;
                }
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
