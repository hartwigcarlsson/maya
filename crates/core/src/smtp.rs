//! En liten SMTP-klient (`MAYA_SMTP_…`) för larmen till driften och fakturamejlen (#138). Ett
//! mejl per anslutning: texten som UTF-8 i base64, och med bilagor `multipart/mixed` med varje
//! bilaga i base64. Port 465 ger TLS direkt; annars används STARTTLS när servern erbjuder det.
//! Ett lösenord skickas aldrig okrypterat, utom till en server på samma maskin.
//!
//! [`skicka`] kör det blockerande samtalet i `spawn_blocking`.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

use jiff::Timestamp;
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};

use crate::config::Smtp;

const TIDSGRÄNS: Duration = Duration::from_secs(30);

/// Ett mejl att skicka, till en mottagare.
#[derive(Debug, Clone, Default)]
pub struct Brev {
    /// Avsändaradressen (`MAIL FROM` och `From:`).
    pub från: String,
    /// Ett namn att visa för avsändaren, t.ex. företagets.
    pub från_namn: Option<String>,
    pub till: String,
    /// `Reply-To:`, dit mottagaren svarar.
    pub svara_till: Option<String>,
    pub ämne: String,
    pub text: String,
    pub bilagor: Vec<Bilaga>,
}

/// En fil som skickas med mejlet.
#[derive(Debug, Clone)]
pub struct Bilaga {
    pub filnamn: String,
    pub mime: String,
    pub innehåll: Vec<u8>,
}

/// Skickar `brev` genom `smtp`, i `spawn_blocking`. `namn` är värden Maya presenterar sig som
/// (`EHLO`) och `nu` mejlets `Date:`. Felet är på svenska.
pub async fn skicka(smtp: &Smtp, namn: &str, brev: Brev, nu: Timestamp) -> Result<(), String> {
    let (smtp, namn) = (smtp.clone(), namn.to_string());
    tokio::task::spawn_blocking(move || skicka_blockerande(&smtp, &namn, &brev, nu))
        .await
        .map_err(|e| format!("SMTP: {e}"))?
}

fn skicka_blockerande(smtp: &Smtp, namn: &str, brev: &Brev, nu: Timestamp) -> Result<(), String> {
    let fel = |e: std::io::Error| format!("SMTP-servern {}:{}: {e}", smtp.host, smtp.port);
    let adress = (smtp.host.as_str(), smtp.port)
        .to_socket_addrs()
        .map_err(fel)?
        .next()
        .ok_or_else(|| format!("SMTP-servern {} har ingen adress", smtp.host))?;
    let tcp = TcpStream::connect_timeout(&adress, TIDSGRÄNS).map_err(fel)?;
    tcp.set_read_timeout(Some(TIDSGRÄNS)).map_err(fel)?;
    tcp.set_write_timeout(Some(TIDSGRÄNS)).map_err(fel)?;
    let lokal = adress.ip().is_loopback();

    let mut s = if smtp.port == 465 {
        Ström::Klar(tcp).till_tls(&smtp.host)?
    } else {
        Ström::Klar(tcp)
    };
    s.svar(220)?;
    let förmågor = s.kommando(&format!("EHLO {namn}"), 250)?;
    if !s.är_tls() && förmågor.iter().any(|f| f.eq_ignore_ascii_case("STARTTLS")) {
        s.kommando("STARTTLS", 220)?;
        s = s.till_tls(&smtp.host)?;
        s.kommando(&format!("EHLO {namn}"), 250)?;
    }
    if let (Some(användare), Some(lösenord)) = (&smtp.username, &smtp.password) {
        if !s.är_tls() && !lokal {
            return Err(format!(
                "SMTP-servern {} erbjuder inte STARTTLS, och Maya skickar inte lösenordet \
                 okrypterat. Använd port 465 eller en server med STARTTLS.",
                smtp.host
            ));
        }
        let plain = base64(format!("\0{användare}\0{}", lösenord.expose()).as_bytes());
        s.kommando(&format!("AUTH PLAIN {plain}"), 235)?;
    }
    s.kommando(&format!("MAIL FROM:<{}>", brev.från), 250)?;
    s.kommando(&format!("RCPT TO:<{}>", brev.till), 250)?;
    s.kommando("DATA", 354)?;
    s.skriv(&meddelande(brev, namn, nu))?;
    s.kommando(".", 250)?;
    // Mejlet är mottaget; ett fel vid QUIT spelar ingen roll.
    let _ = s.kommando("QUIT", 221);
    Ok(())
}

