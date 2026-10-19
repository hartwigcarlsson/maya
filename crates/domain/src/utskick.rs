//! Utskick av kundfakturor (#138): mejlets ämnesrad, meddelandet som förvals och sätten att
//! lämna en faktura på annat sätt än med e-post. Att skicka är en loggad händelse och inget
//! tillstånd på fakturan.

use jiff::civil::Date;

use crate::belopp::Saldo;

/// Hur en faktura lämnades när den inte skickades med Maya.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lämningssätt {
    Post,
    Personligen,
    Portal,
}

impl Lämningssätt {
    pub const ALLA: &[Lämningssätt] = &[
        Lämningssätt::Post,
        Lämningssätt::Personligen,
        Lämningssätt::Portal,
    ];

    /// Koden som lagras och står i formulären.
    pub fn kod(self) -> &'static str {
        match self {
            Lämningssätt::Post => "post",
            Lämningssätt::Personligen => "personligen",
            Lämningssätt::Portal => "portal",
        }
    }

    pub fn från_kod(kod: &str) -> Option<Self> {
        Self::ALLA.iter().copied().find(|s| s.kod() == kod)
    }

    pub fn namn(self) -> &'static str {
        match self {
            Lämningssätt::Post => "Med post",
            Lämningssätt::Personligen => "Överlämnad personligen",
            Lämningssätt::Portal => "Uppladdad i kundens portal",
        }
    }
}

/// Fakturamejlets ämnesrad. Radbrytningar i säljarens namn blir mellanslag, så att ämnet
/// aldrig blir flera huvudrader.
pub fn ämne(nummer: u64, säljare: &str) -> String {
    format!("Faktura {nummer} från {}", en_rad(säljare))
}

fn en_rad(text: &str) -> String {
    text.split(|c: char| c.is_control())
        .filter(|d| !d.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

// #140 Kreditfakturor
/// Ämnesraden för en kreditfaktura.
pub fn kreditämne(nummer: u64, säljare: &str) -> String {
    format!("Kreditfaktura {nummer} från {}", en_rad(säljare))
}

/// Meddelandet som förvals för en kreditfaktura: numret, beloppet och originalet. Företagets
/// standardmeddelande gäller fakturor att betala och används inte.
pub fn kreditmeddelande(nummer: u64, total_öre: i64, original: u64, säljare: &str) -> String {
    format!(
        "Hej!\n\nHär kommer kreditfaktura {nummer} på {} kr, som krediterar faktura {original}. \
         Kreditfakturan är bifogad som PDF.\n\nVänliga hälsningar\n{säljare}",
        Saldo::från_öre(total_öre)
    )
}

/// Meddelandet som förvals i mejlet: företagets standardmeddelande, eller utan ett en kort text
/// med numret, beloppet och förfallodagen.
pub fn förvalt_meddelande(
    standardmeddelande: &str,
    nummer: u64,
    total_öre: i64,
    förfallodag: Date,
    säljare: &str,
) -> String {
    let standard = standardmeddelande.trim();
    if !standard.is_empty() {
        return standard.to_string();
    }
    format!(
        "Hej!\n\nHär kommer faktura {nummer} på {} kr, att betala senast {förfallodag}. \
         Fakturan är bifogad som PDF.\n\nVänliga hälsningar\n{säljare}",
        Saldo::från_öre(total_öre)
    )
}
