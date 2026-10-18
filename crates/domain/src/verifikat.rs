//! Formuläret för ett verifikat och spärrarna vid bokföring. Formuläret i webbläsaren och
//! servern kör samma kontroll, och varje spärr har en kod som formuläret visar vid sitt fält.
//!
//! Spärrarna: verifikatdatum och beskrivning finns, minst två rader, debet = kredit (L7), bara
//! aktiva konton som finns i kontoplanen, ett räkenskapsår som finns eller som kan skapas
//! direkt efter det senaste, och inte före första räkenskapsårets start. Periodlåsningen kommer
//! i etapp 3.

use std::fmt;

use jiff::civil::Date;

use crate::belopp::Belopp;
use crate::kontoplan::Kontonummer;
use crate::rakenskapsar::{Period, Räkenskapsår};

/// En rad i formuläret, som text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NyRad {
    pub konto: String,
    pub debet: String,
    pub kredit: String,
    pub text: String,
}

impl NyRad {
    fn tom(&self) -> bool {
        [&self.konto, &self.debet, &self.kredit, &self.text]
            .iter()
            .all(|f| f.trim().is_empty())
    }
}

/// Formuläret som användaren fyllde i, som text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NyttVerifikat {
    /// `ÅÅÅÅ-MM-DD`.
    pub datum: String,
    pub beskrivning: String,
    /// Raderna i formulärets ordning. Helt tomma rader räknas inte.
    pub rader: Vec<NyRad>,
}

/// Det som spärrarna prövas mot: företagets kontoplan och räkenskapsår.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Förutsättningar {
    /// Kontona i kontoplanen och om de är aktiva.
    pub konton: Vec<(Kontonummer, bool)>,
    /// Räkenskapsåren i ordning.
    pub räkenskapsår: Vec<Räkenskapsår>,
}

/// Fältet som en spärr gäller. Raderna räknas från 0 i formulärets ordning, tomma rader
/// medräknade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Fält {
    Datum,
    Beskrivning,
    /// Raderna som helhet (för få rader).
    Rader,
    Konto(usize),
    Belopp(usize),
    /// Summaraden (differens).
    Summa,
}

/// Spärrarna vid bokföring som Systemdokumentationen återger (S2): koden och regeln. Varje kod
/// som [`NyttVerifikat::kontrollera`] kan ge står här, och ingen annan.
pub const SPÄRRAR: &[(&str, &str)] = &[
    ("datum_saknas", "Verifikatdatum krävs."),
    ("datum_ogiltigt", "Verifikatdatumet skrivs som ÅÅÅÅ-MM-DD."),
    (
        "fore_forsta_aret",
        "Verifikatdatumet är inte före första räkenskapsårets start.",
    ),
    (
        "saknat_rakenskapsar",
        "Verifikatdatumet ligger i ett räkenskapsår som finns, eller i året direkt efter det \
         senaste, som då skapas i samma transaktion.",
    ),
    ("beskrivning_saknas", "Beskrivning krävs."),
    ("rader_for_fa", "Verifikatet har minst två rader."),
    ("konto_saknas", "Varje rad med belopp har ett konto."),
    ("konto_okant", "Kontot finns i företagets kontoplan."),
    ("konto_inaktivt", "Kontot är aktivt."),
    (
        "belopp_ogiltigt",
        "Beloppet är i kronor, inte negativt och har högst två decimaler.",
    ),
    (
        "belopp_saknas",
        "Varje rad har ett belopp i debet eller kredit.",
    ),
    (
        "belopp_debet_och_kredit",
        "En rad har sitt belopp antingen i debet eller i kredit, inte båda.",
    ),
    ("differens", "Summa debet är lika med summa kredit (L7)."),
];

/// Något som hindrar bokföringen, med en kod som formuläret visar vid fältet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spärr {
    pub fält: Fält,
    pub kod: &'static str,
    pub text: String,
}

