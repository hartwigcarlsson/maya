//! Resultat- och balansräkningen: hur bokförda saldon ställs upp i rubriker, INK2-rader och
//! konton enligt INK2-kopplingstabellen, med kolumnerna som urvalet ger.
//!
//! **Resultaträkningen** visar intäkter med plus och kostnader med minus, per månad,
//! ackumulerat eller båda, med delsummorna rörelseresultat, resultat efter finansiella poster,
//! resultat före skatt och årets resultat. Konton på INK2-raderna 3.26/3.27 (899x, årets
//! resultat) räknas inte, så årets resultat är detsamma före och efter att det bokats om.
//!
//! **Balansräkningen** visar ingående balans, förändring och utgående balans. Tillgångar har
//! debet som plus, eget kapital och skulder kredit som plus. Resultatkontona (klass 3–8) står
//! som raden "Årets resultat (ej bokfört på 2099)" under eget kapital, så att sidorna går jämnt
//! ut innan året är stängt; kontrollraden (tillgångar − eget kapital och skulder) är då 0.
//!
//! Konton som tabellen inte täcker står på en egen rad "Utan INK2-rad", så att inget saldo
//! försvinner ur rapporterna.

use std::collections::{BTreeMap, BTreeSet};

use jiff::civil::Date;

use crate::belopp::Saldo;
use crate::kontoplan::{Kontonummer, ink2};
use crate::rakenskapsar::{Räkenskapsår, periodnamn};

/// Vilken rapport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Rapporttyp {
    Resultaträkning,
    Balansräkning,
}

impl Rapporttyp {
    pub const ALLA: [Rapporttyp; 2] = [Rapporttyp::Resultaträkning, Rapporttyp::Balansräkning];

    /// I adresser och filnamn.
    pub fn kod(self) -> &'static str {
        match self {
            Rapporttyp::Resultaträkning => "resultatrakning",
            Rapporttyp::Balansräkning => "balansrakning",
        }
    }

    pub fn från_kod(kod: &str) -> Option<Self> {
        Self::ALLA.into_iter().find(|t| t.kod() == kod)
    }

    pub fn namn(self) -> &'static str {
        match self {
            Rapporttyp::Resultaträkning => "Resultaträkning",
            Rapporttyp::Balansräkning => "Balansräkning",
        }
    }
}

/// Resultaträkningens kolumner: perioden (månaden), ackumulerat eller båda.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Visning {
    Månad,
    Ackumulerat,
    #[default]
    Båda,
}

impl Visning {
    pub const ALLA: [Visning; 3] = [Visning::Månad, Visning::Ackumulerat, Visning::Båda];

    pub fn kod(self) -> &'static str {
        match self {
            Visning::Månad => "manad",
            Visning::Ackumulerat => "ackumulerat",
            Visning::Båda => "bada",
        }
    }

    pub fn från_kod(kod: &str) -> Option<Self> {
        Self::ALLA.into_iter().find(|v| v.kod() == kod)
    }

    pub fn namn(self) -> &'static str {
        match self {
            Visning::Månad => "Månad",
            Visning::Ackumulerat => "Ackumulerat",
            Visning::Båda => "Båda",
        }
    }
}

/// Hur djupt rapporten visas: rubriker, INK2-rader eller konton.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum Nivå {
    Rubriker,
    #[default]
    Ink2Rader,
    Konton,
}

impl Nivå {
    pub const ALLA: [Nivå; 3] = [Nivå::Rubriker, Nivå::Ink2Rader, Nivå::Konton];

    pub fn kod(self) -> &'static str {
        match self {
            Nivå::Rubriker => "rubriker",
            Nivå::Ink2Rader => "ink2",
            Nivå::Konton => "konton",
        }
    }

    pub fn från_kod(kod: &str) -> Option<Self> {
        Self::ALLA.into_iter().find(|n| n.kod() == kod)
    }

    pub fn namn(self) -> &'static str {
        match self {
            Nivå::Rubriker => "Rubriker",
            Nivå::Ink2Rader => "INK2-rader",
            Nivå::Konton => "Konton",
        }
    }
}

/// Utsnittet som användaren valt i verktygsraden. Räkenskapsåret väljs av den som hämtar
/// datat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Urval {
    pub typ: Rapporttyp,
    /// Periodens sista månad: vilket datum som helst i månaden. Hamnar den utanför året gäller
    /// årets första respektive sista månad.
    pub till_och_med: Date,
    pub visning: Visning,
    /// Jämförelse med samma period föregående räkenskapsår.
    pub jämförelse: bool,
    pub nivå: Nivå,
}

