//! BAS 2026, tolkad vid bygget ur BAS fria XLSX (ADR 0006). Grundkontona är ■-markeringen i
//! BAS 2025, den senaste fria utgåvan som har markeringen, mappad mot 2026 års konton.

use super::Kontonummer;

/// BAS-versionen som Maya har inbyggd.
pub const VERSION: &str = "BAS 2026";

/// Ett konto i den inbyggda BAS-kontoplanen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BasPost {
    nummer: Kontonummer,
    namn: &'static str,
    grundkonto: bool,
}

impl BasPost {
    pub fn nummer(&self) -> Kontonummer {
        self.nummer
    }

    /// Kontots namn i BAS.
    pub fn namn(&self) -> &'static str {
        self.namn
    }

    /// Om kontot är ett grundkonto (■), aktivt när ett nytt företag läggs upp.
    pub fn grundkonto(&self) -> bool {
        self.grundkonto
    }
}

include!(concat!(env!("OUT_DIR"), "/bas.rs"));

/// Alla konton i BAS 2026, sorterade på nummer.
pub fn konton() -> &'static [BasPost] {
    KONTON
}

/// Kontot med numret, om det finns i BAS 2026.
pub fn konto(nummer: Kontonummer) -> Option<&'static BasPost> {
    KONTON
        .binary_search_by_key(&nummer, |k| k.nummer)
        .ok()
        .map(|i| &KONTON[i])
}

/// Namnet på kontogruppen (numrets två första siffror), om BAS 2026 har en rubrik för den.
pub fn kontogrupp(grupp: u8) -> Option<&'static str> {
    KONTOGRUPPER
        .binary_search_by_key(&grupp, |&(g, _)| g)
        .ok()
        .map(|i| KONTOGRUPPER[i].1)
}
