//! Betalningen av en faktura i reskontran: panelens formulär, spärrarna och konteringen enligt
//! tabellen över automatkonteringar. Kund- och leverantörsfakturor delar reglerna; det som
//! skiljer är reskontrakontot (1510 eller 2440) och vilken sida betalkontot står på.
//!
//! Betalningen bokförs mot reskontrakontot (fordran eller skulden är bokförd, fakturametoden).
//! En delbetalning lämnar resten kvar. En rest på högst 10 kr kan stängas mot öresutjämning
//! (3740) i samma verifikat. En överbetalning spärras och hänvisas till ett manuellt verifikat,
//! så att det återstående beloppet aldrig blir negativt av misstag. Specen säger det om
//! kundfakturor; leverantörsfakturorna har samma spärr, utom när avvikelsen stängs mot
//! kursdifferens.
//!
//! Har motparten pengar tillgodo (det som återstår är negativt, efter en kreditfaktura #140) är
//! betalningen en *återbetalning*: en betalning åt andra hållet, med 1510 debet mot betalkontot
//! för en kund och betalkontot debet mot 2440 för en leverantör. Användaren skriver beloppet som
//! ett positivt belopp, och [`Betalning::belopp`] blir negativt.
//!
//! **Kursdifferens** (#147): en leverantörsfaktura med originalvaluta är registrerad i SEK, och
//! betalningen i SEK kan bli en annan. Avvikelsen mellan det som återstår och det som betalades
//! stängs då i samma verifikat, mot 7960 vid förlust (betalningen är större) och mot 3960 vid
//! vinst (betalningen är mindre), så att inget återstår.
//!
//! **Fordran eller skulden är inte bokförd** (kontantmetoden, #149): betalningen bokförs då mot
//! fakturans intäkt eller kostnad och moms (och fiktiv moms) i proportion till beloppet
//! ([`crate::kontantmetoden::Obokförd`]), en rest som stängs mot öresutjämning bokförs inte,
//! och en kursdifferens bokförs som vanligt i samma verifikat. En återbetalning av ett
//! tillgodohavande bokförs alltid mot reskontrakontot.

use std::fmt;

use jiff::civil::Date;

use crate::belopp::{Belopp, Saldo};
use crate::kontantmetoden::Obokförd;
use crate::kontoplan::Kontonummer;
use crate::rakenskapsar::Räkenskapsår;
use crate::verifikat::{self, Årsval};

/// Betalkontot som panelen förväljer: Företagskonto.
pub const BETALKONTO: u16 = 1930;

/// Öresutjämning, som tar emot en liten rest.
pub const ÖRESUTJÄMNING: u16 = 3740;

/// Den största rest i öre som kan stängas mot öresutjämning: 10 kr.
pub const STÖRSTA_ÖRESUTJÄMNING: i64 = 1_000;

/// Valutakursvinster, som tar emot en betalning som blev mindre än skulden i SEK.
pub const KURSVINST: u16 = 3960;

/// Valutakursförluster, som tar emot en betalning som blev större än skulden i SEK.
pub const KURSFÖRLUST: u16 = 7960;

/// Vilken reskontra fakturan hör till.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Reskontra {
    /// Kundfakturor: fordran på 1510, betalningen kommer in på betalkontot.
    Kund,
    /// Leverantörsfakturor: skulden på 2440, betalningen går ut från betalkontot.
    Leverantör,
}

impl Reskontra {
    /// Reskontrakontot: kundfordringar 1510 eller leverantörsskulder 2440.
    pub fn konto(self) -> Kontonummer {
        Kontonummer::konstant(match self {
            Reskontra::Kund => 1510,
            Reskontra::Leverantör => 2440,
        })
    }
}

/// Betalningen som användaren skrev den i panelen.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NyBetalning {
    /// `ÅÅÅÅ-MM-DD`.
    pub betaldag: String,
    /// Kronor.
    pub belopp: String,
    /// Betalkontot, ett aktivt konto i klass 1.
    pub konto: String,
    /// Stäng resten mot öresutjämning.
    pub öresutjämning: bool,
    /// Stäng avvikelsen mot kursdifferens (#147): bara på en leverantörsfaktura med
    /// originalvaluta.
    #[serde(default)]
    pub kursdifferens: bool,
}