impl Spärr {
    fn ny(fält: Fält, kod: &'static str, text: impl ToString) -> Self {
        Spärr {
            fält,
            kod,
            text: text.to_string(),
        }
    }
}

impl fmt::Display for Spärr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// Räkenskapsåret som verifikatdatumet avgör.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Årsval {
    Befintligt(Räkenskapsår),
    /// Året direkt efter det senaste, som skapas i samma transaktion som verifikatet.
    Nytt(Räkenskapsår),
}

impl Årsval {
    pub fn år(&self) -> &Räkenskapsår {
        match self {
            Årsval::Befintligt(år) | Årsval::Nytt(år) => år,
        }
    }
}

/// En kontrollerad rad: ett belopp antingen i debet eller i kredit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rad {
    pub konto: Kontonummer,
    pub debet: Belopp,
    pub kredit: Belopp,
    pub text: Option<String>,
}

/// Ett verifikat som klarar alla spärrar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BokförbartVerifikat {
    pub datum: Date,
    pub beskrivning: String,
    pub rader: Vec<Rad>,
    pub räkenskapsår: Årsval,
    pub period: Period,
}

impl BokförbartVerifikat {
    /// Summan av debet, som är lika med summan av kredit.
    pub fn summa(&self) -> Belopp {
        Belopp::från_öre(self.rader.iter().map(|r| r.debet.öre()).sum())
    }
}

impl NyttVerifikat {
    /// Prövar spärrarna. Spärrarna kommer i fältens ordning.
    pub fn kontrollera(
        &self,
        förutsättningar: &Förutsättningar,
    ) -> Result<BokförbartVerifikat, Vec<Spärr>> {
        let mut spärrar = Vec::new();
        let datum = self.kontrollera_datum(&mut spärrar);
        let årsval = datum.and_then(|d| välj_år(d, &förutsättningar.räkenskapsår, &mut spärrar));
        let beskrivning = self.beskrivning.trim().to_string();
        if beskrivning.is_empty() {
            spärrar.push(Spärr::ny(
                Fält::Beskrivning,
                "beskrivning_saknas",
                "Krävs för att bokföra.",
            ));
        }

        let ifyllda: Vec<(usize, &NyRad)> = self
            .rader
            .iter()
            .enumerate()
            .filter(|(_, r)| !r.tom())
            .collect();
        if ifyllda.len() < 2 {
            spärrar.push(Spärr::ny(
                Fält::Rader,
                "rader_for_fa",
                "Ett verifikat behöver minst två rader.",
            ));
        }
        let mut rader = Vec::new();
        let mut radspärrar = Vec::new();
        let (mut debet, mut kredit) = (0i128, 0i128);
        for (i, rad) in ifyllda {
            let konto = kontrollera_konto(i, &rad.konto, förutsättningar, &mut radspärrar);
            let belopp = kontrollera_belopp(i, rad, &mut radspärrar);
            if let Some((d, k)) = belopp {
                debet += i128::from(d.öre());
                kredit += i128::from(k.öre());
            }
            if let (Some(konto), Some((debet, kredit))) = (konto, belopp) {
                let text = rad.text.trim();
                rader.push(Rad {
                    konto,
                    debet,
                    kredit,
                    text: (!text.is_empty()).then(|| text.to_string()),
                });
            }
        }
        spärrar.extend(radspärrar);

        if debet != kredit {
            let differens =
                Belopp::från_öre(i64::try_from((debet - kredit).abs()).unwrap_or(i64::MAX));
            let sida = if debet > kredit { "debet" } else { "kredit" };
            spärrar.push(Spärr::ny(
                Fält::Summa,
                "differens",
                format!("Differens {differens}: {sida} är större. Debet ska vara lika med kredit."),
            ));
        }

        match (datum, årsval) {
            (Some(datum), Some(räkenskapsår)) if spärrar.is_empty() => {
                let period = räkenskapsår
                    .år()
                    .perioder()
                    .into_iter()
                    .find(|p| p.start <= datum && datum <= p.slut)
                    .expect("en period i året som innehåller datumet");
                Ok(BokförbartVerifikat {
                    datum,
                    beskrivning,
                    rader,
                    räkenskapsår,
                    period,
                })
            }
            _ => Err(spärrar),
        }
    }