impl Urval {
    /// Periodens sista dag i räkenskapsåret `år`: sista dagen i månaden `till_och_med`,
    /// flyttad in i året.
    pub fn till(&self, år: &Räkenskapsår) -> Date {
        self.till_och_med
            .last_of_month()
            .clamp(år.start().last_of_month(), år.slut())
    }

    /// Perioden vars verifikat panelen för ett konto visar, och om den börjar med den
    /// ingående balansen: månaden i resultaträkningen som bara visar månaden, annars året fram
    /// till periodens slut; i balansräkningen från den ingående balansen.
    pub fn kontoperiod(&self, år: &Räkenskapsår) -> (Date, Date, bool) {
        let till = self.till(år);
        match self.typ {
            Rapporttyp::Resultaträkning if self.visning == Visning::Månad => {
                (till.first_of_month().max(år.start()), till, false)
            }
            Rapporttyp::Resultaträkning => (år.start(), till, false),
            Rapporttyp::Balansräkning => (år.start(), till, true),
        }
    }
}

/// Summan av bokförda rader på ett konto under en månad.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rörelse {
    pub konto: Kontonummer,
    /// Vilket datum som helst i månaden.
    pub månad: Date,
    pub saldo: Saldo,
}

/// Ett räkenskapsårs saldon: ingående balans och bokförda rörelser per månad.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Årsdata {
    pub år: Räkenskapsår,
    pub ingående: Vec<(Kontonummer, Saldo)>,
    /// Om den ingående balansen är preliminär (föregående år är inte stängt).
    pub preliminär: bool,
    pub rörelser: Vec<Rörelse>,
}

/// En kolumn i rapporten.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kolumn {
    /// T.ex. "Mars 2026" eller "Utgående balans".
    pub rubrik: String,
    /// T.ex. "ackumulerat" eller "2026-03-31".
    pub underrubrik: String,
    /// Jämförelsekolumnen för föregående år.
    pub föregående: bool,
}

/// Vad en rad är, för hur den visas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Radslag {
    /// Balansräkningens sida (Tillgångar, Eget kapital och skulder). Inga värden.
    Sida,
    /// En rubrik. Med värden när nivån är rubriker, annars en överskrift utan värden.
    Rubrik,
    /// En underrubrik i balansräkningen (t.ex. Materiella anläggningstillgångar), utan värden.
    Underrubrik,
    /// En INK2-rad, eller raden "Utan INK2-rad".
    Ink2Rad,
    Konto,
    /// "Summa …" under en rubrik eller underrubrik.
    Summa,
    /// Resultaträkningens delsummor (rörelseresultat m.fl.).
    Delsumma,
    /// Årets resultat respektive summan av en sida.
    Slutsumma,
    /// "Årets resultat (ej bokfört på 2099)" i balansräkningen.
    ÅretsResultat,
    /// Tillgångar − eget kapital och skulder, som ska vara 0.
    Kontroll,
}

/// En rad i rapporten. `värden` har ett värde per kolumn, i öre med rapportens tecken, och är
/// tomt för överskrifter. `None` är en jämförelse som saknas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rad {
    pub slag: Radslag,
    pub text: String,
    /// INK2-raden, t.ex. "3.1".
    pub ink2: Option<String>,
    pub konto: Option<Kontonummer>,
    pub värden: Vec<Option<i64>>,
}

/// Ett konto i exporten: var det står i rapporten och dess värden.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kontorad {
    /// Rubrikerna ovanför, t.ex. "Tillgångar / Omsättningstillgångar / Kassa och bank".
    pub rubrik: String,
    pub ink2: Option<String>,
    /// INK2-radens benämning, eller raden "Årets resultat (ej bokfört på 2099)".
    pub radtext: String,
    /// `None` för raden årets resultat i balansräkningen.
    pub konto: Option<Kontonummer>,
    pub kontonamn: String,
    pub värden: Vec<Option<i64>>,
}

/// En uppställd rapport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rapport {
    pub urval: Urval,
    pub år: Räkenskapsår,
    pub föregående: Option<Räkenskapsår>,
    /// Periodens första och sista dag.
    pub från: Date,
    pub till: Date,
    /// Den ingående balansen är preliminär.
    pub preliminär: bool,
    pub kolumner: Vec<Kolumn>,
    pub rader: Vec<Rad>,
    /// Varje konto med något värde, i rapportens ordning, för CSV-exporten.
    pub konton: Vec<Kontorad>,
}

