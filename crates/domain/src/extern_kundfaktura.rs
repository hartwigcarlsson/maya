//! En extern kundfaktura (#143): en kundfaktura som har skapats i ett annat system och
//! registreras i Maya med sin PDF som underlag. Användaren skriver av kund, fakturanummer,
//! fakturadatum, förfallodag och belopp per momssats och intäktskonto; formuläret i
//! webbläsaren och servern prövar samma spärrar ([`REGISTRERINGSSPÄRRAR`]).
//!
//! Momsen räknas som på Mayas egna fakturor, per momssats på summan av radernas netto
//! ([`crate::faktura::momsrader`]), och registreringen konteras som ett utfärdande (1510 mot
//! intäkt och utgående moms). En extern faktura ingår inte i fakturaserien; numret är det andra
//! systemets och får finnas en gång bland företagets externa fakturor. En *extern kreditfaktura*
//! har negativa belopp och en hänvisning till en registrerad extern faktura, som den krediterar
//! högst det som finns kvar av.
//!
//! En *ny kund* kan läggas upp direkt på utkastet ("+ Ny kund…", #153), som på Mayas egna
//! fakturor (#137): den prövas med kundregistrets regler och blir verklig när fakturan
//! registreras.

use std::fmt;

use jiff::ToSpan;
use jiff::civil::Date;

use crate::belopp;
use crate::faktura::{self, Momsrad, Momssats};
use crate::foretag::Momsperiod;
use crate::kontoplan::Kontonummer;
use crate::kund::{self, GiltigKund, NyKund};
use crate::kundfaktura::{Fakturakund, NY, fakturakund, fordringens_kontering, intäktskontot};
use crate::rakenskapsar::Räkenskapsår;
use crate::verifikat::{self, Årsval};

/// En rad som användaren skrev av den: momssatsen, intäktskontot och beloppet exklusive moms.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NyExternRad {
    pub momssats: String,
    pub konto: String,
    /// Kronor exklusive moms; negativt på en kreditfaktura.
    pub belopp: String,
}

impl Default for NyExternRad {
    fn default() -> Self {
        NyExternRad {
            momssats: "25".into(),
            konto: String::new(),
            belopp: String::new(),
        }
    }
}

impl NyExternRad {
    /// En rad utan konto och belopp räknas inte.
    pub fn tom(&self) -> bool {
        self.konto.trim().is_empty() && self.belopp.trim().is_empty()
    }
}

/// Fakturan som användaren skrev av den. Kunden är kundens id i registret som text, eller
/// [`NY`] för en ny kund från utkastet.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NyExternFaktura {
    pub kund: String,
    /// Det andra systemets fakturanummer.
    pub fakturanummer: String,
    pub fakturadatum: String,
    /// Tom: fakturadatumet plus kundens betalningsvillkor.
    pub förfallodag: String,
    pub rader: Vec<NyExternRad>,
    /// Den nya kunden, när `kund` är [`NY`] (#153).
    #[serde(default)]
    pub ny_kund: NyKund,
}

/// Originalet som en extern kreditfaktura krediterar, läst ur databasen.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Krediterar {
    pub faktura: i64,
    pub fakturanummer: String,
    /// Kunden som den registrerades; kreditfakturan går till samma kund.
    pub kund: Fakturakund,
    /// Det som finns kvar att kreditera: originalets total minus de andra kreditfakturorna.
    pub kvar: i64,
}

impl Krediterar {
    /// "Krediterar faktura F-17".
    pub fn text(&self) -> String {
        format!("Krediterar faktura {}", self.fakturanummer)
    }
}

/// Det som spärrarna prövas mot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Förutsättningar {
    /// De aktiva kunderna i kundregistret.
    pub kunder: Vec<Fakturakund>,
    pub konton: Vec<(Kontonummer, bool)>,
    pub räkenskapsår: Vec<Räkenskapsår>,
    pub momsperioder: Vec<(Date, Momsperiod)>,
    /// Numren på företagets andra externa fakturor som inte är utkast.
    pub upptagna: Vec<String>,
    /// Originalet när fakturan är en kreditfaktura.
    pub krediterar: Option<Krediterar>,
    /// Det som redan har betalats av fakturan, i öre: för en upplåst faktura (Rätta
    /// registrering), som inte får en total under det.
    pub betalt: i64,
}