/// Meddelandet enligt RFC 5322, med brödtexten och bilagorna som base64 så att inga rader
/// behöver dot-stuffing och åtta bitar aldrig når en server som inte tar emot dem.
fn meddelande(brev: &Brev, namn: &str, nu: Timestamp) -> String {
    let från = match &brev.från_namn {
        Some(n) => format!("{} <{}>", visningsnamn(n), brev.från),
        None => brev.från.clone(),
    };
    let mut rader: Vec<String> = vec![format!("From: {från}"), format!("To: {}", brev.till)];
    if let Some(svar) = &brev.svara_till {
        rader.push(format!("Reply-To: {svar}"));
    }
    rader.extend([
        format!("Subject: {}", ämne(&brev.ämne)),
        format!("Date: {}", nu.strftime("%a, %d %b %Y %H:%M:%S +0000")),
        format!("Message-ID: <{}@{namn}>", slump()),
        "MIME-Version: 1.0".into(),
    ]);
    let text = |rader: &mut Vec<String>| {
        rader.push("Content-Type: text/plain; charset=utf-8".into());
        rader.push("Content-Transfer-Encoding: base64".into());
        rader.push(String::new());
        rader.extend(base64_rader(brev.text.replace('\n', "\r\n").as_bytes()));
    };
    if brev.bilagor.is_empty() {
        text(&mut rader);
    } else {
        // Gränsen innehåller `=_`, som aldrig förekommer i base64.
        let gräns = format!("=_maya_{}", slump());
        rader.push(format!(
            "Content-Type: multipart/mixed; boundary=\"{gräns}\""
        ));
        rader.push(String::new());
        rader.push(format!("--{gräns}"));
        text(&mut rader);
        for bilaga in &brev.bilagor {
            rader.push(format!("--{gräns}"));
            rader.push(format!(
                "Content-Type: {}; {}",
                bilaga.mime,
                parameter("name", &bilaga.filnamn)
            ));
            rader.push(format!(
                "Content-Disposition: attachment; {}",
                parameter("filename", &bilaga.filnamn)
            ));
            rader.push("Content-Transfer-Encoding: base64".into());
            rader.push(String::new());
            rader.extend(base64_rader(&bilaga.innehåll));
        }
        rader.push(format!("--{gräns}--"));
    }
    rader.join("\r\n") + "\r\n"
}

fn slump() -> String {
    let mut id = [0u8; 12];
    getrandom::fill(&mut id).expect("slumptal från operativsystemet");
    hex::encode(id)
}

/// Base64 i rader om 76 tecken.
fn base64_rader(data: &[u8]) -> Vec<String> {
    base64(data)
        .as_bytes()
        .chunks(76)
        .map(|r| String::from_utf8_lossy(r).into_owned())
        .collect()
}

/// En parameter i en MIME-huvudrad: citerad när värdet är ren ASCII, annars UTF-8 enligt
/// RFC 2231.
fn parameter(namn: &str, värde: &str) -> String {
    let enkel = värde
        .bytes()
        .all(|b| (b.is_ascii_graphic() && b != b'"' && b != b'\\') || b == b' ');
    if enkel {
        return format!("{namn}=\"{värde}\"");
    }
    let mut kodat = String::new();
    for b in värde.bytes() {
        if b.is_ascii_alphanumeric() || b"!#$&+-.^_`|~".contains(&b) {
            kodat.push(char::from(b));
        } else {
            kodat.push_str(&format!("%{b:02X}"));
        }
    }
    format!("{namn}*=UTF-8''{kodat}")
}

