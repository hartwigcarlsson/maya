//! Formuläret för att lägga upp ett företag. Formuläret i webbläsaren och servern kontrollerar
//! det med samma kod, och varje fel har en kod som visas vid sitt fält.

use jiff::civil::Date;

use crate::orgnr::Organisationsnummer;
use crate::rakenskapsar::Räkenskapsår;

/// Fakturametoden eller kontantmetoden.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Bokföringsmetod {
    Faktura,
    Kontant,
}

impl Bokföringsmetod {
    pub const ALLA: [Bokföringsmetod; 2] = [Bokföringsmetod::Faktura, Bokföringsmetod::Kontant];

    pub fn kod(self) -> &'static str {
        match self {
            Bokföringsmetod::Faktura => "faktura",
            Bokföringsmetod::Kontant => "kontant",
        }
    }

    pub fn namn(self) -> &'static str {
        match self {
            Bokföringsmetod::Faktura => "Fakturametoden",
            Bokföringsmetod::Kontant => "Kontantmetoden",
        }
    }

    pub fn från_kod(kod: &str) -> Option<Self> {
        Self::ALLA.into_iter().find(|m| m.kod() == kod)
    }
}

/// Perioden som företaget redovisar moms för, eller att det inte är momsregistrerat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Momsperiod {
    Månad,
    Kvartal,
    Helår,
    EjMomsregistrerad,
}

impl Momsperiod {
    pub const ALLA: [Momsperiod; 4] = [
        Momsperiod::Månad,
        Momsperiod::Kvartal,
        Momsperiod::Helår,
        Momsperiod::EjMomsregistrerad,
    ];

    pub fn kod(self) -> &'static str {
        match self {
            Momsperiod::Månad => "manad",
            Momsperiod::Kvartal => "kvartal",
            Momsperiod::Helår => "helar",
            Momsperiod::EjMomsregistrerad => "ej_momsregistrerad",
        }
    }

    pub fn namn(self) -> &'static str {
        match self {
            Momsperiod::Månad => "Månad",
            Momsperiod::Kvartal => "Kvartal",
            Momsperiod::Helår => "Helår",
            Momsperiod::EjMomsregistrerad => "Ej momsregistrerad",
        }
    }

    pub fn från_kod(kod: &str) -> Option<Self> {
        Self::ALLA.into_iter().find(|m| m.kod() == kod)
    }
}

/// Formulärets fält, med namnen som formuläret använder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Fält {
    Orgnr,
    Namn,
    FörstaÅrStart,
    FörstaÅrSlut,
    Bokföringsmetod,
    Momsperiod,
}

impl Fält {
    pub fn namn(self) -> &'static str {
        match self {
            Fält::Orgnr => "orgnr",
            Fält::Namn => "namn",
            Fält::FörstaÅrStart => "start",
            Fält::FörstaÅrSlut => "slut",
            Fält::Bokföringsmetod => "bokforingsmetod",
            Fält::Momsperiod => "momsperiod",
        }
    }
}

/// Ett fel vid ett fält.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fältfel {
    pub fält: Fält,
    pub kod: &'static str,
    pub text: String,
}

impl Fältfel {
    fn nytt(fält: Fält, kod: &'static str, text: impl ToString) -> Self {
        Fältfel {
            fält,
            kod,
            text: text.to_string(),
        }
    }
}

/// Formuläret som användaren fyllde i, som text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NyttFöretag {
    pub orgnr: String,
    pub namn: String,
    /// `ÅÅÅÅ-MM-DD`.
    pub första_år_start: String,
    /// `ÅÅÅÅ-MM-DD`.
    pub första_år_slut: String,
    pub bokföringsmetod: String,
    pub momsperiod: String,
}

/// Ett kontrollerat formulär.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GiltigtFöretag {
    pub orgnr: Organisationsnummer,
    pub namn: String,
    pub första_år: Räkenskapsår,
    pub bokföringsmetod: Bokföringsmetod,
    pub momsperiod: Momsperiod,
}

impl NyttFöretag {
    /// Kontrollerar formuläret. Felen kommer i fältens ordning.
    pub fn kontrollera(&self) -> Result<GiltigtFöretag, Vec<Fältfel>> {
        let mut fel = Vec::new();
        let orgnr = Organisationsnummer::tolka(&self.orgnr)
            .map_err(|e| fel.push(Fältfel::nytt(Fält::Orgnr, e.kod(), e)))
            .ok();
        let namn = self.namn.trim().to_string();
        if namn.is_empty() {
            fel.push(Fältfel::nytt(
                Fält::Namn,
                "namn_saknas",
                "Företaget måste ha ett namn.",
            ));
        }
        let start = datum(&self.första_år_start, Fält::FörstaÅrStart, &mut fel);
        let slut = datum(&self.första_år_slut, Fält::FörstaÅrSlut, &mut fel);
        let första_år = match (start, slut) {
            (Some(start), Some(slut)) => Räkenskapsår::första(start, slut)
                .map_err(|e| fel.push(Fältfel::nytt(Fält::FörstaÅrSlut, e.kod(), e)))
                .ok(),
            _ => None,
        };
        let bokföringsmetod = val(
            Bokföringsmetod::från_kod(&self.bokföringsmetod),
            Fält::Bokföringsmetod,
            "Välj bokföringsmetod.",
            &mut fel,
        );
        let momsperiod = val(
            Momsperiod::från_kod(&self.momsperiod),
            Fält::Momsperiod,
            "Välj momsperiod.",
            &mut fel,
        );
        match (orgnr, första_år, bokföringsmetod, momsperiod) {
            (Some(orgnr), Some(första_år), Some(bokföringsmetod), Some(momsperiod))
                if fel.is_empty() =>
            {
                Ok(GiltigtFöretag {
                    orgnr,
                    namn,
                    första_år,
                    bokföringsmetod,
                    momsperiod,
                })
            }
            _ => Err(fel),
        }
    }
}

fn datum(text: &str, fält: Fält, fel: &mut Vec<Fältfel>) -> Option<Date> {
    text.trim()
        .parse::<Date>()
        .map_err(|_| {
            fel.push(Fältfel::nytt(
                fält,
                "datum_ogiltigt",
                "Ange ett datum som ÅÅÅÅ-MM-DD.",
            ))
        })
        .ok()
}

fn val<T>(val: Option<T>, fält: Fält, text: &str, fel: &mut Vec<Fältfel>) -> Option<T> {
    if val.is_none() {
        fel.push(Fältfel::nytt(fält, "val_saknas", text));
    }
    val
}