impl Förutsättningar {
    /// Om företaget är momsregistrerat på dagen.
    pub fn momsregistrerat(&self, dag: Date) -> bool {
        self.momsperioder
            .iter()
            .rev()
            .find(|(från, _)| *från <= dag)
            .or(self.momsperioder.first())
            .is_some_and(|(_, period)| *period != Momsperiod::EjMomsregistrerad)
    }
}

/// Ett fält på en rad.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Radfält {
    Momssats,
    Konto,
    Belopp,
}

/// Fältet som en spärr gäller. Raderna räknas från 0 i formulärets ordning, tomma rader
/// medräknade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Fält {
    Kund,
    /// Ett fält i den nya kundens streckade kort (#153).
    NyKund(kund::Fält),
    Fakturanummer,
    Fakturadatum,
    Förfallodag,
    Rader,
    Rad(usize, Radfält),
    Summa,
}

/// Spärrarna vid registrering som Systemdokumentationen återger (S2): koden och regeln.
pub const REGISTRERINGSSPÄRRAR: &[(&str, &str)] = &[
    (
        "kund_saknas",
        "Fakturan har en aktiv kund ur kundregistret.",
    ),
    (
        "ny_kund",
        "En ny kund från utkastet klarar kundregistrets regler: den har ett namn, och \
         organisations- eller personnummer, e-post och betalningsvillkor är rätt ifyllda.",
    ),
    (
        "fakturanummer_saknas",
        "Fakturan har det andra systemets fakturanummer.",
    ),
    (
        "dubblett",
        "Fakturanumret finns inte redan bland företagets externa fakturor.",
    ),
    ("fakturadatum_saknas", "Fakturadatum krävs."),
    (
        "fakturadatum_ogiltigt",
        "Fakturadatumet skrivs som ÅÅÅÅ-MM-DD.",
    ),
    (
        "fore_forsta_aret",
        "Fakturadatumet är inte före första räkenskapsårets start.",
    ),
    (
        "saknat_rakenskapsar",
        "Fakturadatumet ligger i ett räkenskapsår som finns, eller i året direkt efter det \
         senaste, som då skapas när fakturan registreras.",
    ),
    (
        "forfallodag_ogiltig",
        "Förfallodagen skrivs som ÅÅÅÅ-MM-DD.",
    ),
    (
        "forfallodag_fore_fakturadatum",
        "Förfallodagen är inte före fakturadatumet.",
    ),
    ("rader_saknas", "Fakturan har minst en rad."),
    (
        "rad_belopp_saknas",
        "Varje rad har ett belopp exklusive moms.",
    ),
    (
        "rad_belopp_ogiltigt",
        "Beloppet är i kronor med högst två decimaler, negativt med minustecken.",
    ),
    ("rad_momssats_ogiltig", "Momssatsen är 25, 12, 6 eller 0 %."),
    (
        "momssats_ej_momsregistrerad",
        "Ett företag som inte är momsregistrerat på fakturadatumet har 0 % på varje rad.",
    ),
    ("rad_konto_saknas", "Varje rad har ett intäktskonto."),
    ("konto_okant", "Kontot finns i företagets kontoplan."),
    ("konto_inaktivt", "Kontot är aktivt."),
    ("konto_inte_intakt", "Intäktskontot är ett konto i klass 3."),
    ("summa_inte_positiv", "Fakturans summa är större än noll."),
    (
        "summa_inte_negativ",
        "En kreditfakturas summa är mindre än noll.",
    ),
    (
        "kredit_overstiger",
        "En extern kreditfaktura krediterar högst det som finns kvar att kreditera på \
         originalet: dess total minus de kreditfakturor som redan är registrerade.",
    ),
    (
        "under_betalt",
        "En upplåst faktura får inte en total under det som redan har betalats.",
    ),
];

/// Något som hindrar registreringen, med en kod som formuläret visar vid fältet.
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

/// En rad som klarar spärrarna.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Externrad {
    pub momssats: Momssats,
    pub konto: Kontonummer,
    /// I öre, exklusive moms.
    pub netto: i64,
}

/// En faktura som klarar alla spärrar och kan registreras.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registrerbar {
    /// Kunden. En ny kund har id 0 tills `core` lägger den i registret.
    pub kund: Fakturakund,
    /// Den nya kunden från utkastet, som blir verklig när fakturan registreras (#153).
    pub ny_kund: Option<GiltigKund>,
    pub fakturanummer: String,
    pub fakturadatum: Date,
    pub förfallodag: Date,
    pub rader: Vec<Externrad>,
    pub momsrader: Vec<Momsrad>,
    pub netto: i64,
    pub moms: i64,
    pub total: i64,
    /// Räkenskapsåret som fakturadatumet avgör.
    pub räkenskapsår: Årsval,
    pub krediterar: Option<Krediterar>,
}

