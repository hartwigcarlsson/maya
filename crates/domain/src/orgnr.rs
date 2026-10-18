//! Organisationsnummer: tio siffror, Luhn-kontroll och tredje siffran ≥ 2. Samma regel i
//! formuläret och i servern.

use std::fmt;

/// Ett kontrollerat organisationsnummer.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Organisationsnummer(String);

/// Varför ett organisationsnummer nekas. Koden visas vid fältet i formuläret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrgnrFel {
    Längd,
    TredjeSiffran,
    Kontrollsiffra,
}

impl OrgnrFel {
    pub fn kod(self) -> &'static str {
        match self {
            OrgnrFel::Längd => "orgnr_langd",
            OrgnrFel::TredjeSiffran => "orgnr_tredje_siffran",
            OrgnrFel::Kontrollsiffra => "orgnr_kontrollsiffra",
        }
    }

    pub fn från_kod(kod: &str) -> Option<Self> {
        [
            OrgnrFel::Längd,
            OrgnrFel::TredjeSiffran,
            OrgnrFel::Kontrollsiffra,
        ]
        .into_iter()
        .find(|f| f.kod() == kod)
    }
}

impl fmt::Display for OrgnrFel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            OrgnrFel::Längd => "Organisationsnumret ska ha tio siffror.",
            OrgnrFel::TredjeSiffran => {
                "Tredje siffran ska vara minst 2; det här ser ut som ett personnummer."
            }
            OrgnrFel::Kontrollsiffra => "Kontrollsiffran stämmer inte. Kontrollera numret.",
        })
    }
}

impl std::error::Error for OrgnrFel {}

impl Organisationsnummer {
    /// Numret ur det användaren skrev, med eller utan bindestreck och blanktecken.
    pub fn tolka(text: &str) -> Result<Self, OrgnrFel> {
        let siffror: String = text
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '-')
            .collect();
        if siffror.len() != 10 || !siffror.bytes().all(|b| b.is_ascii_digit()) {
            return Err(OrgnrFel::Längd);
        }
        if siffror.as_bytes()[2] < b'2' {
            return Err(OrgnrFel::TredjeSiffran);
        }
        if !crate::luhn::stämmer(&siffror) {
            return Err(OrgnrFel::Kontrollsiffra);
        }
        Ok(Organisationsnummer(siffror))
    }

    /// De tio siffrorna utan bindestreck.
    pub fn siffror(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Organisationsnummer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", &self.0[..6], &self.0[6..])
    }
}
