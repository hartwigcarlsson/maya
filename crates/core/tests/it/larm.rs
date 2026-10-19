//! Larm till driften via SMTP (beslut 13 i #70), genom `core` mot en tillfällig databas, en fast
//! klocka och en SMTP-server i processen: högst ett per dygn och fel, och ett när felet är löst.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use maya_core::aktor::Aktör;
use maya_core::backup::{self, Backupmål};
use maya_core::config::Config;
use maya_core::drift::{self, DriftFel};
use maya_core::{Databas, Klocka, testhjalp};
use maya_domain::drift::{Larm, Larmfel};

fn klocka(tid: &str) -> Klocka {
    Klocka::fast(tid.parse().unwrap())
}

/// Ett mejl som SMTP-servern tog emot.
#[derive(Debug, Clone)]
struct Mejl {
    från: String,
    till: Vec<String>,
    data: String,
}

impl Mejl {
    fn ämne(&self) -> String {
        let rad = self
            .data
            .lines()
            .find_map(|r| r.strip_prefix("Subject: "))
            .expect("ämnesraden");
        avkoda_ämne(rad)
    }

    /// Brödtexten, som Maya skickar base64-kodad.
    fn text(&self) -> String {
        let (_, kropp) = self.data.split_once("\r\n\r\n").expect("huvud och kropp");
        let base64: String = kropp.split_whitespace().collect();
        String::from_utf8(avkoda_base64(&base64)).unwrap()
    }
}

/// `=?UTF-8?B?…?=` (orden efter varandra), eller ren text.
fn avkoda_ämne(rad: &str) -> String {
    if !rad.starts_with("=?") {
        return rad.to_string();
    }
    rad.split_whitespace()
        .map(|ord| {
            match ord
                .strip_prefix("=?UTF-8?B?")
                .and_then(|o| o.strip_suffix("?="))
            {
                Some(b) => String::from_utf8(avkoda_base64(b)).unwrap(),
                None => ord.to_string(),
            }
        })
        .collect()
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

/// En SMTP-server på 127.0.0.1 som tar emot allt och sparar mejlen. Bara klartext, utan
/// STARTTLS och utan inloggning.
struct FalskSmtp {
    port: u16,
    mejl: Arc<Mutex<Vec<Mejl>>>,
}

impl FalskSmtp {
    fn starta() -> Self {
        Self::som_avvisar(0)
    }

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
            ("MAYA_DRIFT_EPOST", "drift@example.se"),
        ])
    }
}