impl Registrerbar {
    /// Verifikatets beskrivning med motparten (VF2): "Extern kundfaktura F-17, Kund AB", eller
    /// "Extern kreditfaktura K-3, Kund AB, krediterar faktura F-17".
    pub fn beskrivning(&self) -> String {
        match &self.krediterar {
            Some(k) => format!(
                "Extern kreditfaktura {}, {}, krediterar faktura {}",
                self.fakturanummer, self.kund.namn, k.fakturanummer
            ),
            None => format!(
                "Extern kundfaktura {}, {}",
                self.fakturanummer, self.kund.namn
            ),
        }
    }

    /// Registreringens kontering under fakturametoden, som ett utfärdande: 1510 debet med
    /// totalen, intäktskontona kredit med netto och utgående moms kredit per momssats; en
    /// kreditfaktura åt andra hållet.
    pub fn kontering(&self) -> Vec<verifikat::Rad> {
        fordringens_kontering(
            self.total,
            self.rader.iter().map(|r| (r.konto, r.netto)),
            &self.momsrader,
        )
    }
}

/// Summorna i formuläret som de är nu, med de rader som går att räkna.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Beräkning {
    /// Varje rads netto, `None` för en tom rad eller en rad som inte går att tolka.
    pub rader: Vec<Option<i64>>,
    pub momsrader: Vec<Momsrad>,
    pub netto: i64,
    pub moms: i64,
    pub total: i64,
}

/// Ett sparat utkast. Det som inte är ifyllt saknas.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Utkast {
    pub kund: Option<i64>,
    /// En ny kund från utkastet, i stället för en kund ur registret (#153).
    pub ny_kund: Option<GiltigKund>,
    pub fakturanummer: String,
    pub fakturadatum: Option<Date>,
    pub förfallodag: Option<Date>,
    pub rader: Vec<Utkastrad>,
}

/// En rad i ett sparat utkast.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Utkastrad {
    pub momssats: Momssats,
    pub konto: Option<Kontonummer>,
    /// I öre.
    pub belopp: Option<i64>,
}

impl Utkast {
    /// Utkastet som formulär.
    pub fn som_formulär(&self) -> NyExternFaktura {
        NyExternFaktura {
            kund: match (&self.ny_kund, self.kund) {
                (Some(_), _) => NY.to_string(),
                (None, k) => k.map(|k| k.to_string()).unwrap_or_default(),
            },
            ny_kund: self
                .ny_kund
                .as_ref()
                .map(GiltigKund::som_formulär)
                .unwrap_or_default(),
            fakturanummer: self.fakturanummer.clone(),
            fakturadatum: self.fakturadatum.map(|d| d.to_string()).unwrap_or_default(),
            förfallodag: self.förfallodag.map(|d| d.to_string()).unwrap_or_default(),
            rader: self
                .rader
                .iter()
                .map(|r| NyExternRad {
                    momssats: r.momssats.procent().to_string(),
                    konto: r.konto.map(|k| k.to_string()).unwrap_or_default(),
                    belopp: r.belopp.map(belopp::formulärtext).unwrap_or_default(),
                })
                .collect(),
        }
    }
}

const BELOPP_OGILTIGT: &str =
    "Ange beloppet i kronor exklusive moms med högst två decimaler, negativt med minustecken.";
const MOMSSATS_OGILTIG: &str = "Välj 25, 12, 6 eller 0 %.";

impl NyExternFaktura {
    /// Radernas netto, momsen per sats och totalen, med de rader som går att räkna.
    pub fn beräkna(&self) -> Beräkning {
        let netton: Vec<Option<(Momssats, i64)>> = self
            .rader
            .iter()
            .map(|r| {
                if r.tom() {
                    return None;
                }
                Some((
                    Momssats::tolka(&r.momssats)?,
                    belopp::tolka_med_tecken(&r.belopp)?,
                ))
            })
            .collect();
        let momsrader = faktura::momsrader(netton.iter().flatten().copied());
        let netto = momsrader.iter().map(|m| m.underlag).sum();
        let moms = momsrader.iter().map(|m| m.moms).sum();
        Beräkning {
            rader: netton.iter().map(|n| n.map(|(_, netto)| netto)).collect(),
            momsrader,
            netto,
            moms,
            total: netto + moms,
        }
    }

