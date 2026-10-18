//! Aktören som utför en handling, och kanalpolicyn som avgör vad varje kanal får göra
//! (ADR 0003). Middleware gör om sessionscookien till en aktör; `core` auktoriserar.

use std::fmt;

/// En användares id i installationen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AnvändarId(pub(crate) i64);

/// Vem som helst kan skriva ett id; `core` avgör vad aktören får se och göra med det.
impl std::str::FromStr for AnvändarId {
    type Err = std::num::ParseIntError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse().map(AnvändarId)
    }
}

impl fmt::Display for AnvändarId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Vägen in till Maya. MCP läggs till i etapp 6.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kanal {
    Webb,
    /// Server-CLI:t, som den som driver servern kör.
    Cli,
    /// Maya självt: bakgrundsjobb som backup och larm.
    Maya,
}

impl Kanal {
    pub fn namn(self) -> &'static str {
        match self {
            Kanal::Webb => "webb",
            Kanal::Cli => "cli",
            Kanal::Maya => "maya",
        }
    }

    pub(crate) fn från_namn(namn: &str) -> Option<Self> {
        match namn {
            "webb" => Some(Kanal::Webb),
            "cli" => Some(Kanal::Cli),
            "maya" => Some(Kanal::Maya),
            _ => None,
        }
    }
}

/// Den som utför en handling: en användare genom en kanal, den som driver servern genom CLI:t,
/// eller Maya självt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Aktör {
    pub kanal: Kanal,
    pub användare: Option<AnvändarId>,
}

impl Aktör {
    /// En inloggad användare i webben. Bara sessionen ger en sådan aktör.
    pub fn webb(användare: AnvändarId) -> Self {
        Aktör {
            kanal: Kanal::Webb,
            användare: Some(användare),
        }
    }

    /// Den som driver servern och kör CLI:t där.
    pub fn cli() -> Self {
        Aktör {
            kanal: Kanal::Cli,
            användare: None,
        }
    }

    /// Maya självt.
    pub fn maya() -> Self {
        Aktör {
            kanal: Kanal::Maya,
            användare: None,
        }
    }
}

/// Handlingar som kanalpolicyn avgör.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handling {
    /// Skapa, återställa, avaktivera och återaktivera en användare, och byta domän. Bara CLI:t
    /// (ADR 0005).
    HanteraAnvändare,
    /// Lägga upp ett företag. Bara webben (beslut i #61).
    LäggaUppFöretag,
    /// Läsa ett företags uppgifter, kontoplan och behandlingshistorik.
    LäsaFöretag,
    /// Skapa egna konton, byta namn på och aktivera och inaktivera konton. Bara webben, aldrig
    /// AI (beslut i #82).
    ÄndraKontoplan,
    /// Bokföra ett verifikat. MCP läggs till i etapp 6.
    Bokföra,
    /// Ändra företagets uppgifter, bokföringsmetod, momsperiod och första räkenskapsår.
    ÄndraFöretag,
    /// Radera ett företag utan räkenskapsinformation.
    RaderaFöretag,
    /// Spara, ändra och radera ett utkast. MCP läggs till i etapp 6.
    HanteraUtkast,
    /// Skriva in eller ändra den ingående balansen.
    ÄndraIngåendeBalans,
    /// Ladda upp ett underlag och ta bort ett som aldrig har hört till ett bokfört verifikat.
    HanteraUnderlag,
    /// Komplettera ett bokfört verifikat med ett underlag (VF6).
    Komplettera,
    /// Bjuda in till ett företag och ge och ta åtkomst. Bara webben, med färsk passkey.
    HanteraÅtkomst,
    /// Exportera ett räkenskapsår som SIE4. Bara webben, ingen export via MCP eller CLI (beslut
    /// i #67).
    ExporteraSie4,
    /// Ta den nattliga backupen. Bara Maya självt.
    TaBackup,
    /// Verifiera en backup: Maya självt efter varje backup och driften med `maya backup verify`.
    VerifieraBackup,
}

/// Kanalpolicyn: om `kanal` får utföra `handling`.
pub fn tillåter(kanal: Kanal, handling: Handling) -> bool {
    match handling {
        Handling::HanteraAnvändare => kanal == Kanal::Cli,
        Handling::TaBackup => kanal == Kanal::Maya,
        Handling::VerifieraBackup => matches!(kanal, Kanal::Maya | Kanal::Cli),
        Handling::LäggaUppFöretag
        | Handling::LäsaFöretag
        | Handling::ÄndraKontoplan
        | Handling::Bokföra
        | Handling::ÄndraFöretag
        | Handling::RaderaFöretag
        | Handling::HanteraUtkast
        | Handling::ÄndraIngåendeBalans
        | Handling::HanteraUnderlag
        | Handling::Komplettera
        | Handling::HanteraÅtkomst
        | Handling::ExporteraSie4 => kanal == Kanal::Webb,
    }
}
