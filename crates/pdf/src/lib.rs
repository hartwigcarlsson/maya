//! Mayas PDF:er ur inbyggda Typst-mallar (#65).
//!
//! Hela gränssnittet är [`skriv`]: ett [`Dokument`] och en [`Sidfot`] in, en PDF/A-2b ut.
//! Mallarna i `mallar/` och typsnitten i `fonts/` är inbäddade i binären; inget läses från
//! disken eller nätet vid körning. Datat går in som JSON via `sys.inputs` och tolkas därför
//! aldrig som Typst-markup, vad användaren än har skrivit.
//!
//! Sidfoten är gemensam för alla mallar: tidpunkt (Stockholm), användare, Maya-version,
//! INK2-tabellens utgåva och "Sida X av Y". Belopp, datum och tider formateras här, så att
//! alla PDF:er skriver dem likadant.
//!
//! Utdatat är byte-identiskt för samma indata. En utfärdad PDF ska ändå sparas som fil och inte
//! skrivas om, eftersom en uppgradering av Typst kan ändra utseendet (A3).
//!
//! Ny mall: lägg till en variant i [`Dokument`], en `.typ`-fil i `mallar/` som importerar
//! `mall.typ`, och en gren i `Dokument::mall`/`Dokument::data`.

mod data;
mod varld;

use jiff::Timestamp;
use jiff::civil::Date;
use maya_domain::belopp::Belopp;
use maya_domain::kontoplan::Kontonummer;

pub use data::Dokument;

/// Det som står längst ned på varje sida, utöver versionen och INK2-utgåvan som Maya själv vet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sidfot {
    /// När PDF:en skrevs. Också dokumentets datum i PDF-metadatan.
    pub tidpunkt: Timestamp,
    /// Användaren som skrev ut, med visningsnamn.
    pub användare: String,
}

/// Företaget som dokumentet hör till, i dokumentets huvud.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Företag {
    pub namn: String,
    /// Organisationsnumret som det visas, t.ex. `556036-0793`.
    pub orgnr: String,
}

/// Kontoplanen (S1), alla konton med status och INK2-rad, grupperade per kontoklass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kontoplan {
    pub företag: Företag,
    /// BAS-versionen som kontoplanen bygger på, t.ex. `BAS 2026`.
    pub bas_version: String,
    pub konton: Vec<Konto>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Konto {
    pub nummer: Kontonummer,
    pub namn: String,
    pub aktiv: bool,
}

/// Ett enskilt verifikat (A2): rubrik, nummer, metadata, rader, underlagsförteckning och
/// historik.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verifikat {
    pub företag: Företag,
    /// T.ex. `V5`; `None` för ett utkast.
    pub nummer: Option<String>,
    pub beskrivning: String,
    pub datum: Option<Date>,
    /// Perioden som den visas, t.ex. `mars 2026`.
    pub period: Option<String>,
    pub skapad: Signatur,
    pub bokförd: Option<Signatur>,
    pub rader: Vec<Rad>,
    pub underlag: Vec<Underlag>,
    pub historik: Vec<Historikpost>,
}

/// Vem som gjorde något och när.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signatur {
    pub tid: Timestamp,
    pub namn: String,
    /// Kanalen, t.ex. `webb`.
    pub kanal: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rad {
    pub konto: Kontonummer,
    pub kontonamn: String,
    pub text: Option<String>,
    pub debet: Belopp,
    pub kredit: Belopp,
}

/// Ett underlag i förteckningen, med SHA-256 från mottagandet (VF5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Underlag {
    pub filnamn: String,
    /// Hex, 64 tecken.
    pub sha256: String,
    /// Storlek i byte.
    pub storlek: u64,
    /// Vem och när, om underlaget kopplades efter bokföringen (VF6).
    pub komplettering: Option<Signatur>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Historikpost {
    pub tid: Timestamp,
    /// Händelsens typ, t.ex. `verifikat bokfört`.
    pub typ: String,
    pub namn: String,
    pub kanal: String,
}

#[derive(Debug, thiserror::Error)]
pub enum PdfFel {
    /// Mallen gick inte att kompilera eller blev inte PDF/A-2b. Mallarna är inbyggda, så det
    /// är ett fel i Maya och inte i indatat.
    #[error("PDF:en kunde inte skapas: {0}")]
    Mall(String),
}

/// Skriver dokumentet som PDF/A-2b.
pub fn skriv(dokument: &Dokument, sidfot: &Sidfot) -> Result<Vec<u8>, PdfFel> {
    varld::skriv(dokument.mall(), dokument.data(sidfot), sidfot.tidpunkt)
}
