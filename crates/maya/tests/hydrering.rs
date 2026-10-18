//! Röktest för hydreringen: WASM:en tar över sidan utan panik, och en intern länk navigerar
//! sedan på klienten.
//!
//! Kräver Chrome eller Chromium (sökvägen kan anges i `CHROME`) och ett `cargo leptos build`,
//! så att `target/site/pkg/maya.wasm` finns. Utan WASM:en hoppar testet över sig självt,
//! utom i CI.

mod stod;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use headless_chrome::protocol::cdp::Page;
use headless_chrome::{Browser, LaunchOptions, Tab};

const MAYA: &str = env!("CARGO_BIN_EXE_maya");
const VÄNTA: Duration = Duration::from_secs(20);

/// Samlar fel från sidan (panik via `console_error_panic_hook`, oväntade undantag) i
/// `window.__fel`, innan sidans egna skript körs.
const SAMLA_FEL: &str = "window.__fel=[];\
const e=console.error.bind(console);\
console.error=(...a)=>{window.__fel.push(a.map(String).join(' '));e(...a)};\
addEventListener('error',h=>window.__fel.push(String(h.message)));\
addEventListener('unhandledrejection',h=>window.__fel.push(String(h.reason)));";

#[test]
fn sidan_hydreras_utan_panik_och_interna_lankar_navigerar() {
    if !stod::sajten_finns() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let server = Server::starta(dir.path());
    let bas = format!("http://localhost:{}", server.port);

    let browser = Browser::new(
        LaunchOptions::default_builder()
            .sandbox(false)
            .idle_browser_timeout(Duration::from_secs(120))
            .build()
            .unwrap(),
    )
    .expect("röktestet kräver Chrome eller Chromium; ange sökvägen i CHROME");
    let tab = browser.new_tab().unwrap();
    tab.call_method(Page::AddScriptToEvaluateOnNewDocument {
        source: SAMLA_FEL.to_string(),
        world_name: None,
        include_command_line_api: None,
        run_immediately: None,
    })
    .unwrap();

    // Registreringssidan (ur CLI-länken) har logotypen som intern länk till startsidan.
    let länk = skapa_användare(dir.path(), &bas);
    tab.navigate_to(&länk).unwrap();
    vänta_på_hydreringen(&tab);

    // Routern ska navigera på klienten: sidan laddas inte om, så markören finns kvar.
    tab.evaluate("window.__samma_sida = true", false).unwrap();
    tab.find_element("a.logotyp").unwrap().click().unwrap();
    vänta_på_text(&tab, "Fortsätt med passkey");
    assert_eq!(tab.get_url(), format!("{bas}/"));
    let samma_sida = tab
        .evaluate("window.__samma_sida === true", false)
        .unwrap()
        .value;
    assert_eq!(samma_sida, Some(true.into()), "länken laddade om sidan");
    assert_eq!(fel(&tab), Vec::<String>::new());
}

/// Väntar tills `hydrate()` har kört klart (den sätter `<html data-hydrerad>`). Bryter direkt
/// om sidan har loggat ett fel.
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

struct Server {
    process: Child,
    port: u16,
}

impl Server {
    /// Startar `maya serve` på en ledig port. Den publika URL:en måste ha samma port, så
    /// porten väljs här och servern får den via `.env`.
    fn starta(dir: &Path) -> Self {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
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
        let server = Server {
            process: Command::new(MAYA)
                .current_dir(dir)
                .env_clear()
                .arg("serve")
                .stdout(Stdio::null())
                .spawn()
                .unwrap(),
            port,
        };
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
        let _ = self.process.kill();
    }
}