fn samtal(ström: TcpStream, lådan: &Mutex<Vec<Mejl>>, avvisa: &AtomicUsize) {
    let mut ut = ström.try_clone().unwrap();
    let mut in_ = BufReader::new(ström);
    let mut svara = |text: &str| ut.write_all(format!("{text}\r\n").as_bytes()).unwrap();
    svara("220 falsk.example.se ESMTP");
    let (mut från, mut till) = (String::new(), Vec::new());
    let mut rad = String::new();
    loop {
        rad.clear();
        if in_.read_line(&mut rad).unwrap_or(0) == 0 {
            return;
        }
        let kommando = rad.trim_end();
        let versaler = kommando.to_ascii_uppercase();
        if versaler.starts_with("EHLO") || versaler.starts_with("HELO") {
            svara("250-falsk.example.se");
            svara("250 8BITMIME");
        } else if let Some(a) = versaler.strip_prefix("MAIL FROM:") {
            från = kommando[kommando.len() - a.len()..].to_string();
            svara("250 OK");
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
                svara("554 Avvisat");
                continue;
            }
            lådan.lock().unwrap().push(Mejl {
                från: std::mem::take(&mut från),
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
        [
            ("MAYA_PUBLIC_URL", "https://bokforing.example.se"),
            ("MAYA_BACKUP_DIR", "/backup"),
        ]
        .iter()
        .chain(extra)
        .map(|(k, v)| (k.to_string(), v.to_string())),
    )
    .unwrap()
}

async fn larm(db: &Databas, tid: &str, config: &Config) -> Vec<Larm> {
    drift::skicka_larm(db, &klocka(tid), config, &Aktör::maya())
        .await
        .unwrap()
}

#[tokio::test]
async fn larmet_skickas_högst_en_gång_per_dygn_och_fel_och_en_gång_när_felet_är_löst() {
    let db = Databas::tillfällig().await;
    // Användaren skapades 2026-01-01; ingen backup har tagits.
    testhjalp::företag(&db).await;
    let smtp = FalskSmtp::starta();
    let config = smtp.config();

    let gammal = Larm::Fel(Larmfel::BackupGammal);
    assert_eq!(
        larm(&db, "2026-10-18T08:00:00Z", &config).await,
        vec![gammal]
    );
    let mejl = smtp.mejl();
    assert_eq!(mejl.len(), 1);
    assert_eq!(mejl[0].från, "<maya@example.se>");
    assert_eq!(mejl[0].till, vec!["<drift@example.se>"]);
    assert!(mejl[0].data.contains("To: drift@example.se"), "{mejl:?}");
    assert_eq!(mejl[0].ämne(), "Maya: backupen är äldre än 36 timmar");
    let text = mejl[0].text();
    assert!(text.contains("Ingen backup har lyckats än."), "{text}");
    assert!(text.contains("https://bokforing.example.se/"), "{text}");

    // Samma fel inom ett dygn: inget nytt mejl.
    assert_eq!(larm(&db, "2026-10-18T09:00:00Z", &config).await, vec![]);
    assert_eq!(larm(&db, "2026-10-19T07:59:59Z", &config).await, vec![]);
    assert_eq!(smtp.mejl().len(), 1);

    // Ett dygn senare: ett nytt.
    assert_eq!(
        larm(&db, "2026-10-19T08:00:00Z", &config).await,
        vec![gammal]
    );
    assert_eq!(smtp.mejl().len(), 2);

    // En lyckad backup löser felet: ett mejl om det, och sedan inget mer.
    testhjalp::backup(&db, &klocka("2026-10-19T09:00:00Z")).await;
    assert_eq!(
        larm(&db, "2026-10-19T09:05:00Z", &config).await,
        vec![Larm::Löst(Larmfel::BackupGammal)]
    );
    let mejl = smtp.mejl();
    assert_eq!(mejl.len(), 3);
    assert_eq!(mejl[2].ämne(), "Maya: löst: backupen är äldre än 36 timmar");
    assert_eq!(larm(&db, "2026-10-20T09:05:00Z", &config).await, vec![]);
    assert_eq!(smtp.mejl().len(), 3);
}

#[tokio::test]
async fn en_misslyckad_backup_ger_ett_larm_med_felet() {
    let db = Databas::tillfällig().await;
    testhjalp::företag(&db).await;
    testhjalp::backup(&db, &klocka("2026-10-18T01:00:00Z")).await;
    let smtp = FalskSmtp::starta();
    let config = smtp.config();
    let arbete = tempfile::tempdir().unwrap();
    let saknas = Backupmål::katalog(&arbete.path().join("finns-inte"));
    backup::ta_backup(
        &db,
        &klocka("2026-10-19T01:00:00Z"),
        &Aktör::maya(),
        &saknas,
        arbete.path(),
    )
    .await
    .unwrap_err();

    assert_eq!(
        larm(&db, "2026-10-19T01:01:00Z", &config).await,
        vec![Larm::Fel(Larmfel::BackupMisslyckades)]
    );
    let mejl = smtp.mejl();
    assert_eq!(mejl[0].ämne(), "Maya: backupen misslyckades");
    assert!(mejl[0].text().contains("finns-inte"), "{}", mejl[0].text());
}

#[tokio::test]
async fn utan_smtp_skickas_inga_larm() {
    let db = Databas::tillfällig().await;
    testhjalp::företag(&db).await;
    let utan_smtp = konfiguration(&[("MAYA_DRIFT_EPOST", "drift@example.se")]);
    assert_eq!(larm(&db, "2026-10-18T08:00:00Z", &utan_smtp).await, vec![]);

    // Utan adress skickas inget heller, och felet larmas när allt är på plats.
    let smtp = FalskSmtp::starta();
    let utan_adress = konfiguration(&[
        ("MAYA_SMTP_HOST", "127.0.0.1"),
        ("MAYA_SMTP_PORT", &smtp.port.to_string()),
    ]);
    assert_eq!(
        larm(&db, "2026-10-18T08:00:00Z", &utan_adress).await,
        vec![]
    );
    assert!(smtp.mejl().is_empty());
    assert_eq!(
        larm(&db, "2026-10-18T08:10:00Z", &smtp.config()).await,
        vec![Larm::Fel(Larmfel::BackupGammal)]
    );
}

#[tokio::test]
async fn ett_larm_som_inte_gick_att_skicka_skickas_nästa_gång() {
    let db = Databas::tillfällig().await;
    testhjalp::företag(&db).await;
    let stängd = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = stängd.local_addr().unwrap().port().to_string();
    drop(stängd);
    let ingen_server = konfiguration(&[
        ("MAYA_SMTP_HOST", "127.0.0.1"),
        ("MAYA_SMTP_PORT", &port),
        ("MAYA_DRIFT_EPOST", "drift@example.se"),
    ]);
    let fel = drift::skicka_larm(
        &db,
        &klocka("2026-10-18T08:00:00Z"),
        &ingen_server,
        &Aktör::maya(),
    )
    .await;
    assert!(matches!(fel, Err(DriftFel::Smtp(_))), "{fel:?}");

    let smtp = FalskSmtp::starta();
    assert_eq!(
        larm(&db, "2026-10-18T08:10:00Z", &smtp.config()).await,
        vec![Larm::Fel(Larmfel::BackupGammal)]
    );
}

#[tokio::test]
async fn ett_larm_som_inte_går_att_skicka_stoppar_inte_de_andra() {
    let db = Databas::tillfällig().await;
    testhjalp::företag(&db).await;
    let arbete = tempfile::tempdir().unwrap();
    let saknas = Backupmål::katalog(&arbete.path().join("finns-inte"));
    backup::ta_backup(
        &db,
        &klocka("2026-10-19T01:00:00Z"),
        &Aktör::maya(),
        &saknas,
        arbete.path(),
    )
    .await
    .unwrap_err();
    // Två fel: backupen misslyckades och ingen backup har lyckats. Det första mejlet avvisas.
    let smtp = FalskSmtp::som_avvisar(1);
    let config = smtp.config();

    let fel = drift::skicka_larm(
        &db,
        &klocka("2026-10-19T01:01:00Z"),
        &config,
        &Aktör::maya(),
    )
    .await;

    assert!(matches!(fel, Err(DriftFel::Smtp(_))), "{fel:?}");
    assert_eq!(smtp.mejl().len(), 1, "det andra larmet skickades ändå");
    // Nästa gång skickas bara det som inte kom fram.
    assert_eq!(larm(&db, "2026-10-19T01:11:00Z", &config).await.len(), 1);
    assert_eq!(smtp.mejl().len(), 2);
    assert_ne!(smtp.mejl()[0].ämne(), smtp.mejl()[1].ämne());
}

#[tokio::test]
async fn bara_maya_självt_skickar_larm() {
    let db = Databas::tillfällig().await;
    let smtp = FalskSmtp::starta();
    for aktör in [
        Aktör::cli(),
        testhjalp::användare(&db, "anna@example.se").await,
    ] {
        let fel =
            drift::skicka_larm(&db, &klocka("2026-10-18T08:00:00Z"), &smtp.config(), &aktör).await;
        assert!(matches!(fel, Err(DriftFel::KanalenFårInte)), "{fel:?}");
    }
}