    fn kontrollera_datum(&self, spärrar: &mut Vec<Spärr>) -> Option<Date> {
        let text = self.datum.trim();
        if text.is_empty() {
            spärrar.push(Spärr::ny(
                Fält::Datum,
                "datum_saknas",
                "Krävs för att bokföra.",
            ));
            return None;
        }
        text.parse::<Date>()
            .map_err(|_| {
                spärrar.push(Spärr::ny(
                    Fält::Datum,
                    "datum_ogiltigt",
                    "Ange ett datum som ÅÅÅÅ-MM-DD.",
                ))
            })
            .ok()
    }
}

fn välj_år(datum: Date, år: &[Räkenskapsår], spärrar: &mut Vec<Spärr>) -> Option<Årsval> {
    let (Some(första), Some(senaste)) = (år.first(), år.last()) else {
        spärrar.push(Spärr::ny(
            Fält::Datum,
            "saknat_rakenskapsar",
            "Företaget har inget räkenskapsår.",
        ));
        return None;
    };
    if datum < första.start() {
        spärrar.push(Spärr::ny(
            Fält::Datum,
            "fore_forsta_aret",
            format!(
                "Datumet är före första räkenskapsårets start ({}).",
                första.start()
            ),
        ));
        return None;
    }
    if let Some(befintligt) = år.iter().find(|å| å.innehåller(datum)) {
        return Some(Årsval::Befintligt(*befintligt));
    }
    let nästa = senaste.nästa();
    if nästa.innehåller(datum) {
        return Some(Årsval::Nytt(nästa));
    }
    spärrar.push(Spärr::ny(
        Fält::Datum,
        "saknat_rakenskapsar",
        format!(
            "Det finns inget räkenskapsår för datumet. Nästa år som kan skapas är {} – {}.",
            nästa.start(),
            nästa.slut()
        ),
    ));
    None
}

fn kontrollera_konto(
    i: usize,
    text: &str,
    förutsättningar: &Förutsättningar,
    spärrar: &mut Vec<Spärr>,
) -> Option<Kontonummer> {
    let text = text.trim();
    if text.is_empty() {
        spärrar.push(Spärr::ny(
            Fält::Konto(i),
            "konto_saknas",
            "Välj ett konto.",
        ));
        return None;
    }
    let konto = text.parse::<Kontonummer>().ok();
    match konto.and_then(|k| förutsättningar.konton.iter().find(|(n, _)| *n == k)) {
        Some((konto, true)) => Some(*konto),
        Some((konto, false)) => {
            spärrar.push(Spärr::ny(
                Fält::Konto(i),
                "konto_inaktivt",
                format!("Konto {konto} är inaktivt. Aktivera det för att bokföra på det."),
            ));
            None
        }
        None => {
            spärrar.push(Spärr::ny(
                Fält::Konto(i),
                "konto_okant",
                format!("Konto {text} finns inte i kontoplanen."),
            ));
            None
        }
    }
}