/// Ett visningsnamn i `From:`: citerat, eller som kodade ord när det inte är ASCII.
fn visningsnamn(namn: &str) -> String {
    let namn = utan_styrtecken(namn);
    if namn.is_ascii() {
        format!("\"{}\"", namn.replace(['"', '\\'], ""))
    } else {
        ämne(&namn)
    }
}

/// Styrtecken, som radbrytningar, blir mellanslag: en huvudrad blir aldrig två.
fn utan_styrtecken(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// Ämnesraden: ren ASCII som den är, annars som kodade ord (RFC 2047) på hela tecken.
fn ämne(text: &str) -> String {
    let text = utan_styrtecken(text);
    if text.is_ascii() {
        return text;
    }
    let mut ord = Vec::new();
    let mut del = String::new();
    for c in text.chars() {
        if del.len() + c.len_utf8() > 45 {
            ord.push(std::mem::take(&mut del));
        }
        del.push(c);
    }
    ord.push(del);
    ord.iter()
        .map(|d| format!("=?UTF-8?B?{}?=", base64(d.as_bytes())))
        .collect::<Vec<_>>()
        .join(" ")
}

fn base64(data: &[u8]) -> String {
    const ALFABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut ut = String::with_capacity(data.len().div_ceil(3) * 4);
    for bitar in data.chunks(3) {
        let b = [
            bitar[0],
            bitar.get(1).copied().unwrap_or(0),
            bitar.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= bitar.len() {
                ut.push(char::from(ALFABET[(n >> (18 - 6 * i)) as usize & 63]));
            } else {
                ut.push('=');
            }
        }
    }
    ut
}

enum Ström {
    Klar(TcpStream),
    Tls(Box<StreamOwned<ClientConnection, TcpStream>>),
}

impl Ström {
    fn är_tls(&self) -> bool {
        matches!(self, Ström::Tls(_))
    }

    fn till_tls(self, värd: &str) -> Result<Self, String> {
        let Ström::Klar(tcp) = self else {
            return Ok(self);
        };
        let rötter = RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        let config =
            ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .map_err(|e| format!("TLS: {e}"))?
                .with_root_certificates(rötter)
                .with_no_client_auth();
        let namn = ServerName::try_from(värd.to_string())
            .map_err(|_| format!("SMTP-servern {värd} är inget giltigt namn för TLS"))?;
        let conn = ClientConnection::new(Arc::new(config), namn)
            .map_err(|e| format!("TLS mot {värd}: {e}"))?;
        Ok(Ström::Tls(Box::new(StreamOwned::new(conn, tcp))))
    }

    fn skriv(&mut self, text: &str) -> Result<(), String> {
        self.write_all(text.as_bytes())
            .and_then(|()| self.flush())
            .map_err(|e| format!("SMTP: {e}"))
    }

    /// Skickar `rad` och väntar på svaret `kod`. Ger svarets rader utan kod.
    fn kommando(&mut self, rad: &str, kod: u16) -> Result<Vec<String>, String> {
        self.skriv(&format!("{rad}\r\n"))?;
        self.svar(kod).map_err(|e| {
            let vad = rad.split(' ').next().unwrap_or(rad);
            format!("{e} (efter {vad})")
        })
    }

    /// Läser ett svar, också ett på flera rader (`250-…`), och kräver koden `kod`.
    fn svar(&mut self, kod: u16) -> Result<Vec<String>, String> {
        let mut rader = Vec::new();
        loop {
            let rad = self.läs_rad()?;
            let fått: u16 = rad
                .get(..3)
                .and_then(|k| k.parse().ok())
                .ok_or_else(|| format!("SMTP-servern svarade oväntat: {rad}"))?;
            let fortsätter = rad.as_bytes().get(3) == Some(&b'-');
            rader.push(rad.get(4..).unwrap_or_default().to_string());
            if !fortsätter {
                if fått != kod {
                    return Err(format!("SMTP-servern svarade {rad}"));
                }
                return Ok(rader);
            }
        }
    }

    fn läs_rad(&mut self) -> Result<String, String> {
        let mut rad = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            match self.read(&mut byte) {
                Ok(0) => return Err("SMTP-servern stängde anslutningen".into()),
                Ok(_) => {
                    if byte[0] == b'\n' {
                        if rad.last() == Some(&b'\r') {
                            rad.pop();
                        }
                        return Ok(String::from_utf8_lossy(&rad).into_owned());
                    }
                    rad.push(byte[0]);
                    if rad.len() > 4096 {
                        return Err("SMTP-serverns svar är för långt".into());
                    }
                }
                Err(e) => return Err(format!("SMTP: {e}")),
            }
        }
    }
}