    /// Prövar spärrarna vid registrering, i fältens ordning.
    pub fn kontrollera(&self, f: &Förutsättningar) -> Result<Registrerbar, Vec<Spärr>> {
        let mut spärrar = Vec::new();
        let (kund, ny_kund) = match self.kontrollera_kund(f, &mut spärrar) {
            Some((k, ny)) => (Some(k), ny),
            None => (None, None),
        };
        let fakturanummer = self.fakturanummer.trim();
        if fakturanummer.is_empty() {
            spärrar.push(Spärr::ny(
                Fält::Fakturanummer,
                "fakturanummer_saknas",
                "Ange fakturanumret från det andra systemet.",
            ));
        } else if f
            .upptagna
            .iter()
            .any(|n| n.trim().to_lowercase() == fakturanummer.to_lowercase())
        {
            spärrar.push(Spärr::ny(
                Fält::Fakturanummer,
                "dubblett",
                format!(
                    "Faktura {fakturanummer} finns redan bland företagets externa fakturor. \
                     Samma faktura registreras inte två gånger."
                ),
            ));
        }
        let fakturadatum = match self.fakturadatum.trim() {
            "" => {
                spärrar.push(Spärr::ny(
                    Fält::Fakturadatum,
                    "fakturadatum_saknas",
                    "Ange fakturadatum.",
                ));
                None
            }
            text => datum(
                text,
                Fält::Fakturadatum,
                "fakturadatum_ogiltigt",
                &mut spärrar,
            ),
        };
        let årsval = fakturadatum.and_then(|d| {
            let mut fel = Vec::new();
            let val = verifikat::välj_år(d, &f.räkenskapsår, &mut fel);
            spärrar.extend(fel.into_iter().map(|s| {
                Spärr::ny(
                    Fält::Fakturadatum,
                    s.kod,
                    s.text.replacen("Datumet", "Fakturadatumet", 1).replacen(
                        "datumet",
                        "fakturadatumet",
                        1,
                    ),
                )
            }));
            val
        });
        let förfallodag = match self.förfallodag.trim() {
            "" => fakturadatum.and_then(|d| {
                d.checked_add(i64::from(kund.as_ref()?.betalningsvillkor).days())
                    .ok()
            }),
            text => datum(text, Fält::Förfallodag, "forfallodag_ogiltig", &mut spärrar),
        };
        let förfallodag = match (förfallodag, fakturadatum) {
            (Some(dag), Some(fd)) if dag < fd => {
                spärrar.push(Spärr::ny(
                    Fält::Förfallodag,
                    "forfallodag_fore_fakturadatum",
                    "Förfallodagen kan inte vara före fakturadatumet.",
                ));
                None
            }
            (dag, _) => dag,
        };
        let momsregistrerat = fakturadatum.is_none_or(|d| f.momsregistrerat(d));

        let ifyllda: Vec<(usize, &NyExternRad)> = self
            .rader
            .iter()
            .enumerate()
            .filter(|(_, r)| !r.tom())
            .collect();
        if ifyllda.is_empty() {
            spärrar.push(Spärr::ny(
                Fält::Rader,
                "rader_saknas",
                "Lägg till minst en rad.",
            ));
        }
        let antal = ifyllda.len();
        let mut rader = Vec::new();
        for (i, r) in ifyllda {
            let mut fel =
                |fält, kod, text: String| spärrar.push(Spärr::ny(Fält::Rad(i, fält), kod, text));
            let momssats = match Momssats::tolka(&r.momssats) {
                None => {
                    fel(
                        Radfält::Momssats,
                        "rad_momssats_ogiltig",
                        MOMSSATS_OGILTIG.into(),
                    );
                    None
                }
                Some(s) if s != Momssats::Noll && !momsregistrerat => {
                    fel(
                        Radfält::Momssats,
                        "momssats_ej_momsregistrerad",
                        "Företaget är inte momsregistrerat på fakturadatumet: välj 0 %.".into(),
                    );
                    None
                }
                s => s,
            };
            let konto = match intäktskontot(&r.konto, &f.konton) {
                Ok(k) => Some(k),
                Err((kod, text)) => {
                    fel(Radfält::Konto, kod, text);
                    None
                }
            };
            let netto = match (r.belopp.trim(), belopp::tolka_med_tecken(&r.belopp)) {
                ("", _) => {
                    fel(
                        Radfält::Belopp,
                        "rad_belopp_saknas",
                        "Ange beloppet exklusive moms.".into(),
                    );
                    None
                }
                (_, None) => {
                    fel(
                        Radfält::Belopp,
                        "rad_belopp_ogiltigt",
                        BELOPP_OGILTIGT.into(),
                    );
                    None
                }
                (_, n) => n,
            };
            if let (Some(momssats), Some(konto), Some(netto)) = (momssats, konto, netto) {
                rader.push(Externrad {
                    momssats,
                    konto,
                    netto,
                });
            }
        }
        let momsrader = faktura::momsrader(rader.iter().map(|r| (r.momssats, r.netto)));
        let netto: i64 = momsrader.iter().map(|m| m.underlag).sum();
        let moms: i64 = momsrader.iter().map(|m| m.moms).sum();
        let total = netto + moms;
        if !rader.is_empty() && rader.len() == antal {
            match &f.krediterar {
                None if total <= 0 => spärrar.push(Spärr::ny(
                    Fält::Summa,
                    "summa_inte_positiv",
                    "Fakturans summa måste vara större än noll. En extern kreditfaktura \
                     registreras från originalets dokument.",
                )),
                Some(_) if total >= 0 => spärrar.push(Spärr::ny(
                    Fält::Summa,
                    "summa_inte_negativ",
                    "En kreditfakturas summa måste vara mindre än noll: ange beloppen med \
                     minustecken.",
                )),
                Some(k) if -total > k.kvar => spärrar.push(Spärr::ny(
                    Fält::Summa,
                    "kredit_overstiger",
                    format!(
                        "Kreditfakturan krediterar mer än som finns kvar att kreditera på \
                         faktura {} ({} kr).",
                        k.fakturanummer,
                        crate::belopp::Saldo::från_öre(k.kvar)
                    ),
                )),
                _ => {}
            }
            if f.betalt > 0 && total < f.betalt {
                spärrar.push(Spärr::ny(
                    Fält::Summa,
                    "under_betalt",
                    format!(
                        "Fakturan är redan betald med {} kr. Totalen kan inte bli lägre än det.",
                        crate::belopp::Saldo::från_öre(f.betalt)
                    ),
                ));
            }
        }

        match (kund, fakturadatum, årsval, förfallodag) {
            (Some(kund), Some(fakturadatum), Some(räkenskapsår), Some(förfallodag))
                if spärrar.is_empty() =>
            {
                Ok(Registrerbar {
                    kund,
                    ny_kund,
                    fakturanummer: fakturanummer.to_string(),
                    fakturadatum,
                    förfallodag,
                    rader,
                    momsrader,
                    netto,
                    moms,
                    total,
                    räkenskapsår,
                    krediterar: f.krediterar.clone(),
                })
            }
            _ => Err(spärrar),
        }
    }

