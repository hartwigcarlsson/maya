//! Kontoplanens regler: kontonummer, kontoklass och den inbyggda BAS-kontoplanen.

pub mod bas;
pub mod ink2;

/// Kontona som Mayas flöden bokför på, med flödets namn. De är aktiva från början och kan aldrig
/// inaktiveras ("Används av flödet X"). Varje etapp lägger till sina konton.
const FLÖDENAS_KONTON: &[(Kontonummer, &str)] =
    &[(Kontonummer(2099), "Preliminär ingående balans")];

/// Flödenas konton, med flödets namn per konto.
pub fn flödenas_konton() -> &'static [(Kontonummer, &'static str)] {
    FLÖDENAS_KONTON
}

/// Namnet på flödet som bokför på kontot, om något gör det.
pub fn flöde_som_bokför_på(nummer: Kontonummer) -> Option<&'static str> {
    FLÖDENAS_KONTON
        .iter()
        .find(|(n, _)| *n == nummer)
        .map(|(_, flöde)| *flöde)
}

/// Ett fyrsiffrigt kontonummer i BAS klass 1–8.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Kontonummer(u16);

impl Kontonummer {
    /// Kontonumret, om det är fyrsiffrigt och hör till klass 1–8. Klass 9 (internredovisning)
    /// för Maya inte.
    pub fn new(nummer: u16) -> Option<Self> {
        (1000..=8999).contains(&nummer).then_some(Self(nummer))
    }

    pub fn get(self) -> u16 {
        self.0
    }

    /// Kontoklassen, numrets första siffra (1–8).
    pub fn klass(self) -> u8 {
        (self.0 / 1000) as u8
    }

    /// Kontots typ, som kontoklassen avgör.
    pub fn typ(self) -> Kontotyp {
        match self.klass() {
            1 => Kontotyp::Tillgång,
            2 => Kontotyp::EgetKapitalOchSkuld,
            3 => Kontotyp::Intäkt,
            4..=7 => Kontotyp::Kostnad,
            _ => Kontotyp::FinansiellPost,
        }
    }
}

/// Vad ett konto är för slags konto, enligt BAS kontoklasser.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kontotyp {
    /// Klass 1.
    Tillgång,
    /// Klass 2.
    EgetKapitalOchSkuld,
    /// Klass 3, rörelsens intäkter.
    Intäkt,
    /// Klass 4–7, rörelsens kostnader.
    Kostnad,
    /// Klass 8, finansiella och andra intäkter och kostnader, bokslutsdispositioner, skatt och
    /// årets resultat.
    FinansiellPost,
}

impl Kontotyp {
    /// Om kontot hör till balansräkningen (klass 1–2) och inte resultaträkningen (klass 3–8).
    pub fn balanskonto(self) -> bool {
        matches!(self, Kontotyp::Tillgång | Kontotyp::EgetKapitalOchSkuld)
    }
}

impl std::fmt::Display for Kontonummer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