impl Read for Ström {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Ström::Klar(s) => s.read(buf),
            Ström::Tls(s) => s.read(buf),
        }
    }
}

impl Write for Ström {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Ström::Klar(s) => s.write(buf),
            Ström::Tls(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Ström::Klar(s) => s.flush(),
            Ström::Tls(s) => s.flush(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Bilaga, Brev, base64, meddelande, parameter, ämne};

    #[test]
    fn ett_filnamn_med_å_ä_ö_kodas_enligt_rfc_2231() {
        assert_eq!(
            parameter("filename", "Kundfaktura_1.pdf"),
            "filename=\"Kundfaktura_1.pdf\""
        );
        assert_eq!(
            parameter("filename", "Räkning 1.pdf"),
            "filename*=UTF-8''R%C3%A4kning%201.pdf"
        );
    }

    #[test]
    fn ett_mejl_med_bilaga_är_multipart_med_svarsadress() {
        let brev = Brev {
            från: "maya@example.se".into(),
            från_namn: Some("Exempel AB".into()),
            till: "kund@example.se".into(),
            svara_till: Some("faktura@exempel.se".into()),
            ämne: "Faktura 1".into(),
            text: "Hej".into(),
            bilagor: vec![Bilaga {
                filnamn: "Kundfaktura_1.pdf".into(),
                mime: "application/pdf".into(),
                innehåll: b"%PDF".to_vec(),
            }],
        };
        let m = meddelande(
            &brev,
            "maya.example.se",
            "2026-03-01T09:00:00Z".parse().unwrap(),
        );
        assert!(
            m.contains("From: \"Exempel AB\" <maya@example.se>\r\n"),
            "{m}"
        );
        assert!(m.contains("Reply-To: faktura@exempel.se\r\n"), "{m}");
        assert!(
            m.contains("Content-Type: multipart/mixed; boundary=\"=_maya_"),
            "{m}"
        );
        assert!(m.contains("Content-Disposition: attachment; filename=\"Kundfaktura_1.pdf\""));
        assert!(m.contains(&format!("\r\n{}\r\n", base64(b"%PDF"))), "{m}");
        assert!(m.lines().all(|r| r.len() <= 998 && r != "."), "{m}");
    }

    #[test]
    fn base64_följer_rfc_4648() {
        // Testvektorerna i RFC 4648 avsnitt 10.
        for (in_, ut) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(in_.as_bytes()), ut);
        }
    }

    #[test]
    fn ämnesraden_kodas_bara_när_den_inte_är_ascii() {
        assert_eq!(ämne("Maya: backup"), "Maya: backup");
        // "å" är två byte i UTF-8.
        assert_eq!(ämne("å"), "=?UTF-8?B?w6U=?=");
        let lång = ämne("Maya: löst: verifieringen av backupen misslyckades");
        assert!(
            lång
                .split(' ')
                .all(|o| o.len() <= 75 && o.starts_with("=?UTF-8?B?")),
            "{lång}"
        );
    }
}
