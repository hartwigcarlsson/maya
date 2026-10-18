//! Driftens regler (beslut 11 och 13 i #70): när backupen är för gammal och när ett larm till
//! driften ska skickas. Tiderna är absoluta, så tidszonen spelar ingen roll här.

use jiff::{SignedDuration, Timestamp};

/// En backup äldre än så här ger en driftvarning och ett larm.
pub const BACKUPENS_ÅLDER: SignedDuration = SignedDuration::from_hours(36);
/// Ett larm om samma fel skickas högst en gång per så här lång tid.
pub const LARMINTERVALL: SignedDuration = SignedDuration::from_hours(24);

/// Om backupen är för gammal vid `nu`. `senaste` är den senaste lyckade backupen; utan någon
/// räknas åldern från `sedan`, då installationen började användas. Utan båda finns inget att
/// säkerhetskopiera än.
pub fn backupen_för_gammal(
    nu: Timestamp,
    senaste: Option<Timestamp>,
    sedan: Option<Timestamp>,
) -> bool {
    senaste
        .or(sedan)
        .is_some_and(|t| nu.duration_since(t) > BACKUPENS_ÅLDER)
}

/// Felen som driften får larm om.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Larmfel {
    /// Den senaste backupen misslyckades.
    BackupMisslyckades,
    /// Den senaste lyckade backupen är äldre än 36 timmar.
    BackupGammal,
    /// Den senaste verifieringen misslyckades.
    VerifieringMisslyckades,
}

impl Larmfel {
    pub const ALLA: [Larmfel; 3] = [
        Larmfel::BackupMisslyckades,
        Larmfel::BackupGammal,
        Larmfel::VerifieringMisslyckades,
    ];

    /// Felets kod, som den sparas.
    pub fn kod(self) -> &'static str {
        match self {
            Larmfel::BackupMisslyckades => "backup_misslyckades",
            Larmfel::BackupGammal => "backup_gammal",
            Larmfel::VerifieringMisslyckades => "verifiering_misslyckades",
        }
    }

    pub fn från_kod(kod: &str) -> Option<Self> {
        Self::ALLA.into_iter().find(|f| f.kod() == kod)
    }

    /// Felet i en mening, t.ex. i ämnesraden.
    pub fn namn(self) -> &'static str {
        match self {
            Larmfel::BackupMisslyckades => "backupen misslyckades",
            Larmfel::BackupGammal => "backupen är äldre än 36 timmar",
            Larmfel::VerifieringMisslyckades => "verifieringen av backupen misslyckades",
        }
    }
}

/// Ett larm som har skickats för ett fel som inte har lösts än.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Skickat {
    pub fel: Larmfel,
    pub tid: Timestamp,
}

/// Ett larm att skicka.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Larm {
    /// Felet gäller: första gången, eller ett dygn efter förra larmet.
    Fel(Larmfel),
    /// Felet som det larmades om är löst.
    Löst(Larmfel),
}

/// Larmen att skicka vid `nu`, när felen `aktiva` gäller och larmen `skickade` har skickats om fel
/// som inte har lösts. Högst ett per dygn och fel, och ett när felet är löst.
pub fn larm(nu: Timestamp, aktiva: &[Larmfel], skickade: &[Skickat]) -> Vec<Larm> {
    let mut larm = Vec::new();
    for fel in Larmfel::ALLA {
        let skickat = skickade.iter().find(|s| s.fel == fel);
        match (aktiva.contains(&fel), skickat) {
            (true, None) => larm.push(Larm::Fel(fel)),
            (true, Some(s)) if nu.duration_since(s.tid) >= LARMINTERVALL => {
                larm.push(Larm::Fel(fel))
            }
            (false, Some(_)) => larm.push(Larm::Löst(fel)),
            _ => {}
        }
    }
    larm
}