/// Texten på raden årets resultat i balansräkningen.
pub const ÅRETS_RESULTAT_EJ_BOKFÖRT: &str = "Årets resultat (ej bokfört på 2099)";
/// Raden för konton som INK2-tabellen inte täcker.
pub const UTAN_INK2_RAD: &str = "Utan INK2-rad";

/// Resultaträkningens rubriker med sina INK2-rader, och delsummorna efter dem.
enum RrDel {
    Rubrik(&'static str, &'static [&'static str]),
    Delsumma(&'static str),
    Slutsumma(&'static str),
}

const RESULTATRÄKNINGEN: &[RrDel] = &[
    RrDel::Rubrik("Rörelsens intäkter", &["3.1", "3.2", "3.3", "3.4"]),
    RrDel::Rubrik(
        "Rörelsens kostnader",
        &["3.5", "3.6", "3.7", "3.8", "3.9", "3.10", "3.11"],
    ),
    RrDel::Delsumma("Rörelseresultat"),
    RrDel::Rubrik(
        "Finansiella poster",
        &["3.12", "3.13", "3.14", "3.15", "3.16", "3.17", "3.18"],
    ),
    RrDel::Delsumma("Resultat efter finansiella poster"),
    RrDel::Rubrik(
        "Bokslutsdispositioner",
        &["3.19", "3.20", "3.21", "3.22", "3.23", "3.24"],
    ),
    RrDel::Delsumma("Resultat före skatt"),
    RrDel::Rubrik("Skatt", &["3.25"]),
    RrDel::Slutsumma("Årets resultat"),
];

/// INK2-raderna för årets resultat (899x), som resultaträkningen inte räknar.
const ÅRETS_RESULTAT_RADER: [&str; 2] = ["3.26", "3.27"];

/// Ett stycke i uppställningen.
enum Nod {
    /// Sida (djup 0), rubrik (1) eller underrubrik (2).
    Grupp {
        djup: u8,
        text: &'static str,
        barn: Vec<Nod>,
    },
    Rad {
        ink2: Option<&'static str>,
        text: &'static str,
        konton: Vec<Kontonummer>,
        årets_resultat: bool,
    },
    Delsumma {
        text: &'static str,
        slut: bool,
        konton: Vec<Kontonummer>,
    },
}

impl Nod {
    fn konton(&self) -> Vec<Kontonummer> {
        match self {
            Nod::Grupp { barn, .. } => barn.iter().flat_map(Nod::konton).collect(),
            Nod::Rad { konton, .. } | Nod::Delsumma { konton, .. } => konton.clone(),
        }
    }
}

/// En kolumns period: vilket år, om den ingående balansen ingår och vilka månader.
#[derive(Clone, Copy)]
struct Kolumnperiod {
    föregående: bool,
    ingående: bool,
    /// Månaderna vars rörelser ingår, första och sista dagen.
    rörelser: Option<(Date, Date)>,
}

impl Rapport {
    /// Ställer upp rapporten för `år` enligt urvalet, med `föregående` år för jämförelsen.
    /// `kontonamn` ger kontonas namn i företagets kontoplan.
    pub fn ställ_upp(
        urval: &Urval,
        år: &Årsdata,
        föregående: Option<&Årsdata>,
        kontonamn: &[(Kontonummer, String)],
    ) -> Rapport {
        let å = år.år;
        let till = urval.till(&å);
        let från = å.start();
        let månadens_start = till.first_of_month().max(från);
        let (föregående_från, föregående_till) = match föregående {
            Some(f) => {
                let t = till
                    .checked_sub(jiff::Span::new().months(12))
                    .unwrap_or(till)
                    .last_of_month()
                    .min(f.år.slut());
                (f.år.start(), t)
            }
            None => (från, till),
        };

        let mut perioder: Vec<(Kolumn, Kolumnperiod)> = Vec::new();
        let saknas = || Kolumn {
            rubrik: "Föregående år".into(),
            underrubrik: "finns inte i Maya".into(),
            föregående: true,
        };
        match urval.typ {
            Rapporttyp::Resultaträkning => {
                let månad = |från: Date, till: Date, föregående: bool| {
                    (
                        Kolumn {
                            rubrik: stor_bokstav(&periodnamn(till)),
                            underrubrik: "perioden".into(),
                            föregående,
                        },
                        Kolumnperiod {
                            föregående,
                            ingående: false,
                            rörelser: Some((från, till)),
                        },
                    )
                };
                let ackumulerat = |från: Date, till: Date, föregående: bool| {
                    (
                        Kolumn {
                            rubrik: periodspann(från, till),
                            underrubrik: "ackumulerat".into(),
                            föregående,
                        },
                        Kolumnperiod {
                            föregående,
                            ingående: false,
                            rörelser: Some((från, till)),
                        },
                    )
                };
                let visa_månad = urval.visning != Visning::Ackumulerat;
                let visa_ack = urval.visning != Visning::Månad;
                if visa_månad {
                    perioder.push(månad(månadens_start, till, false));
                }
                if visa_ack {
                    perioder.push(ackumulerat(från, till, false));
                }
                if urval.jämförelse {
                    let f_månad = föregående_till.first_of_month().max(föregående_från);
                    for (visa, ack) in [(visa_månad, false), (visa_ack, true)] {
                        if !visa {
                            continue;
                        }
                        perioder.push(match (föregående, ack) {
                            (None, _) => (saknas(), saknad_period()),
                            (Some(_), false) => månad(f_månad, föregående_till, true),
                            (Some(_), true) => ackumulerat(föregående_från, föregående_till, true),
                        });
                    }
                }
            }
            Rapporttyp::Balansräkning => {
                let period = |föregående, ingående, rörelser| Kolumnperiod {
                    föregående,
                    ingående,
                    rörelser,
                };
                perioder.push((
                    Kolumn {
                        rubrik: "Ingående balans".into(),
                        underrubrik: if år.preliminär {
                            format!("{från} · preliminär")
                        } else {
                            från.to_string()
                        },
                        föregående: false,
                    },
                    period(false, true, None),
                ));
                perioder.push((
                    Kolumn {
                        rubrik: "Förändring".into(),
                        underrubrik: periodspann(från, till),
                        föregående: false,
                    },
                    period(false, false, Some((från, till))),
                ));
                perioder.push((
                    Kolumn {
                        rubrik: "Utgående balans".into(),
                        underrubrik: till.to_string(),
                        föregående: false,
                    },
                    period(false, true, Some((från, till))),
                ));
                if urval.jämförelse {
                    perioder.push(match föregående {
                        None => (saknas(), saknad_period()),
                        Some(_) => (
                            Kolumn {
                                rubrik: "Föregående år".into(),
                                underrubrik: föregående_till.to_string(),
                                föregående: true,
                            },
                            period(true, true, Some((föregående_från, föregående_till))),
                        ),
                    });
                }
            }
        }

        let (kolumner, perioder): (Vec<_>, Vec<_>) = perioder.into_iter().unzip();
        let saldon = Saldon::räkna(år, föregående, &perioder);
        let uppställning = match urval.typ {
            Rapporttyp::Resultaträkning => resultaträkningen(&saldon),
            Rapporttyp::Balansräkning => balansräkningen(&saldon),
        };
        let namn = |nr: Kontonummer| {
            kontonamn
                .iter()
                .find(|(k, _)| *k == nr)
                .map(|(_, n)| n.clone())
                .unwrap_or_default()
        };
        let mut ut = Utskrivare {
            typ: urval.typ,
            nivå: urval.nivå,
            saldon: &saldon,
            namn: &namn,
            rader: Vec::new(),
            konton: Vec::new(),
        };
        for nod in &uppställning {
            ut.skriv(nod, &mut Vec::new());
        }
        if urval.typ == Rapporttyp::Balansräkning
            && let [tillgångar, skulder] = &uppställning[..]
        {
            let t = saldon.värden(&tillgångar.konton(), Tecken::Balans);
            let s = saldon.värden(&skulder.konton(), Tecken::Balans);
            ut.rader.push(Rad {
                slag: Radslag::Kontroll,
                text: "Tillgångar − eget kapital och skulder".into(),
                ink2: None,
                konto: None,
                värden: t.iter().zip(&s).map(|(t, s)| Some((*t)? - (*s)?)).collect(),
            });
        }
        let Utskrivare { rader, konton, .. } = ut;
        Rapport {
            urval: *urval,
            år: å,
            föregående: föregående.map(|f| f.år),
            från,
            till,
            preliminär: år.preliminär,
            kolumner,
            rader,
            konton,
        }
    }

    /// Om rapporten ska säga att den ingående balansen är preliminär: balansräkningen för ett
    /// år vars föregående år inte är stängt.
    pub fn preliminär_ingående_balans(&self) -> bool {
        self.urval.typ == Rapporttyp::Balansräkning && self.preliminär
    }

    /// Om året är företagets första i Maya, så att det inte finns något att jämföra med.
    pub fn första_året(&self) -> bool {
        self.föregående.is_none()
    }

    /// Perioden som rapporten gäller, t.ex. "januari–mars 2026".
    pub fn period(&self) -> String {
        periodspann(self.från, self.till)
    }

    /// Filnamnet utan ändelse: `<rapport>_<orgnr>_<år>_<ÅÅÅÅMM>`, där år är räkenskapsårets
    /// startår och ÅÅÅÅMM periodens sista månad.
    pub fn filnamn(&self, orgnr: &str) -> String {
        let orgnr: String = orgnr.chars().filter(char::is_ascii_digit).collect();
        format!(
            "{}_{orgnr}_{}_{:04}{:02}",
            self.urval.typ.kod(),
            self.år.start().year(),
            self.till.year(),
            self.till.month()
        )
    }

    /// CSV för kalkylark: UTF-8 med BOM, semikolon, decimalkomma och en rad per konto, med
    /// kolumnerna som rapporten visar.
    pub fn csv(&self) -> String {
        let mut rubriker: Vec<String> = [
            "Rapport",
            "Räkenskapsår",
            "Rubrik",
            "INK2-rad",
            "Radtext",
            "Konto",
            "Kontonamn",
        ]
        .map(String::from)
        .to_vec();
        rubriker.extend(
            self.kolumner
                .iter()
                .map(|k| format!("{} ({})", k.rubrik, k.underrubrik)),
        );
        let rubriker: Vec<&str> = rubriker.iter().map(String::as_str).collect();
        let mut ut = crate::csv::Csv::new(&rubriker);
        let år = format!("{} – {}", self.år.start(), self.år.slut());
        for k in &self.konton {
            let mut fält = vec![
                self.urval.typ.namn().to_string(),
                år.clone(),
                k.rubrik.clone(),
                k.ink2.clone().unwrap_or_default(),
                k.radtext.clone(),
                k.konto.map(|k| k.to_string()).unwrap_or_default(),
                k.kontonamn.clone(),
            ];
            fält.extend(
                k.värden
                    .iter()
                    .map(|v| v.map(crate::csv::belopp).unwrap_or_default()),
            );
            ut.rad(fält);
        }
        ut.text()
    }
}

fn saknad_period() -> Kolumnperiod {
    // Läses aldrig: kolumnen saknar föregående år och blir `None`.
    Kolumnperiod {
        föregående: true,
        ingående: false,
        rörelser: None,
    }
}

fn stor_bokstav(text: &str) -> String {
    let mut tecken = text.chars();
    tecken
        .next()
        .map(|c| c.to_uppercase().chain(tecken).collect())
        .unwrap_or_default()
}

/// "mars 2026", "januari–mars 2026" eller "juli 2025–juni 2026".
pub fn periodspann(från: Date, till: Date) -> String {
    if (från.year(), från.month()) == (till.year(), till.month()) {
        periodnamn(till)
    } else if från.year() == till.year() {
        let månad = periodnamn(från);
        let månad = månad.split(' ').next().unwrap_or_default();
        format!("{månad}–{}", periodnamn(till))
    } else {
        format!("{}–{}", periodnamn(från), periodnamn(till))
    }
}

/// Hur saldot vänds för att visas.
#[derive(Clone, Copy)]
enum Tecken {
    /// Resultaträkningen och resultatkontona: kredit som plus.
    Resultat,
    /// Balansräkningen: tillgångar med debet som plus, eget kapital och skulder med kredit.
    Balans,
}

/// Varje kontos saldo per kolumn (debet positivt), eller `None` där jämförelsen saknas.
struct Saldon {
    konton: BTreeMap<Kontonummer, Vec<Option<i64>>>,
    /// Årets rörelser per konto, för konton vars INK2-rad beror på tecknet.
    årets: BTreeMap<Kontonummer, i64>,
    kolumner: usize,
    har_föregående: Vec<bool>,
}

impl Saldon {
    fn räkna(
        år: &Årsdata, föregående: Option<&Årsdata>, perioder: &[Kolumnperiod]
    ) -> Saldon {
        let alla: BTreeSet<Kontonummer> = [Some(år), föregående]
            .into_iter()
            .flatten()
            .flat_map(|å| {
                å.ingående
                    .iter()
                    .map(|(k, _)| *k)
                    .chain(å.rörelser.iter().map(|r| r.konto))
            })
            .collect();
        let har_föregående: Vec<bool> = perioder
            .iter()
            .map(|p| !p.föregående || föregående.is_some())
            .collect();
        let konton = alla
            .into_iter()
            .map(|konto| {
                let värden = perioder
                    .iter()
                    .map(|p| {
                        let data = if p.föregående { föregående? } else { år };
                        let ib = if p.ingående {
                            data.ingående
                                .iter()
                                .filter(|(k, _)| *k == konto)
                                .map(|(_, s)| s.öre())
                                .sum()
                        } else {
                            0
                        };
                        let rörelser: i64 = match p.rörelser {
                            None => 0,
                            Some((från, till)) => data
                                .rörelser
                                .iter()
                                .filter(|r| {
                                    let m = r.månad.first_of_month();
                                    r.konto == konto
                                        && m >= från.first_of_month()
                                        && m <= till.first_of_month()
                                })
                                .map(|r| r.saldo.öre())
                                .sum(),
                        };
                        Some(ib + rörelser)
                    })
                    .collect();
                (konto, värden)
            })
            .collect();
        let mut årets = BTreeMap::new();
        for r in &år.rörelser {
            *årets.entry(r.konto).or_insert(0) += r.saldo.öre();
        }
        Saldon {
            konton,
            årets,
            kolumner: perioder.len(),
            har_föregående,
        }
    }

    /// Summan av kontonas värden per kolumn, vända för att visas.
    fn värden(&self, konton: &[Kontonummer], tecken: Tecken) -> Vec<Option<i64>> {
        (0..self.kolumner)
            .map(|i| {
                if !self.har_föregående[i] {
                    return None;
                }
                Some(
                    konton
                        .iter()
                        .filter_map(|k| Some((k, self.konton.get(k)?[i]?)))
                        .map(|(k, öre)| match tecken {
                            Tecken::Resultat => -öre,
                            Tecken::Balans if k.klass() == 1 => öre,
                            Tecken::Balans => -öre,
                        })
                        .sum(),
                )
            })
            .collect()
    }

    fn noll(&self, konton: &[Kontonummer]) -> bool {
        konton.iter().all(|k| {
            self.konton
                .get(k)
                .is_none_or(|v| v.iter().all(|x| x.unwrap_or(0) == 0))
        })
    }
}

/// INK2-raden som kontot står på i rapporten, eller `None` om tabellen inte täcker det.
/// Konton vars rad beror på tecknet (t.ex. 8810) står där årets saldo hamnar; kredit räknas
/// som plus.
fn ink2_rad(konto: Kontonummer, saldon: &Saldon) -> Option<&'static ink2::Ink2Rad> {
    let koppling = ink2::koppling(konto)?;
    let saldo = saldon.årets.get(&konto).copied().unwrap_or(0);
    Some(koppling.fält(saldo <= 0).rad())
}

fn resultaträkningen(saldon: &Saldon) -> Vec<Nod> {
    let resultatkonton: Vec<Kontonummer> = saldon
        .konton
        .keys()
        .copied()
        .filter(|k| !k.typ().balanskonto())
        .filter(|k| {
            ink2::koppling(*k).is_none_or(|kp| {
                !ÅRETS_RESULTAT_RADER.contains(&kp.fält(true).rad().rad())
                    && !ÅRETS_RESULTAT_RADER.contains(&kp.fält(false).rad().rad())
            })
        })
        .collect();
    let mut hittills: Vec<Kontonummer> = Vec::new();
    let mut noder = Vec::new();
    for del in RESULTATRÄKNINGEN {
        match del {
            RrDel::Rubrik(text, rader) => {
                let mut barn: Vec<Nod> = ink2::rader()
                    .iter()
                    .filter(|r| rader.contains(&r.rad()))
                    .map(|r| Nod::Rad {
                        ink2: Some(r.rad()),
                        text: r.benämning(),
                        konton: resultatkonton
                            .iter()
                            .copied()
                            .filter(|k| {
                                ink2_rad(*k, saldon).map(ink2::Ink2Rad::rad) == Some(r.rad())
                            })
                            .collect(),
                        årets_resultat: false,
                    })
                    .collect();
                // Konton utan INK2-rad: intäkter, rörelsekostnader eller finansiella poster
                // efter kontoklassen.
                let utan_rad: Vec<Kontonummer> = resultatkonton
                    .iter()
                    .copied()
                    .filter(|k| ink2_rad(*k, saldon).is_none())
                    .filter(|k| match k.klass() {
                        3 => *text == "Rörelsens intäkter",
                        4..=7 => *text == "Rörelsens kostnader",
                        _ => *text == "Finansiella poster",
                    })
                    .collect();
                if !utan_rad.is_empty() {
                    barn.push(Nod::Rad {
                        ink2: None,
                        text: UTAN_INK2_RAD,
                        konton: utan_rad,
                        årets_resultat: false,
                    });
                }
                let nod = Nod::Grupp {
                    djup: 1,
                    text,
                    barn,
                };
                hittills.extend(nod.konton());
                noder.push(nod);
            }
            RrDel::Delsumma(text) | RrDel::Slutsumma(text) => noder.push(Nod::Delsumma {
                text,
                slut: matches!(del, RrDel::Slutsumma(_)),
                konton: hittills.clone(),
            }),
        }
    }
    noder
}

fn balansräkningen(saldon: &Saldon) -> Vec<Nod> {
    let balanskonton: Vec<Kontonummer> = saldon
        .konton
        .keys()
        .copied()
        .filter(|k| k.typ().balanskonto())
        .collect();
    let resultatkonton: Vec<Kontonummer> = saldon
        .konton
        .keys()
        .copied()
        .filter(|k| !k.typ().balanskonto())
        .collect();
    let mut sidor: Vec<Nod> = Vec::new();
    for rad in ink2::rader()
        .iter()
        .filter(|r| r.rubriker().first() == Some(&"Balansräkning"))
    {
        let rubriker = &rad.rubriker()[1..];
        let nod = Nod::Rad {
            ink2: Some(rad.rad()),
            text: rad.benämning(),
            konton: balanskonton
                .iter()
                .copied()
                .filter(|k| ink2_rad(*k, saldon).map(ink2::Ink2Rad::rad) == Some(rad.rad()))
                .collect(),
            årets_resultat: false,
        };
        let mut barn = &mut sidor;
        for (djup, rubrik) in rubriker.iter().enumerate() {
            let i = match barn
                .iter()
                .position(|n| matches!(n, Nod::Grupp { text, .. } if text == rubrik))
            {
                Some(i) => i,
                None => {
                    barn.push(Nod::Grupp {
                        djup: djup as u8,
                        text: rubrik,
                        barn: Vec::new(),
                    });
                    barn.len() - 1
                }
            };
            let Nod::Grupp { barn: under, .. } = &mut barn[i] else {
                unreachable!("bara grupper har rubriker")
            };
            barn = under;
        }
        barn.push(nod);
        if rad.rad() == "2.28" {
            barn.push(Nod::Rad {
                ink2: None,
                text: ÅRETS_RESULTAT_EJ_BOKFÖRT,
                konton: resultatkonton.clone(),
                årets_resultat: true,
            });
        }
    }
    // Balanskonton utan INK2-rad, sist på sin sida.
    for (i, sida) in sidor.iter_mut().enumerate() {
        let klass = i as u8 + 1;
        let utan_rad: Vec<Kontonummer> = balanskonton
            .iter()
            .copied()
            .filter(|k| k.klass() == klass && ink2_rad(*k, saldon).is_none())
            .collect();
        if !utan_rad.is_empty()
            && let Nod::Grupp { barn, .. } = sida
        {
            barn.push(Nod::Grupp {
                djup: 1,
                text: UTAN_INK2_RAD,
                barn: vec![Nod::Rad {
                    ink2: None,
                    text: UTAN_INK2_RAD,
                    konton: utan_rad,
                    årets_resultat: false,
                }],
            });
        }
    }
    sidor
}

struct Utskrivare<'a> {
    typ: Rapporttyp,
    nivå: Nivå,
    saldon: &'a Saldon,
    namn: &'a dyn Fn(Kontonummer) -> String,
    rader: Vec<Rad>,
    konton: Vec<Kontorad>,
}

impl Utskrivare<'_> {
    fn tecken(&self) -> Tecken {
        match self.typ {
            Rapporttyp::Resultaträkning => Tecken::Resultat,
            Rapporttyp::Balansräkning => Tecken::Balans,
        }
    }

    fn rad(&mut self, slag: Radslag, text: &str, värden: Vec<Option<i64>>) {
        self.rader.push(Rad {
            slag,
            text: text.to_string(),
            ink2: None,
            konto: None,
            värden,
        });
    }

    /// Skriver noden och det under den som nivån visar. `väg` är rubrikerna ovanför.
    fn skriv(&mut self, nod: &Nod, väg: &mut Vec<&'static str>) {
        let tecken = self.tecken();
        match nod {
            Nod::Delsumma { text, slut, konton } => {
                let slag = if *slut {
                    Radslag::Slutsumma
                } else {
                    Radslag::Delsumma
                };
                let värden = self.saldon.värden(konton, tecken);
                self.rad(slag, text, värden);
            }
            Nod::Grupp { djup, text, barn } => {
                let konton = nod.konton();
                if *djup > 0 && self.saldon.noll(&konton) {
                    return;
                }
                let värden = self.saldon.värden(&konton, tecken);
                väg.push(text);
                let summa = format!("Summa {}", utan_parentes(text).to_lowercase());
                match djup {
                    0 => {
                        self.rad(Radslag::Sida, text, Vec::new());
                        for b in barn {
                            self.skriv(b, väg);
                        }
                        self.rad(Radslag::Slutsumma, &summa, värden);
                    }
                    1 if self.nivå == Nivå::Rubriker => {
                        self.rad(Radslag::Rubrik, text, värden);
                        // Raderna under visas inte, men exporten får kontona.
                        let synliga = std::mem::take(&mut self.rader);
                        for b in barn {
                            self.skriv(b, väg);
                        }
                        self.rader = synliga;
                    }
                    _ => {
                        let slag = if *djup == 1 {
                            Radslag::Rubrik
                        } else {
                            Radslag::Underrubrik
                        };
                        self.rad(slag, text, Vec::new());
                        for b in barn {
                            self.skriv(b, väg);
                        }
                        self.rad(Radslag::Summa, &summa, värden);
                    }
                }
                väg.pop();
            }
            Nod::Rad {
                ink2,
                text,
                konton,
                årets_resultat,
            } => {
                let rad_tecken = if *årets_resultat {
                    Tecken::Resultat
                } else {
                    tecken
                };
                // Årets resultat visas bara när det inte är ombokat, alltså när summan inte är
                // noll; de enskilda resultatkontona har saldon ändå.
                let tom = if *årets_resultat {
                    self.saldon
                        .värden(konton, rad_tecken)
                        .iter()
                        .all(|v| v.unwrap_or(0) == 0)
                } else {
                    self.saldon.noll(konton)
                };
                if tom {
                    return;
                }
                let rubrik = väg.join(" / ");
                self.rader.push(Rad {
                    slag: if *årets_resultat {
                        Radslag::ÅretsResultat
                    } else {
                        Radslag::Ink2Rad
                    },
                    text: text.to_string(),
                    ink2: ink2.map(String::from),
                    konto: None,
                    värden: self.saldon.värden(konton, rad_tecken),
                });
                if *årets_resultat {
                    self.konton.push(Kontorad {
                        rubrik,
                        ink2: None,
                        radtext: text.to_string(),
                        konto: None,
                        kontonamn: String::new(),
                        värden: self.saldon.värden(konton, rad_tecken),
                    });
                    return;
                }
                for &k in konton {
                    if self.saldon.noll(&[k]) {
                        continue;
                    }
                    let värden = self.saldon.värden(&[k], rad_tecken);
                    let namn = (self.namn)(k);
                    if self.nivå == Nivå::Konton {
                        self.rader.push(Rad {
                            slag: Radslag::Konto,
                            text: namn.clone(),
                            ink2: None,
                            konto: Some(k),
                            värden: värden.clone(),
                        });
                    }
                    self.konton.push(Kontorad {
                        rubrik: rubrik.clone(),
                        ink2: ink2.map(String::from),
                        radtext: text.to_string(),
                        konto: Some(k),
                        kontonamn: namn,
                        värden,
                    });
                }
            }
        }
    }
}

/// "Kortfristiga skulder (förfaller …)" → "Kortfristiga skulder".
fn utan_parentes(text: &str) -> &str {
    text.split(" (").next().unwrap_or(text).trim()
}
