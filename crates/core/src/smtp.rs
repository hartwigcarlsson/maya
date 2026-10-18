//! En liten SMTP-klient för larmen till driften (`MAYA_SMTP_…`). Ett mejl per anslutning, i
//! klartext UTF-8 som base64. Port 465 ger TLS direkt; annars används STARTTLS när servern
//! erbjuder det. Ett lösenord skickas aldrig okrypterat, utom till en server på samma maskin.
//!
//! Blockerande: anropa från `spawn_blocking`.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

use jiff::Timestamp;
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};

use crate::config::Smtp;

const TIDSGRÄNS: Duration = Duration::from_secs(30);

/// Ett mejl att skicka.
pub(crate) struct Brev<'a> {
    pub från: &'a str,
    pub till: &'a str,
    pub ämne: &'a str,
    pub text: &'a str,
}

/// Skickar `brev` genom `smtp`. `namn` är värden Maya presenterar sig som (`EHLO`). Felet är
/// på svenska.
pub(crate) fn skicka(smtp: &Smtp, namn: &str, brev: &Brev, nu: Timestamp) -> Result<(), String> {
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

/// Meddelandet enligt RFC 5322, med brödtexten som base64 så att inga rader behöver
/// dot-stuffing och åtta bitar aldrig når en server som inte tar emot dem.
fn meddelande(brev: &Brev, namn: &str, nu: Timestamp) -> String {
    let mut id = [0u8; 12];
    getrandom::fill(&mut id).expect("slumptal från operativsystemet");
    let kropp = base64(brev.text.replace('\n', "\r\n").as_bytes());
    let mut rader: Vec<String> = vec![
        format!("From: {}", brev.från),
        format!("To: {}", brev.till),
        format!("Subject: {}", ämne(brev.ämne)),
        format!("Date: {}", nu.strftime("%a, %d %b %Y %H:%M:%S +0000")),
        format!("Message-ID: <{}@{namn}>", hex::encode(id)),
        "MIME-Version: 1.0".into(),
        "Content-Type: text/plain; charset=utf-8".into(),
        "Content-Transfer-Encoding: base64".into(),
        String::new(),
    ];
    rader.extend(
        kropp
            .as_bytes()
            .chunks(76)
            .map(|r| String::from_utf8_lossy(r).into_owned()),
    );
    rader.join("\r\n") + "\r\n"
}

/// Ämnesraden: ren ASCII som den är, annars som kodade ord (RFC 2047) på hela tecken.
fn ämne(text: &str) -> String {
    if text.is_ascii() {
        return text.to_string();
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
    use super::{base64, ämne};

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