/// Det som betalningen prövas mot, läst i samma transaktion som den bokförs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Förutsättningar {
    pub reskontra: Reskontra,
    /// Det som återstår av fakturan i öre före betalningen.
    pub återstår: i64,
    /// Kontona och om de är aktiva.
    pub konton: Vec<(Kontonummer, bool)>,
    pub räkenskapsår: Vec<Räkenskapsår>,
    /// Fakturan har en originalvaluta, så att en avvikelse kan stängas mot kursdifferens.
    /// Bara leverantörsfakturor har det.
    pub originalvaluta: bool,
    /// Fakturans kontering när dess fordran eller skuld inte är bokförd (kontantmetoden,
    /// #149); `None` när den är bokförd på 1510 eller 2440.
    pub obokförd: Option<Obokförd>,
}

/// Fälten i panelen, där spärrarna visas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fält {
    Betaldag,
    Belopp,
    Konto,
    Öresutjämning,
    Kursdifferens,
}

/// Spärrarna vid betalning som Systemdokumentationen återger (S2). Varje kod som
/// [`NyBetalning::kontrollera`] kan ge står här, och ingen annan.
pub const BETALNINGSSPÄRRAR: &[(&str, &str)] = &[
    ("betaldag_saknas", "Betaldag krävs."),
    ("betaldag_ogiltig", "Betaldagen skrivs som ÅÅÅÅ-MM-DD."),
    (
        "fore_forsta_aret",
        "Betaldagen är inte före första räkenskapsårets start.",
    ),
    (
        "saknat_rakenskapsar",
        "Betaldagen ligger i ett räkenskapsår som finns, eller i året direkt efter det senaste, \
         som då skapas när betalningen bokförs.",
    ),
    (
        "redan_betald",
        "Fakturan har ett belopp kvar att betala, eller ett tillgodohavande att återbetala.",
    ),
    ("belopp_saknas", "Belopp krävs."),
    (
        "belopp_ogiltigt",
        "Beloppet är i kronor, större än noll och har högst två decimaler.",
    ),
    (
        "overbetalning",
        "Beloppet är högst det som återstår. En överbetalning bokförs som ett manuellt verifikat.",
    ),
    (
        "overaterbetalning",
        "En återbetalning är högst det som motparten har tillgodo.",
    ),
    ("konto_saknas", "Betalkonto krävs."),
    ("konto_okant", "Kontot finns i företagets kontoplan."),
    ("konto_inaktivt", "Kontot är aktivt."),
    ("konto_inte_klass_1", "Betalkontot är ett konto i klass 1."),
    (
        "oresutjamning_for_stor",
        "En rest stängs mot öresutjämning bara när den är högst 10 kr.",
    ),
    (
        "oresutjamning_utan_rest",
        "Öresutjämning kräver en rest efter betalningen; en återbetalning har ingen.",
    ),
    (
        "kursdifferens_utan_originalvaluta",
        "Kursdifferens finns bara på en leverantörsfaktura med originalvaluta.",
    ),
    (
        "kursdifferens_utan_avvikelse",
        "Kursdifferens kräver att betalningen avviker från det som återstår.",
    ),
    (
        "kursdifferens_och_oresutjamning",
        "En avvikelse stängs mot kursdifferens eller öresutjämning, inte båda.",
    ),
];

/// Något som hindrar betalningen, med en kod som panelen visar vid fältet.
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