    /// Kunden som fakturan går till: originalets för en kreditfaktura, den valda i registret,
    /// eller den nya kunden när den klarar kundregistrets regler.
    pub fn kunden(&self, f: &Förutsättningar) -> Option<Fakturakund> {
        if let Some(k) = &f.krediterar {
            return Some(k.kund.clone());
        }
        match self.kund.trim() {
            NY => self.ny_kund.kontrollera().ok().map(|k| fakturakund(&k)),
            id => {
                let id = id.parse::<i64>().ok()?;
                f.kunder.iter().find(|k| k.id == id).cloned()
            }
        }
    }

    fn kontrollera_kund(
        &self,
        f: &Förutsättningar,
        spärrar: &mut Vec<Spärr>,
    ) -> Option<(Fakturakund, Option<GiltigKund>)> {
        // En kreditfaktura går till kunden som originalet registrerades med.
        if let Some(k) = &f.krediterar {
            return Some((k.kund.clone(), None));
        }
        if self.kund.trim() == NY {
            return match self.ny_kund.kontrollera() {
                Ok(k) => Some((fakturakund(&k), Some(k))),
                Err(fel) => {
                    spärrar.extend(ny_kunds_spärrar(fel));
                    None
                }
            };
        }
        let id = self.kund.trim();
        match id
            .parse::<i64>()
            .ok()
            .and_then(|id| f.kunder.iter().find(|k| k.id == id))
        {
            Some(k) => Some((k.clone(), None)),
            None => {
                spärrar.push(Spärr::ny(
                    Fält::Kund,
                    "kund_saknas",
                    if id.is_empty() {
                        "Välj en kund."
                    } else {
                        "Kunden finns inte bland de aktiva kunderna i kundregistret."
                    },
                ));
                None
            }
        }
    }

