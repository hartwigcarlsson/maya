//! Konstaterad kundförlust på en utfärdad kundfaktura (#141): panelens formulär, spärrarna och
//! konteringen enligt tabellen över automatkonteringar.
//!
//! Förlusten gäller det som återstår av fakturan och kräver skäl (konkurs, ackord, utmätning
//! utan resultat, eller annat med fritext) och datum. När fordran är bokförd på 1510 bokförs
//! förlusten på 6351 netto, utgående moms per sats minskas, och 1510 krediteras med det som
//! återstår. Beloppet fördelas på utfärdandets netto och momssatser i proportion till deras
//! andel av fakturans total ([`crate::fordelning::fördela`]). När fordran inte är bokförd
//! (kontantmetoden) blir förlusten bara en status, utan verifikat.
//!
//! *Återkalla kundförlust* vänder förlustens verifikat med ett nytt, länkat verifikat, så att en
//! betalning som ändå kommer in kan tas emot. En befarad kundförlust bokförs som ett manuellt
//! verifikat och hanteras inte här.

use std::fmt;

use jiff::civil::Date;

use crate::belopp::{Belopp, Saldo};
use crate::faktura::Momssats;
use crate::fordelning::fördela;
use crate::kontoplan::Kontonummer;
use crate::kundfaktura::KUNDFORDRINGAR;
use crate::rakenskapsar::Räkenskapsår;
use crate::verifikat::{self, Rad, Årsval};

/// Konstaterade kundförluster.
pub const KUNDFÖRLUSTER: u16 = 6351;

/// Skälet till att förlusten är konstaterad.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Skäl {
    Konkurs,
    Ackord,
    UtmätningUtanResultat,
    /// Ett annat skäl, som beskrivs med fritext.
    Annat,
}

impl Skäl {
    pub const ALLA: [Skäl; 4] = [
        Skäl::Konkurs,
        Skäl::Ackord,
        Skäl::UtmätningUtanResultat,
        Skäl::Annat,
    ];

    /// Koden i formuläret och databasen.
    pub fn kod(self) -> &'static str {
        match self {
            Skäl::Konkurs => "konkurs",
            Skäl::Ackord => "ackord",
            Skäl::UtmätningUtanResultat => "utmatning",
            Skäl::Annat => "annat",
        }
    }

    pub fn från_kod(kod: &str) -> Option<Self> {
        Self::ALLA.into_iter().find(|s| s.kod() == kod)
    }

    pub fn namn(self) -> &'static str {
        match self {
            Skäl::Konkurs => "Konkurs",
            Skäl::Ackord => "Ackord",
            Skäl::UtmätningUtanResultat => "Utmätning utan resultat",
            Skäl::Annat => "Annat",
        }
    }

    /// Skälet i text: namnet, och för [`Skäl::Annat`] beskrivningen efter ett kolon.
    pub fn text(self, annat: &str) -> String {
        match self {
            Skäl::Annat => format!("Annat: {}", annat.trim()),
            s => s.namn().to_string(),
        }
    }
}

/// Kundförlusten som användaren skrev den i panelen.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NyKundförlust {
    /// Skälets kod, se [`Skäl::kod`].
    pub skäl: String,
    /// Beskrivningen när skälet är annat.
    pub annat: String,
    /// `ÅÅÅÅ-MM-DD`.
    pub datum: String,
}

/// Det som förlusten prövas mot, läst i samma transaktion som den bokförs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Förutsättningar {
    /// Det som återstår av fakturan i öre.
    pub återstår: i64,
    pub fakturadatum: Date,
    /// En kundförlust som inte är återkallad gäller redan fakturan.
    pub kundförlust: bool,
    /// Raderna i fakturans gällande utfärdandeverifikat, eller `None` när fordran inte är
    /// bokförd.
    pub utfärdandet: Option<Vec<Rad>>,
    pub räkenskapsår: Vec<Räkenskapsår>,
    /// Årets slut när fakturan var obetald vid räkenskapsårets slut och väntar på
    /// T2-verifikatet (#150). `utfärdandet` är då fakturans kontering under fakturametoden, och
    /// en förlust daterad efter årets slut bokförs mot 1510 med den; en förlust daterad senast
    /// årets slut ger bara status.
    pub bokförd_efter: Option<Date>,
}

/// Fälten i panelen, där spärrarna visas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fält {
    /// Fakturan själv: inget återstår, eller en förlust gäller redan.
    Fakturan,
    Skäl,
    Datum,
}