/// En betalning som klarar spärrarna och kan bokföras.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Betalning {
    pub reskontra: Reskontra,
    pub betaldag: Date,
    /// Det som betalades, i öre; negativt för en återbetalning av ett tillgodohavande.
    pub belopp: i64,
    pub konto: Kontonummer,
    /// Resten som stängs mot öresutjämning, i öre; noll utan öresutjämning.
    pub öresutjämning: i64,
    /// Avvikelsen som stängs mot kursdifferens, i öre: positiv vid vinst (3960), negativ vid
    /// förlust (7960) och noll utan kursdifferens.
    pub kursdifferens: i64,
    /// Det som återstår av fakturan efter betalningen.
    pub återstår_efter: i64,
    /// Räkenskapsåret som betaldagen avgör.
    pub räkenskapsår: Årsval,
    /// Fakturans kontering när fordran eller skulden inte är bokförd (#149).
    pub obokförd: Option<Obokförd>,
}

/// Verifikatets beskrivning med fakturan som referens: "Betalning av kundfaktura 1007, Kund AB".
pub fn beskrivning(faktura: &str) -> String {
    format!("Betalning av {faktura}")
}

/// Återbetalningens beskrivning med fakturan som referens: "Återbetalning av tillgodohavande på
/// kundfaktura 1007, Kund AB".
pub fn återbetalningens_beskrivning(faktura: &str) -> String {
    format!("Återbetalning av tillgodohavande på {faktura}")
}

impl NyBetalning {
    /// Panelen när den öppnas: det som återstår (eller tillgodohavandet att återbetala),
    /// dagens datum och 1930.
    pub fn förvald(återstår: i64, idag: Date) -> Self {
        NyBetalning {
            betaldag: idag.to_string(),
            belopp: Belopp::från_öre(återstår.abs()).to_string(),
            konto: BETALKONTO.to_string(),
            ..Default::default()
        }
    }

    /// Det som återstår efter betalningen om beloppet går att tolka, för texten
    /// "Delbetalning, X kr återstår" och kryssrutan medan användaren skriver.
    pub fn rest(&self, återstår: i64) -> Option<i64> {
        tolka_belopp(&self.belopp).map(|b| återstår - b * återstår.signum())
    }