    /// Prövar det som måste gälla för att utkastet ska kunna sparas: datum som är datum,
    /// belopp som är belopp, momssatser och kontonummer. Ett utkast får sakna kund, nummer,
    /// datum och rader och använda inaktiva konton.
    pub fn som_utkast(&self) -> Result<Utkast, Vec<Spärr>> {
        let mut spärrar = Vec::new();
        let mut ny_kund = None;
        let kund = match self.kund.trim() {
            "" => None,
            // En ny kund utan namn är inget att spara; utkastet saknar då kund.
            NY if self.ny_kund.namn.trim().is_empty() => None,
            NY => {
                match self.ny_kund.kontrollera() {
                    Ok(k) => ny_kund = Some(k),
                    Err(fel) => spärrar.extend(ny_kunds_spärrar(fel)),
                }
                None
            }
            id => match id.parse::<i64>() {
                Ok(id) => Some(id),
                Err(_) => {
                    spärrar.push(Spärr::ny(
                        Fält::Kund,
                        "kund_saknas",
                        "Kunden finns inte bland de aktiva kunderna i kundregistret.",
                    ));
                    None
                }
            },
        };
        let valfritt = |text: &str, fält, kod, spärrar: &mut Vec<Spärr>| match text.trim() {
            "" => None,
            text => datum(text, fält, kod, spärrar),
        };
        let fakturadatum = valfritt(
            &self.fakturadatum,
            Fält::Fakturadatum,
            "fakturadatum_ogiltigt",
            &mut spärrar,
        );
        let förfallodag = valfritt(
            &self.förfallodag,
            Fält::Förfallodag,
            "forfallodag_ogiltig",
            &mut spärrar,
        );
        let mut rader = Vec::new();
        for (i, r) in self.rader.iter().enumerate().filter(|(_, r)| !r.tom()) {
            let momssats = Momssats::tolka(&r.momssats).unwrap_or_else(|| {
                spärrar.push(Spärr::ny(
                    Fält::Rad(i, Radfält::Momssats),
                    "rad_momssats_ogiltig",
                    MOMSSATS_OGILTIG,
                ));
                Momssats::Tjugofem
            });
            let konto = match r.konto.trim() {
                "" => None,
                text => {
                    let konto = text.parse::<Kontonummer>().ok();
                    if konto.is_none() {
                        spärrar.push(Spärr::ny(
                            Fält::Rad(i, Radfält::Konto),
                            "konto_okant",
                            format!("Konto {text} finns inte i kontoplanen."),
                        ));
                    }
                    konto
                }
            };
            let belopp = match r.belopp.trim() {
                "" => None,
                text => {
                    let b = belopp::tolka_med_tecken(text);
                    if b.is_none() {
                        spärrar.push(Spärr::ny(
                            Fält::Rad(i, Radfält::Belopp),
                            "rad_belopp_ogiltigt",
                            BELOPP_OGILTIGT,
                        ));
                    }
                    b
                }
            };
            rader.push(Utkastrad {
                momssats,
                konto,
                belopp,
            });
        }
        if !spärrar.is_empty() {
            return Err(spärrar);
        }
        Ok(Utkast {
            kund,
            ny_kund,
            fakturanummer: self.fakturanummer.trim().to_string(),
            fakturadatum,
            förfallodag,
            rader,
        })
    }
}

/// Den nya kundens fel som spärrar vid fälten i kortet.
fn ny_kunds_spärrar(fel: Vec<kund::Fältfel>) -> impl Iterator<Item = Spärr> {
    fel.into_iter()
        .map(|f| Spärr::ny(Fält::NyKund(f.fält), "ny_kund", f.text))
}

fn datum(text: &str, fält: Fält, kod: &'static str, spärrar: &mut Vec<Spärr>) -> Option<Date> {
    text.parse::<Date>()
        .map_err(|_| spärrar.push(Spärr::ny(fält, kod, "Ange ett datum som ÅÅÅÅ-MM-DD.")))
        .ok()
}
