//! Sökningen i ⌘K och sökfältet: vad en fråga träffar. Servern prövar verifikaten, fakturorna,
//! kunderna och leverantörerna och webbläsaren sidorna och handlingarna med samma regler.

use jiff::civil::Date;

use crate::belopp::Belopp;

/// En fråga som användaren skrev, tolkad på alla sätt den kan läsas: som ord, som ett
/// verifikationsnummer (`V12` eller `12`), som ett belopp (`1 250,50`) och som ett datum eller
/// en månad (`2026-03-14`, `2026-03`). Den träffar det som någon av läsningarna träffar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fråga {
    /// Hela frågan med gemener, för fakturanumret.
    text: String,
    ord: Vec<String>,
    nummer: Option<u32>,
    belopp: Option<Belopp>,
    datum: Option<String>,
}

/// Det i ett verifikat som en fråga prövas mot.
#[derive(Debug, Clone, Default)]
pub struct Sökbart<'a> {
    /// `None` för ett utkast.
    pub nummer: Option<u32>,
    pub datum: Option<Date>,
    /// Beskrivningen och radtexterna.
    pub texter: Vec<&'a str>,
    /// Summan och radernas belopp.
    pub belopp: Vec<Belopp>,
}

/// Det i en kundfaktura eller leverantörsfaktura som en fråga prövas mot (story 76).
#[derive(Debug, Clone, Default)]
pub struct Fakturasökbart<'a> {
    /// Numret som fakturan visar; tomt för ett utkast utan nummer.
    pub nummer: &'a str,
    /// Fakturadatum och förfallodag.
    pub datum: Vec<Date>,
    /// Kundens eller leverantörens namn.
    pub texter: Vec<&'a str>,
    /// Totalen och det som återstår, utan tecken: en kreditfaktura hittas på sitt belopp.
    pub belopp: Vec<Belopp>,
}

impl Fråga {
    /// `None` när frågan är tom.
    pub fn tolka(text: &str) -> Option<Fråga> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let ord = text.split_whitespace().map(str::to_lowercase).collect();
        let siffror = text.strip_prefix(['V', 'v']).unwrap_or(text);
        let nummer = (!siffror.is_empty() && siffror.bytes().all(|b| b.is_ascii_digit()))
            .then(|| siffror.parse().ok())
            .flatten();
        let datum = är_datum_eller_månad(text).then(|| text.to_string());
        Some(Fråga {
            text: text.to_lowercase(),
            ord,
            nummer,
            belopp: Belopp::tolka(text),
            datum,
        })
    }

    /// Om varje ord i frågan finns i texten, utan hänsyn till versaler.
    pub fn träffar_text(&self, text: &str) -> bool {
        let text = text.to_lowercase();
        self.ord.iter().all(|o| text.contains(o.as_str()))
    }

    pub fn träffar_verifikat(&self, v: &Sökbart<'_>) -> bool {
        let nummer = self.nummer.is_some() && self.nummer == v.nummer;
        let belopp = self.belopp.is_some_and(|b| v.belopp.contains(&b));
        let datum = match (&self.datum, v.datum) {
            (Some(fråga), Some(d)) => d.to_string().starts_with(fråga.as_str()),
            _ => false,
        };
        nummer || belopp || datum || self.träffar_text(&v.texter.join("\n"))
    }

    /// En faktura träffas av hela sitt nummer (utan hänsyn till versaler), ett av sina belopp,
    /// sitt fakturadatum eller sin förfallodag (dag eller månad), eller kundens eller
    /// leverantörens namn.
    pub fn träffar_faktura(&self, f: &Fakturasökbart<'_>) -> bool {
        let nummer = !f.nummer.is_empty() && f.nummer.to_lowercase() == self.text;
        let belopp = self.belopp.is_some_and(|b| f.belopp.contains(&b));
        let datum = self.datum.as_ref().is_some_and(|fråga| {
            f.datum
                .iter()
                .any(|d| d.to_string().starts_with(fråga.as_str()))
        });
        nummer || belopp || datum || self.träffar_text(&f.texter.join("\n"))
    }
}

/// `ÅÅÅÅ-MM` eller `ÅÅÅÅ-MM-DD`.
fn är_datum_eller_månad(text: &str) -> bool {
    let b = text.as_bytes();
    let siffror = |r: std::ops::Range<usize>| b[r].iter().all(u8::is_ascii_digit);
    match b.len() {
        7 => siffror(0..4) && b[4] == b'-' && siffror(5..7),
        10 => siffror(0..4) && b[4] == b'-' && siffror(5..7) && b[7] == b'-' && siffror(8..10),
        _ => false,
    }
}