fn kontrollera_belopp(
    i: usize, rad: &NyRad, spärrar: &mut Vec<Spärr>
) -> Option<(Belopp, Belopp)> {
    let mut tolka = |text: &str| {
        let text = text.trim();
        if text.is_empty() {
            return Some(Belopp::NOLL);
        }
        let belopp = Belopp::tolka(text);
        if belopp.is_none() {
            spärrar.push(Spärr::ny(
                Fält::Belopp(i),
                "belopp_ogiltigt",
                "Ange ett belopp i kronor med högst två decimaler.",
            ));
        }
        belopp
    };
    let debet = tolka(&rad.debet)?;
    let kredit = tolka(&rad.kredit)?;
    match (debet != Belopp::NOLL, kredit != Belopp::NOLL) {
        (true, false) | (false, true) => Some((debet, kredit)),
        (false, false) => {
            spärrar.push(Spärr::ny(
                Fält::Belopp(i),
                "belopp_saknas",
                "Ange ett belopp i debet eller kredit.",
            ));
            None
        }
        (true, true) => {
            spärrar.push(Spärr::ny(
                Fält::Belopp(i),
                "belopp_debet_och_kredit",
                "En rad har ett belopp antingen i debet eller i kredit.",
            ));
            None
        }
    }
}

/// En rad i ett utkast. Konto och belopp får saknas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Utkastrad {
    pub konto: Option<Kontonummer>,
    pub debet: Belopp,
    pub kredit: Belopp,
    pub text: Option<String>,
}

/// Ett utkast som går att spara: det som är ifyllt går att lagra, men behöver inte vara
/// balanserat eller komplett.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SparbartUtkast {
    pub datum: Option<Date>,
    pub beskrivning: String,
    /// Raderna i formulärets ordning, utan helt tomma rader.
    pub rader: Vec<Utkastrad>,
    /// Räkenskapsåret som datumet avgör; `None` utan datum.
    pub räkenskapsår: Option<Årsval>,
}

impl NyttVerifikat {
    /// Prövar det som måste gälla för att ett utkast ska kunna sparas: ett datum som är ett
    /// datum och ligger i ett räkenskapsår som finns eller kan skapas, kontonummer som är
    /// kontonummer och belopp som är belopp. Ett utkast får vara obalanserat, sakna
    /// beskrivning, rader, konton och belopp, och använda inaktiva konton. Spärrarna har samma
    /// koder som vid bokföring.
    pub fn som_utkast(
        &self,
        förutsättningar: &Förutsättningar,
    ) -> Result<SparbartUtkast, Vec<Spärr>> {
        let mut spärrar = Vec::new();
        let datum = if self.datum.trim().is_empty() {
            None
        } else {
            self.kontrollera_datum(&mut spärrar)
        };
        let räkenskapsår =
            datum.and_then(|d| välj_år(d, &förutsättningar.räkenskapsår, &mut spärrar));
        let mut rader = Vec::new();
        for (i, rad) in self.rader.iter().enumerate().filter(|(_, r)| !r.tom()) {
            let text = rad.konto.trim();
            let konto = if text.is_empty() {
                None
            } else {
                let konto = text.parse::<Kontonummer>().ok();
                if konto.is_none() {
                    spärrar.push(Spärr::ny(
                        Fält::Konto(i),
                        "konto_okant",
                        format!("Konto {text} finns inte i kontoplanen."),
                    ));
                }
                konto
            };
            let mut ogiltigt = false;
            let mut belopp = |text: &str| {
                let text = text.trim();
                if text.is_empty() {
                    return Belopp::NOLL;
                }
                Belopp::tolka(text).unwrap_or_else(|| {
                    ogiltigt = true;
                    Belopp::NOLL
                })
            };
            let (debet, kredit) = (belopp(&rad.debet), belopp(&rad.kredit));
            if ogiltigt {
                spärrar.push(Spärr::ny(
                    Fält::Belopp(i),
                    "belopp_ogiltigt",
                    "Ange ett belopp i kronor med högst två decimaler.",
                ));
            }
            let text = rad.text.trim();
            rader.push(Utkastrad {
                konto,
                debet,
                kredit,
                text: (!text.is_empty()).then(|| text.to_string()),
            });
        }
        if !spärrar.is_empty() {
            return Err(spärrar);
        }
        Ok(SparbartUtkast {
            datum,
            beskrivning: self.beskrivning.trim().to_string(),
            rader,
            räkenskapsår,
        })
    }
}
