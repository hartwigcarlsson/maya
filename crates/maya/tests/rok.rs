//! Röktest med riktig server och en webbläsare med virtuell autentiserare: första användaren
//! via CLI-länken, och inloggning med e-post och passkey.
//!
//! Kräver Chrome eller Chromium (sökvägen kan anges i `CHROME`). Sidan behöver ingen WASM:
//! inloggningen är vanlig HTML med JS-shimmen.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use headless_chrome::protocol::cdp::WebAuthn;
use headless_chrome::{Browser, LaunchOptions, Tab};

const MAYA: &str = env!("CARGO_BIN_EXE_maya");
const VÄNTA: Duration = Duration::from_secs(20);

#[test]
fn forsta_anvandaren_via_cli_lanken_och_inloggning_med_passkey() {
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
    tab.call_method(WebAuthn::Enable {
        enable_ui: Some(false),
    })
    .unwrap();
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
    vänta_på_text(&tab, "Du är inloggad som anna@example.se");

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

    // Logga ut och in igen: e-post först, sedan passkey.
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
    vänta_på_text(&tab, "Du är inloggad som anna@example.se");
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