    /// Prövar spärrarna. Spärrarna kommer i fältens ordning.
    pub fn kontrollera(&self, f: &Förutsättningar) -> Result<Betalning, Vec<Spärr>> {
        let mut spärrar = Vec::new();
        let betaldag = match self.betaldag.trim() {
            "" => {
                spärrar.push(Spärr::ny(
                    Fält::Betaldag,
                    "betaldag_saknas",
                    "Ange betaldag.",
                ));
                None
            }
            text => match text.parse::<Date>() {
                Ok(d) => Some(d),
                Err(_) => {
                    spärrar.push(Spärr::ny(
                        Fält::Betaldag,
                        "betaldag_ogiltig",
                        "Skriv betaldagen som ÅÅÅÅ-MM-DD.",
                    ));
                    None
                }
            },
        };
        let årsval = betaldag.and_then(|d| {
            let mut fel = Vec::new();
            let val = verifikat::välj_år(d, &f.räkenskapsår, &mut fel);
            spärrar.extend(fel.into_iter().map(|s| {
                Spärr::ny(
                    Fält::Betaldag,
                    s.kod,
                    s.text.replace("Datumet", "Betaldagen"),
                )
            }));
            val
        });

        // En återbetalning har beloppet med motsatt tecken mot en betalning.
        let tecken = if f.återstår < 0 { -1 } else { 1 };
        // Kursdifferens stänger avvikelsen åt båda hållen, så en större betalning är ingen
        // överbetalning då. En återbetalning har ingen kursdifferens.
        let kurs = self.kursdifferens
            && f.reskontra == Reskontra::Leverantör
            && f.originalvaluta
            && tecken > 0;
        let belopp = if f.återstår == 0 {
            spärrar.push(Spärr::ny(
                Fält::Belopp,
                "redan_betald",
                "Fakturan är redan betald.",
            ));
            None
        } else {
            match self.belopp.trim() {
                "" => {
                    spärrar.push(Spärr::ny(Fält::Belopp, "belopp_saknas", "Ange belopp."));
                    None
                }
                text => match tolka_belopp(text) {
                    None => {
                        spärrar.push(Spärr::ny(
                            Fält::Belopp,
                            "belopp_ogiltigt",
                            "Skriv beloppet i kronor, större än noll, t.ex. 1 250,00.",
                        ));
                        None
                    }
                    Some(b) if tecken < 0 && b > -f.återstår => {
                        spärrar.push(Spärr::ny(
                            Fält::Belopp,
                            "overaterbetalning",
                            format!(
                                "Beloppet är större än tillgodohavandet, {} kr.",
                                Belopp::från_öre(-f.återstår)
                            ),
                        ));
                        None
                    }
                    Some(b) if tecken > 0 && b > f.återstår && !kurs => {
                        spärrar.push(Spärr::ny(
                            Fält::Belopp,
                            "overbetalning",
                            format!(
                                "Beloppet är större än det som återstår, {} kr. Bokför \
                                 överbetalningen som ett manuellt verifikat.",
                                Belopp::från_öre(f.återstår)
                            ),
                        ));
                        None
                    }
                    Some(b) => Some(b * tecken),
                },
            }
        };

        let konto = self.kontrollera_konto(f, &mut spärrar);

        let kursdifferens = match (belopp, self.kursdifferens) {
            (_, true) if !kurs => {
                spärrar.push(Spärr::ny(
                    Fält::Kursdifferens,
                    "kursdifferens_utan_originalvaluta",
                    "Fakturan har ingen originalvaluta, så det finns ingen kursdifferens att stänga.",
                ));
                0
            }
            (Some(b), true) if self.öresutjämning => {
                spärrar.push(Spärr::ny(
                    Fält::Kursdifferens,
                    "kursdifferens_och_oresutjamning",
                    "Välj kursdifferens eller öresutjämning, inte båda.",
                ));
                f.återstår - b
            }
            (Some(b), true) if b == f.återstår => {
                spärrar.push(Spärr::ny(
                    Fält::Kursdifferens,
                    "kursdifferens_utan_avvikelse",
                    "Betalningen är lika med det som återstår; det finns ingen kursdifferens.",
                ));
                0
            }
            (Some(b), true) => f.återstår - b,
            _ => 0,
        };

        let öresutjämning = match (belopp, self.öresutjämning) {
            (Some(_), true) if tecken < 0 => {
                spärrar.push(Spärr::ny(
                    Fält::Öresutjämning,
                    "oresutjamning_utan_rest",
                    "En återbetalning stängs inte mot öresutjämning.",
                ));
                0
            }
            (Some(_), true) if self.kursdifferens => 0,
            (Some(b), true) => {
                let rest = f.återstår - b;
                if rest == 0 {
                    spärrar.push(Spärr::ny(
                        Fält::Öresutjämning,
                        "oresutjamning_utan_rest",
                        "Hela beloppet betalas; det finns ingen rest att stänga.",
                    ));
                } else if rest > STÖRSTA_ÖRESUTJÄMNING {
                    spärrar.push(Spärr::ny(
                        Fält::Öresutjämning,
                        "oresutjamning_for_stor",
                        format!(
                            "Resten {} kr är mer än 10 kr och kan inte stängas mot \
                             öresutjämning.",
                            Belopp::från_öre(rest)
                        ),
                    ));
                }
                rest
            }
            _ => 0,
        };

        match (betaldag, årsval, belopp, konto) {
            (Some(betaldag), Some(räkenskapsår), Some(belopp), Some(konto))
                if spärrar.is_empty() =>
            {
                Ok(Betalning {
                    reskontra: f.reskontra,
                    betaldag,
                    belopp,
                    konto,
                    öresutjämning,
                    kursdifferens,
                    återstår_efter: f.återstår - belopp - öresutjämning - kursdifferens,
                    räkenskapsår,
                    obokförd: f.obokförd.clone(),
                })
            }
            _ => Err(spärrar),
        }
    }

