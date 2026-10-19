//! Betalsätt: bankgiro, plusgiro och IBAN med BIC. Samma regler för företagets egna betalsätt
//! på fakturan och för leverantörernas betaluppgifter, i formuläret och på servern.

use std::fmt;

use crate::luhn;

/// Varför ett betalsätt nekas. Koden visas vid fältet i formuläret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Betalsättsfel {
    Bankgiro,
    BankgiroKontrollsiffra,
    Plusgiro,
    PlusgiroKontrollsiffra,
    Iban,
    IbanKontrollsiffra,
    Bic,
}

impl Betalsättsfel {
    pub fn kod(self) -> &'static str {
        match self {
            Betalsättsfel::Bankgiro => "bankgiro_ogiltigt",
            Betalsättsfel::BankgiroKontrollsiffra => "bankgiro_kontrollsiffra",
            Betalsättsfel::Plusgiro => "plusgiro_ogiltigt",
            Betalsättsfel::PlusgiroKontrollsiffra => "plusgiro_kontrollsiffra",
            Betalsättsfel::Iban => "iban_ogiltigt",
            Betalsättsfel::IbanKontrollsiffra => "iban_kontrollsiffra",
            Betalsättsfel::Bic => "bic_ogiltig",
        }
    }
}

impl fmt::Display for Betalsättsfel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Betalsättsfel::Bankgiro => {
                "Bankgironumret har sju eller åtta siffror, t.ex. 5050-1055."
            }
            Betalsättsfel::Plusgiro => "Plusgironumret har två till åtta siffror, t.ex. 12345-5.",
            Betalsättsfel::BankgiroKontrollsiffra
            | Betalsättsfel::PlusgiroKontrollsiffra
            | Betalsättsfel::IbanKontrollsiffra => {
                "Kontrollsiffran stämmer inte. Kontrollera numret."
            }
            Betalsättsfel::Iban => {
                "IBAN-numret börjar med landskoden, t.ex. SE45 5000 0000 0583 9825 7466."
            }
            Betalsättsfel::Bic => "BIC har åtta eller elva tecken, t.ex. ESSESESS.",
        })
    }
}

impl std::error::Error for Betalsättsfel {}

/// Siffrorna i det användaren skrev, utan blanktecken och bindestreck, eller `None` om något
/// annat tecken finns.
fn bara_siffror(text: &str) -> Option<String> {
    let siffror: String = text
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect();
    siffror
        .bytes()
        .all(|b| b.is_ascii_digit())
        .then_some(siffror)
}

/// Ett bankgironummer: sju eller åtta siffror med Luhn-kontrollsiffra, skrivet `NNN-NNNN` eller
/// `NNNN-NNNN`.
pub fn tolka_bankgiro(text: &str) -> Result<String, Betalsättsfel> {
    let siffror = bara_siffror(text)
        .filter(|s| matches!(s.len(), 7 | 8))
        .ok_or(Betalsättsfel::Bankgiro)?;
    if !luhn::stämmer(&siffror) {
        return Err(Betalsättsfel::BankgiroKontrollsiffra);
    }
    let delning = siffror.len() - 4;
    Ok(format!("{}-{}", &siffror[..delning], &siffror[delning..]))
}

/// Ett plusgironummer: två till åtta siffror med Luhn-kontrollsiffra, skrivet med
/// kontrollsiffran efter ett bindestreck.
pub fn tolka_plusgiro(text: &str) -> Result<String, Betalsättsfel> {
    let siffror = bara_siffror(text)
        .filter(|s| (2..=8).contains(&s.len()))
        .ok_or(Betalsättsfel::Plusgiro)?;
    if !luhn::stämmer(&siffror) {
        return Err(Betalsättsfel::PlusgiroKontrollsiffra);
    }
    let delning = siffror.len() - 1;
    Ok(format!("{}-{}", &siffror[..delning], &siffror[delning..]))
}

/// Ett IBAN-nummer: landskod, två kontrollsiffror och kontonumret, 15 till 34 tecken (24 för
/// Sverige), prövat med modulo 97 (ISO 13616). Det skrivs med stora bokstäver i grupper om
/// fyra.
pub fn tolka_iban(text: &str) -> Result<String, Betalsättsfel> {
    let tecken: String = text
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_uppercase();
    let b = tecken.as_bytes();
    let form = (15..=34).contains(&b.len())
        && b[..2].iter().all(u8::is_ascii_uppercase)
        && b[2..4].iter().all(u8::is_ascii_digit)
        && b[4..].iter().all(u8::is_ascii_alphanumeric)
        && (&b[..2] != b"SE" || b.len() == 24);
    if !form {
        return Err(Betalsättsfel::Iban);
    }
    let rest = b[4..].iter().chain(&b[..4]).fold(0u32, |rest, &c| {
        let värde = if c.is_ascii_digit() {
            u32::from(c - b'0')
        } else {
            u32::from(c - b'A') + 10
        };
        if värde < 10 {
            (rest * 10 + värde) % 97
        } else {
            (rest * 100 + värde) % 97
        }
    });
    if rest != 1 {
        return Err(Betalsättsfel::IbanKontrollsiffra);
    }
    Ok(b.chunks(4)
        .map(|grupp| std::str::from_utf8(grupp).unwrap_or_default())
        .collect::<Vec<_>>()
        .join(" "))
}

/// En BIC (ISO 9362): fyra bokstäver för banken, två för landet, två tecken för orten och
/// eventuellt tre för kontoret. Den skrivs med stora bokstäver utan mellanrum.
pub fn tolka_bic(text: &str) -> Result<String, Betalsättsfel> {
    let bic: String = text
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_uppercase();
    let b = bic.as_bytes();
    let giltig = matches!(b.len(), 8 | 11)
        && b[..6].iter().all(u8::is_ascii_uppercase)
        && b[6..].iter().all(u8::is_ascii_alphanumeric);
    if giltig {
        Ok(bic)
    } else {
        Err(Betalsättsfel::Bic)
    }
}
