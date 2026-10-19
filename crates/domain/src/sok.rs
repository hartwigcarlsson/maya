//! Sökningen i ⌘K och sökfältet: vad en fråga träffar. Servern prövar verifikaten, fakturorna,
//! kunderna och leverantörerna och webbläsaren sidorna och handlingarna med samma regler.

use jiff::civil::Date;

use crate::belopp::Belopp;

/// En fråga som användaren skrev, tolkad på alla sätt den kan läsas: som ord, som ett
/// verifikationsnummer (`V12` eller `12`), som ett belopp (`1 250,50`) och som ett datum eller
/// en månad (`2026-03-14`, `2026-03`). Den träffar det som någon av läsningarna träffar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fråga {
    /// Fakturanumret som frågan söker, med gemener: hela frågan, eller det som står efter
    /// "faktura", "kundfaktura", "kreditfaktura" eller "leverantörsfaktura".
    fakturanummer: String,
    /// Vilka fakturor numret söks bland.
    fakturaslag: Fakturaslag,
    /// Frågan började med "faktura", "kundfaktura", "kreditfaktura" eller "leverantörsfaktura".
    nämner_faktura: bool,
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

/// Fakturorna som ett fakturanummer i frågan söks bland.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fakturaslag {
    Alla,
    Kund,
    Leverantör,
}

/// Orden som kan stå före ett fakturanummer i frågan, t.ex. "faktura 2".
const FAKTURAORD: [(&str, Fakturaslag); 4] = [
    ("faktura", Fakturaslag::Alla),
    ("kundfaktura", Fakturaslag::Kund),
    ("kreditfaktura", Fakturaslag::Kund),
    ("leverantörsfaktura", Fakturaslag::Leverantör),
];

/// Det i en kundfaktura eller leverantörsfaktura som en fråga prövas mot (story 76).
#[derive(Debug, Clone, Default)]
pub struct Fakturasökbart<'a> {
    /// Numret som fakturan visar; tomt för ett utkast utan nummer.
    pub nummer: &'a str,
    /// Fakturan är en leverantörsfaktura, inte en kundfaktura.
    pub leverantörsfaktura: bool,
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
        let gemener = text.to_lowercase();
        let (fakturaslag, fakturanummer, nämner_faktura) = gemener
            .split_once(char::is_whitespace)
            .and_then(|(först, resten)| {
                let resten = resten.trim();
                FAKTURAORD
                    .iter()
                    .find(|(ord, _)| *ord == först)
                    .filter(|_| !resten.is_empty() && !resten.contains(char::is_whitespace))
                    .map(|(_, slag)| (*slag, resten.to_string()))
            })
            .map(|(slag, nummer)| (slag, nummer, true))
            .unwrap_or((Fakturaslag::Alla, gemener.clone(), false));
        Some(Fråga {
            fakturanummer,
            fakturaslag,
            nämner_faktura,
            ord,
            nummer,
            belopp: Belopp::tolka(text),
            datum,
        })
    }

    /// Om frågan nämner en faktura med nummer, t.ex. "faktura 2": då visas fakturorna före
    /// verifikaten.
    pub fn nämner_faktura(&self) -> bool {
        self.nämner_faktura
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

    /// En faktura träffas av hela sitt nummer (utan hänsyn till versaler), också med ett tillägg
    /// efter ett skiljetecken (`8812` träffar `8812-2`), ett av sina belopp, sitt fakturadatum
    /// eller sin förfallodag (dag eller månad), eller kundens eller leverantörens namn. Numret
    /// kan stå efter "faktura", "kundfaktura", "kreditfaktura" eller "leverantörsfaktura".
    pub fn träffar_faktura(&self, f: &Fakturasökbart<'_>) -> bool {
        let rätt_slag = match self.fakturaslag {
            Fakturaslag::Alla => true,
            Fakturaslag::Kund => !f.leverantörsfaktura,
            Fakturaslag::Leverantör => f.leverantörsfaktura,
        };
        let nummer = rätt_slag && samma_nummer(&f.nummer.to_lowercase(), &self.fakturanummer);
        let belopp = self.belopp.is_some_and(|b| f.belopp.contains(&b));
        let datum = self.datum.as_ref().is_some_and(|fråga| {
            f.datum
                .iter()
                .any(|d| d.to_string().starts_with(fråga.as_str()))
        });
        nummer || belopp || datum || self.träffar_text(&f.texter.join("\n"))
    }
}

/// Om fakturanumret `nummer` är `fråga`, eller `fråga` följt av ett skiljetecken och ett
/// tillägg (`8812-2`, `8812/2`). Ett nummer som bara börjar likadant (`88121`) är ett annat.
fn samma_nummer(nummer: &str, fråga: &str) -> bool {
    if nummer.is_empty() || fråga.is_empty() {
        return false;
    }
    match nummer.strip_prefix(fråga) {
        Some("") => true,
        Some(tillägg) => {
            let mut tecken = tillägg.chars();
            tecken.next().is_some_and(|t| !t.is_alphanumeric())
                && fråga.chars().last().is_some_and(char::is_alphanumeric)
                && tecken.as_str().chars().any(char::is_alphanumeric)
        }
        None => false,
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
