//! Aktören som utför en handling, och kanalpolicyn som avgör vad varje kanal får göra
//! (ADR 0003). Middleware gör om sessionscookien till en aktör; `core` auktoriserar.

use std::fmt;

/// En användares id i installationen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AnvändarId(pub(crate) i64);

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
}

/// Kanalpolicyn: om `kanal` får utföra `handling`.
pub fn tillåter(kanal: Kanal, handling: Handling) -> bool {
    match handling {
        Handling::HanteraAnvändare => kanal == Kanal::Cli,
    }
}