    fn kontrollera_konto(
        &self,
        f: &Förutsättningar,
        spärrar: &mut Vec<Spärr>,
    ) -> Option<Kontonummer> {
        let text = self.konto.trim();
        if text.is_empty() {
            spärrar.push(Spärr::ny(Fält::Konto, "konto_saknas", "Välj betalkonto."));
            return None;
        }
        let Some((konto, aktiv)) = text
            .parse::<Kontonummer>()
            .ok()
            .and_then(|k| f.konton.iter().find(|(nr, _)| *nr == k))
        else {
            spärrar.push(Spärr::ny(
                Fält::Konto,
                "konto_okant",
                format!("Konto {text} finns inte i kontoplanen."),
            ));
            return None;
        };
        if konto.klass() != 1 {
            spärrar.push(Spärr::ny(
                Fält::Konto,
                "konto_inte_klass_1",
                "Betalkontot ska vara ett konto i klass 1, t.ex. 1930.",
            ));
            return None;
        }
        if !aktiv {
            spärrar.push(Spärr::ny(
                Fält::Konto,
                "konto_inaktivt",
                format!("Konto {konto} är inaktivt."),
            ));
            return None;
        }
        Some(*konto)
    }
}

/// Ett belopp större än noll i öre, eller `None`.
fn tolka_belopp(text: &str) -> Option<i64> {
    Belopp::tolka(text.trim())
        .map(Belopp::öre)
        .filter(|b| *b > 0)
}

impl Betalning {
    /// Betalningens kontering när fordran eller skulden är bokförd. Kundfaktura: betalkontot
    /// debet, en rest debet på 3740, och 1510 kredit med båda. Leverantörsfaktura: 2440 debet
    /// med det som stängs, betalkontot och resten på 3740 kredit, och kursdifferensen på 3960
    /// kredit (vinst) eller 7960 debet (förlust).
    ///
    /// När fordran eller skulden inte är bokförd (#149) står intäkten och momsen i kredit mot
    /// betalkontot, eller kostnaden och momsen för det som stängs (betalningen med
    /// kursdifferensen) i debet mot betalkontot och kursdifferensen, i proportion; resten mot
    /// öresutjämning bokförs inte. En återbetalning bokförs alltid mot reskontrakontot.
    pub fn kontering(&self) -> Vec<verifikat::Rad> {
        let reskontra = self.reskontra.konto();
        let öresutjämning = Kontonummer::konstant(ÖRESUTJÄMNING);
        let stängt = self.belopp + self.öresutjämning + self.kursdifferens;
        let kurskonto = Kontonummer::konstant(if self.kursdifferens < 0 {
            KURSFÖRLUST
        } else {
            KURSVINST
        });
        if let (Some(o), true) = (&self.obokförd, self.belopp > 0) {
            let mut rader = match self.reskontra {
                Reskontra::Kund => {
                    let mut r = vec![rad(self.konto, self.belopp)];
                    r.extend(o.andel(self.belopp));
                    r
                }
                Reskontra::Leverantör => {
                    let mut r = o.andel(-(self.belopp + self.kursdifferens));
                    r.push(rad(self.konto, -self.belopp));
                    r.push(rad(kurskonto, -self.kursdifferens));
                    r
                }
            };
            rader.retain(|r| r.debet != Belopp::NOLL || r.kredit != Belopp::NOLL);
            return rader;
        }
        let mut rader = match self.reskontra {
            Reskontra::Kund => vec![
                rad(self.konto, self.belopp),
                rad(öresutjämning, self.öresutjämning),
                rad(reskontra, -stängt),
            ],
            Reskontra::Leverantör => vec![
                rad(reskontra, stängt),
                rad(self.konto, -self.belopp),
                rad(öresutjämning, -self.öresutjämning),
                rad(kurskonto, -self.kursdifferens),
            ],
        };
        rader.retain(|r| r.debet != Belopp::NOLL || r.kredit != Belopp::NOLL);
        rader
    }
}

/// En verifikatrad med `belopp` i debet om det är positivt och i kredit om det är negativt.
fn rad(konto: Kontonummer, belopp: i64) -> verifikat::Rad {
    let saldo = Saldo::från_öre(belopp);
    verifikat::Rad {
        konto,
        debet: saldo.debet(),
        kredit: saldo.kredit(),
        text: None,
    }
}