/// Spärrarna vid kundförlust som Systemdokumentationen återger (S2). Varje kod som
/// [`NyKundförlust::kontrollera`] kan ge står här, och ingen annan.
pub const KUNDFÖRLUSTSPÄRRAR: &[(&str, &str)] = &[
    ("inget_aterstar", "Något återstår av fakturan."),
    (
        "redan_kundforlust",
        "Ingen kundförlust gäller redan fakturan; en återkallad förlust räknas inte.",
    ),
    ("skal_saknas", "Skäl krävs."),
    (
        "skal_ogiltigt",
        "Skälet är konkurs, ackord, utmätning utan resultat eller annat.",
    ),
    ("annat_saknas", "Ett annat skäl beskrivs med fritext."),
    ("datum_saknas", "Datum krävs."),
    ("datum_ogiltigt", "Datumet skrivs som ÅÅÅÅ-MM-DD."),
    ("fore_fakturadatum", "Datumet är inte före fakturadatumet."),
    (
        "fore_forsta_aret",
        "När fordran är bokförd är datumet inte före första räkenskapsårets start.",
    ),
    (
        "saknat_rakenskapsar",
        "När fordran är bokförd ligger datumet i ett räkenskapsår som finns, eller i året \
         direkt efter det senaste, som då skapas när förlusten bokförs.",
    ),
];

/// Något som hindrar förlusten, med en kod som panelen visar vid fältet.
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

/// En kundförlust som klarar spärrarna.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kundförlust {
    pub skäl: Skäl,
    /// Beskrivningen för [`Skäl::Annat`], annars tom.
    pub annat: String,
    pub datum: Date,
    /// Det som skrivs bort: det som återstår, i öre.
    pub belopp: i64,
    /// Räkenskapsåret som datumet avgör, eller `None` när fordran inte är bokförd.
    pub räkenskapsår: Option<Årsval>,
    utfärdandet: Option<Vec<Rad>>,
}

/// Förlustverifikatets beskrivning med fakturan som referens:
/// "Kundförlust, kundfaktura 1007, Kund AB".
pub fn beskrivning(faktura: &str) -> String {
    format!("Kundförlust, {faktura}")
}

/// Återkallelsens beskrivning: "Återkallad kundförlust, kundfaktura 1007, Kund AB".
pub fn återkallelsens_beskrivning(faktura: &str) -> String {
    format!("Återkallad kundförlust, {faktura}")
}

impl NyKundförlust {
    /// Panelen när den öppnas: inget skäl valt och dagens datum.
    pub fn förvald(idag: Date) -> Self {
        NyKundförlust {
            datum: idag.to_string(),
            ..NyKundförlust::default()
        }
    }

    /// Prövar spärrarna. Spärrarna kommer i fältens ordning.
    pub fn kontrollera(&self, f: &Förutsättningar) -> Result<Kundförlust, Vec<Spärr>> {
        let mut spärrar = Vec::new();
        if f.återstår <= 0 {
            spärrar.push(Spärr::ny(
                Fält::Fakturan,
                "inget_aterstar",
                "Inget återstår av fakturan att skriva bort.",
            ));
        } else if f.kundförlust {
            spärrar.push(Spärr::ny(
                Fält::Fakturan,
                "redan_kundforlust",
                "Fakturan är redan bokförd som kundförlust.",
            ));
        }

        let skäl = match self.skäl.trim() {
            "" => {
                spärrar.push(Spärr::ny(Fält::Skäl, "skal_saknas", "Välj skäl."));
                None
            }
            kod => match Skäl::från_kod(kod) {
                None => {
                    spärrar.push(Spärr::ny(
                        Fält::Skäl,
                        "skal_ogiltigt",
                        "Välj konkurs, ackord, utmätning utan resultat eller annat.",
                    ));
                    None
                }
                Some(Skäl::Annat) if self.annat.trim().is_empty() => {
                    spärrar.push(Spärr::ny(Fält::Skäl, "annat_saknas", "Beskriv skälet."));
                    None
                }
                Some(s) => Some(s),
            },
        };

        let datum = match self.datum.trim() {
            "" => {
                spärrar.push(Spärr::ny(Fält::Datum, "datum_saknas", "Ange datum."));
                None
            }
            text => match text.parse::<Date>() {
                Ok(d) if d < f.fakturadatum => {
                    spärrar.push(Spärr::ny(
                        Fält::Datum,
                        "fore_fakturadatum",
                        format!("Datumet är före fakturadatumet {}.", f.fakturadatum),
                    ));
                    Some(d)
                }
                Ok(d) => Some(d),
                Err(_) => {
                    spärrar.push(Spärr::ny(
                        Fält::Datum,
                        "datum_ogiltigt",
                        "Skriv datumet som ÅÅÅÅ-MM-DD.",
                    ));
                    None
                }
            },
        };
        // #150: före årets slut är en fordran som väntar på T2 inte bokförd.
        let utfärdandet = match (datum, f.bokförd_efter) {
            (Some(d), Some(slut)) if d <= slut => None,
            (None, Some(_)) => None,
            _ => f.utfärdandet.clone(),
        };
        let räkenskapsår = match (datum, &utfärdandet) {
            (Some(d), Some(_)) => {
                let mut fel = Vec::new();
                let val = verifikat::välj_år(d, &f.räkenskapsår, &mut fel);
                spärrar.extend(
                    fel.into_iter()
                        .map(|s| Spärr::ny(Fält::Datum, s.kod, s.text)),
                );
                val
            }
            _ => None,
        };

        match (skäl, datum) {
            (Some(skäl), Some(datum)) if spärrar.is_empty() => Ok(Kundförlust {
                skäl,
                annat: if skäl == Skäl::Annat {
                    self.annat.trim().to_string()
                } else {
                    String::new()
                },
                datum,
                belopp: f.återstår,
                räkenskapsår,
                utfärdandet,
            }),
            _ => Err(spärrar),
        }
    }
}

