//! Regler som alla fakturor delar: momssatserna, momsen per sats, antal och rabatt på en rad,
//! radens netto och OCR-referensen. Kundfakturor, kreditfakturor och leverantörsfakturor
//! använder samma regler, i formuläret och på servern.
//!
//! Belopp är heltal i öre och har tecken (`i64`), så att en kreditfaktura kan ha negativa
//! rader. Avrundningen till öre är halva bort från noll, alltså symmetrisk kring noll.

use std::fmt;

use crate::kontoplan::Kontonummer;

/// En momssats som en fakturarad kan ha.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Momssats {
    Tjugofem,
    Tolv,
    Sex,
    Noll,
}

impl Momssats {
    /// Satserna i den ordning formuläret och fakturan visar dem.
    pub const ALLA: [Momssats; 4] = [
        Momssats::Tjugofem,
        Momssats::Tolv,
        Momssats::Sex,
        Momssats::Noll,
    ];

    pub fn procent(self) -> u8 {
        match self {
            Momssats::Tjugofem => 25,
            Momssats::Tolv => 12,
            Momssats::Sex => 6,
            Momssats::Noll => 0,
        }
    }

    pub fn från_procent(procent: u8) -> Option<Self> {
        Self::ALLA.into_iter().find(|s| s.procent() == procent)
    }

    /// `25`, `25 %` eller `25%`.
    pub fn tolka(text: &str) -> Option<Self> {
        let text = text.trim().trim_end_matches('%').trim();
        text.parse::<u8>().ok().and_then(Self::från_procent)
    }

    /// Kontot för utgående moms med satsen, eller `None` för 0 %.
    pub fn utgående_momskonto(self) -> Option<Kontonummer> {
        match self {
            Momssats::Tjugofem => Some(Kontonummer::konstant(2611)),
            Momssats::Tolv => Some(Kontonummer::konstant(2621)),
            Momssats::Sex => Some(Kontonummer::konstant(2631)),
            Momssats::Noll => None,
        }
    }
}

impl fmt::Display for Momssats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} %", self.procent())
    }
}

/// `täljare / nämnare` avrundat till närmaste heltal, halva bort från noll.
fn avrunda(täljare: i128, nämnare: i128) -> i128 {
    let (kvot, rest) = (täljare / nämnare, täljare % nämnare);
    if rest.abs() * 2 >= nämnare {
        kvot + täljare.signum()
    } else {
        kvot
    }
}

fn till_öre(värde: i128) -> i64 {
    i64::try_from(värde).unwrap_or(if värde < 0 { i64::MIN } else { i64::MAX })
}

/// Momsen på `netto` öre med satsen, avrundad till öre.
pub fn moms(netto: i64, sats: Momssats) -> i64 {
    till_öre(avrunda(i128::from(netto) * i128::from(sats.procent()), 100))
}

/// Ett tal med högst två decimaler, lagrat i hundradelar. Gemensamt för antal och rabatt.
fn hundradelar(text: &str) -> Option<i64> {
    let text: String = text
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '\u{a0}' && *c != '\u{202f}')
        .collect();
    let (tecken, text) = match text.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, text.as_str()),
    };
    let (heltal, decimaler) = match text.split_once([',', '.']) {
        Some((h, d)) => (h, d),
        None => (text, ""),
    };
    if heltal.is_empty()
        || !heltal.bytes().all(|b| b.is_ascii_digit())
        || decimaler.len() > 2
        || !decimaler.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let heltal: i64 = heltal.parse().ok()?;
    let decimaler: i64 = format!("{decimaler:0<2}").parse().ok()?;
    heltal
        .checked_mul(100)?
        .checked_add(decimaler)
        .map(|v| v * tecken)
}

/// Ett tal i hundradelar skrivet med decimalkomma och utan överflödiga nollor.
fn skriv_hundradelar(f: &mut fmt::Formatter<'_>, värde: i64) -> fmt::Result {
    let tecken = if värde < 0 { "-" } else { "" };
    let (heltal, rest) = (värde.unsigned_abs() / 100, värde.unsigned_abs() % 100);
    match rest {
        0 => write!(f, "{tecken}{heltal}"),
        r if r % 10 == 0 => write!(f, "{tecken}{heltal},{}", r / 10),
        r => write!(f, "{tecken}{heltal},{r:02}"),
    }
}