impl Kundförlust {
    /// Skälet i text, t.ex. "Konkurs" eller "Annat: dödsboet saknar tillgångar".
    pub fn skälet(&self) -> String {
        self.skäl.text(&self.annat)
    }

    /// Förlustens kontering, se [`kontering`]. Tom när fordran inte är bokförd.
    pub fn kontering(&self) -> Vec<Rad> {
        self.utfärdandet
            .as_deref()
            .map(|u| kontering(self.belopp, u))
            .unwrap_or_default()
    }
}

/// Konteringen av en kundförlust på `belopp` öre när fordran är bokförd med verifikatet
/// `utfärdandet`: 6351 med nettots andel, varje utgående momskonto med sin andel och 1510
/// kredit med beloppet.
pub fn kontering(belopp: i64, utfärdandet: &[Rad]) -> Vec<Rad> {
    let momskonton: Vec<Kontonummer> = Momssats::ALLA
        .iter()
        .filter_map(|s| s.utgående_momskonto())
        .collect();
    // Vikterna: nettot (allt utom 1510 och momsen) först, sedan varje momskonto.
    let mut konton: Vec<Kontonummer> = vec![Kontonummer::konstant(KUNDFÖRLUSTER)];
    let mut vikter: Vec<i64> = vec![0];
    for r in utfärdandet {
        if r.konto.get() == KUNDFORDRINGAR {
            continue;
        }
        let vikt = r.kredit.öre() - r.debet.öre();
        if momskonton.contains(&r.konto) {
            match konton.iter().position(|k| *k == r.konto) {
                Some(i) => vikter[i] += vikt,
                None => {
                    konton.push(r.konto);
                    vikter.push(vikt);
                }
            }
        } else {
            vikter[0] += vikt;
        }
    }
    let mut rader: Vec<Rad> = konton
        .into_iter()
        .zip(fördela(belopp, &vikter))
        .map(|(konto, del)| rad(konto, del))
        .collect();
    rader.push(rad(Kontonummer::konstant(KUNDFORDRINGAR), -belopp));
    rader.retain(|r| r.debet != Belopp::NOLL || r.kredit != Belopp::NOLL);
    rader
}

/// Återkallelsens kontering: förlustverifikatets rader med debet och kredit bytta.
pub fn återkallelse(förlusten: &[Rad]) -> Vec<Rad> {
    förlusten
        .iter()
        .map(|r| Rad {
            konto: r.konto,
            debet: r.kredit,
            kredit: r.debet,
            text: r.text.clone(),
        })
        .collect()
}

/// En verifikatrad med `belopp` i debet om det är positivt och i kredit om det är negativt.
fn rad(konto: Kontonummer, belopp: i64) -> Rad {
    let saldo = Saldo::från_öre(belopp);
    Rad {
        konto,
        debet: saldo.debet(),
        kredit: saldo.kredit(),
        text: None,
    }
}