/// Antalet på en fakturarad, med högst två decimaler (t.ex. 1,5 timmar).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Antal(i64);

impl Antal {
    pub fn från_hundradelar(hundradelar: i64) -> Self {
        Antal(hundradelar)
    }

    pub fn hundradelar(self) -> i64 {
        self.0
    }

    /// `3`, `1,5` eller `0.25`, med blanksteg som tusentalsavgränsare och ett minustecken
    /// först för en kreditfaktura. Tom text är inget antal.
    pub fn tolka(text: &str) -> Option<Self> {
        hundradelar(text).map(Antal)
    }
}

impl fmt::Display for Antal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        skriv_hundradelar(f, self.0)
    }
}

/// Rabatten på en fakturarad i procent, 0–100 med högst två decimaler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rabatt(i64);

impl Rabatt {
    pub const INGEN: Rabatt = Rabatt(0);

    /// Rabatten i hundradelar av en procent, 0–10 000.
    pub fn från_hundradelar(hundradelar: i64) -> Option<Self> {
        (0..=10_000)
            .contains(&hundradelar)
            .then_some(Rabatt(hundradelar))
    }

    pub fn hundradelar(self) -> i64 {
        self.0
    }

    /// `10`, `12,5` eller `12,5 %`. Tom text är ingen rabatt.
    pub fn tolka(text: &str) -> Option<Self> {
        let text = text.trim().trim_end_matches('%').trim();
        if text.is_empty() {
            return Some(Rabatt::INGEN);
        }
        hundradelar(text).and_then(Self::från_hundradelar)
    }
}

impl fmt::Display for Rabatt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        skriv_hundradelar(f, self.0)
    }
}

/// Radens netto i öre: antal gånger à-pris (exklusive moms) minus rabatten, avrundat till öre.
pub fn radnetto(antal: Antal, à_pris: i64, rabatt: Rabatt) -> i64 {
    let täljare = i128::from(antal.0) * i128::from(à_pris) * i128::from(10_000 - rabatt.0);
    till_öre(avrunda(täljare, 100 * 10_000))
}

/// Beskattningsunderlaget och momsen för en momssats på en faktura.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Momsrad {
    pub sats: Momssats,
    /// Summan av radernas netto med satsen, i öre.
    pub underlag: i64,
    /// Momsen på underlaget, avrundad till öre.
    pub moms: i64,
}

/// Momsen per sats, räknad på summan av radernas netto med satsen (inte per rad), i
/// [`Momssats::ALLA`]s ordning. Bara satser som någon rad har kommer med.
pub fn momsrader(rader: impl IntoIterator<Item = (Momssats, i64)>) -> Vec<Momsrad> {
    let mut underlag = [None::<i64>; 4];
    for (sats, netto) in rader {
        let i = Momssats::ALLA
            .iter()
            .position(|s| *s == sats)
            .expect("en sats");
        underlag[i] = Some(underlag[i].unwrap_or(0).saturating_add(netto));
    }
    Momssats::ALLA
        .into_iter()
        .zip(underlag)
        .filter_map(|(sats, u)| {
            u.map(|underlag| Momsrad {
                sats,
                underlag,
                moms: moms(underlag, sats),
            })
        })
        .collect()
}

/// OCR-referensen för fakturanumret: numret, en längdsiffra (antalet siffror i referensen,
/// längd- och kontrollsiffran medräknade, modulo 10) och en Luhn-kontrollsiffra över resten.
pub fn ocr(fakturanummer: u64) -> String {
    let nummer = fakturanummer.to_string();
    let längd = (nummer.len() + 2) % 10;
    let utan_kontroll = format!("{nummer}{längd}");
    let kontroll = crate::luhn::kontrollsiffra(&utan_kontroll).expect("bara siffror");
    format!("{utan_kontroll}{kontroll}")
}
