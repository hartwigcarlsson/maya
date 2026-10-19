//! En kundfaktura som skapas i Maya: utkastets formulär, spärrarna vid utfärdande och
//! konteringen under fakturametoden. Formuläret i webbläsaren, förhandsvisningen och servern
//! kör samma kod (ADR 0003).
//!
//! Ett utkast får vara ofullständigt. Att utfärda kräver att varje spärr i
//! [`UTFÄRDANDESPÄRRAR`] är uppfylld; då ger [`NyKundfaktura::kontrollera`] en [`Utfärdbar`]
//! faktura med numret, referensen, beloppen och konteringen. Belopp är heltal i öre; momsen
//! räknas per momssats på summan av radernas netto ([`crate::faktura::momsrader`]).

use std::fmt;

use jiff::ToSpan;
use jiff::civil::Date;

use crate::belopp::Belopp;
use crate::faktura::{self, Antal, Momsrad, Momssats, Rabatt};
use crate::foretag::Momsperiod;
use crate::kontoplan::Kontonummer;
use crate::rakenskapsar::Räkenskapsår;
use crate::verifikat::{self, Årsval};

/// Kundfordringar, debet när fakturan utfärdas under fakturametoden.
pub const KUNDFORDRINGAR: u16 = 1510;

/// Det största startnummer som kan väljas.
pub const STÖRSTA_STARTNUMMER: u64 = 999_999_999;

/// Intäktskontot som en rad får efter sin momssats: 3001, 3002, 3003 eller 3004. Det kan
/// ändras per rad till ett annat aktivt konto i klass 3.
pub fn intäktskonto(sats: Momssats) -> Kontonummer {
    Kontonummer::konstant(match sats {
        Momssats::Tjugofem => 3001,
        Momssats::Tolv => 3002,
        Momssats::Sex => 3003,
        Momssats::Noll => 3004,
    })
}

/// En fakturarad som användaren skrev den.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NyFakturarad {
    pub beskrivning: String,
    pub antal: String,
    pub enhet: String,
    /// À-pris exklusive moms, i kronor.
    pub à_pris: String,
    /// Rabatt i procent; tom text är ingen rabatt.
    pub rabatt: String,
    /// `25`, `12`, `6` eller `0`.
    pub momssats: String,
    pub konto: String,
}

impl Default for NyFakturarad {
    /// En tom rad med 25 % moms och kontot som hör till satsen.
    fn default() -> Self {
        NyFakturarad {
            beskrivning: String::new(),
            antal: String::new(),
            enhet: String::new(),
            à_pris: String::new(),
            rabatt: String::new(),
            momssats: Momssats::Tjugofem.procent().to_string(),
            konto: intäktskonto(Momssats::Tjugofem).to_string(),
        }
    }
}

impl NyFakturarad {
    /// En rad där inget är ifyllt utöver momssatsen och kontot, som formuläret förväljer.
    pub fn tom(&self) -> bool {
        [
            &self.beskrivning,
            &self.antal,
            &self.enhet,
            &self.à_pris,
            &self.rabatt,
        ]
        .iter()
        .all(|f| f.trim().is_empty())
    }
}

/// Utkastet till en kundfaktura som användaren skrev det.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NyKundfaktura {
    /// Kundens id i kundregistret.
    pub kund: String,
    /// `ÅÅÅÅ-MM-DD`.
    pub fakturadatum: String,
    /// Leveransdatum eller period i fritext; tom text är fakturadatumet.
    pub leveransdatum: String,
    /// `ÅÅÅÅ-MM-DD`; tom text är fakturadatumet plus kundens betalningsvillkor.
    pub förfallodag: String,
    pub er_referens: String,
    /// Första fakturanumret, som bara väljs innan företaget har utfärdat sin första faktura.
    /// Tom text är företagets sparade startnummer.
    pub startnummer: String,
    /// Raderna i formulärets ordning. Helt tomma rader räknas inte.
    pub rader: Vec<NyFakturarad>,
}

/// Kunden som fakturan går till, ur kundregistret. Uppgifterna kopieras in när fakturan
/// utfärdas.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Fakturakund {
    pub id: i64,
    pub namn: String,
    pub identitet: String,
    pub adress: String,
    pub epost: String,
    pub momsregnr: String,
    /// Dagar från fakturadatum till förfallodag.
    pub betalningsvillkor: u16,
}

/// Företagets uppgifter som fakturan kopierar in: säljaren enligt momslagen, F-skatt,
/// betalsätten och om fakturorna har OCR-referens. Tom text betyder att uppgiften saknas.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Säljare {
    pub namn: String,
    /// `NNNNNN-NNNN`.
    pub orgnr: String,
    pub gatuadress: String,
    pub postnummer: String,
    pub postort: String,
    pub säte: String,
    pub momsregnr: String,
    pub f_skatt: bool,
    pub bankgiro: String,
    pub plusgiro: String,
    pub iban: String,
    pub bic: String,
    /// Om fakturorna har en OCR-referens; annars är fakturanumret referensen.
    pub ocr: bool,
}

impl Säljare {
    /// Om minst ett betalsätt finns: bankgiro, plusgiro eller IBAN med BIC.
    pub fn har_betalsätt(&self) -> bool {
        !self.bankgiro.is_empty()
            || !self.plusgiro.is_empty()
            || (!self.iban.is_empty() && !self.bic.is_empty())
    }
}

/// Det som spärrarna prövas mot, läst i utfärdandets transaktion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Förutsättningar {
    /// Kunderna som kan väljas: de aktiva i kundregistret.
    pub kunder: Vec<Fakturakund>,
    pub säljare: Säljare,
    /// Kontona i kontoplanen och om de är aktiva.
    pub konton: Vec<(Kontonummer, bool)>,
    /// Räkenskapsåren i ordning.
    pub räkenskapsår: Vec<Räkenskapsår>,
    /// Momsperioden med gäller-från-datum, äldst först.
    pub momsperioder: Vec<(Date, Momsperiod)>,
    /// Numret som nästa faktura i serien får, när företaget redan har utfärdat en faktura.
    /// `None` före det första utfärdandet; då gäller startnumret.
    pub nästa_nummer: Option<u64>,
    /// Företagets sparade startnummer (1 om inget är valt).
    pub startnummer: u64,
}

impl Förutsättningar {
    /// Om företaget är momsregistrerat på dagen, enligt momsperiodens historik.
    pub fn momsregistrerat(&self, dag: Date) -> bool {
        self.momsperioder
            .iter()
            .rev()
            .find(|(från, _)| *från <= dag)
            .or(self.momsperioder.first())
            .is_some_and(|(_, period)| *period != Momsperiod::EjMomsregistrerad)
    }
}

/// Ett fält på en fakturarad.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Radfält {
    Beskrivning,
    Antal,
    Pris,
    Rabatt,
    Momssats,
    Konto,
}

/// Fältet som en spärr gäller. Raderna räknas från 0 i formulärets ordning, tomma rader
/// medräknade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Fält {
    Kund,
    Fakturadatum,
    Förfallodag,
    Startnummer,
    /// Raderna som helhet (inga rader).
    Rader,
    Rad(usize, Radfält),
    /// Summaraden.
    Summa,
    /// Företagets fakturauppgifter, som ändras under Inställningar → Företag.
    Företag,
}

/// Spärrarna vid utfärdande som Systemdokumentationen återger (S2): koden och regeln. Varje
/// kod som [`NyKundfaktura::kontrollera`] kan ge står här, och ingen annan.
pub const UTFÄRDANDESPÄRRAR: &[(&str, &str)] = &[
    (
        "kund_saknas",
        "Fakturan har en aktiv kund ur kundregistret.",
    ),
    ("kund_adress_saknas", "Kunden har en adress."),
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
         senaste, som då skapas när fakturan utfärdas.",
    ),
    (
        "forfallodag_ogiltig",
        "Förfallodagen skrivs som ÅÅÅÅ-MM-DD.",
    ),
    (
        "forfallodag_fore_fakturadatum",
        "Förfallodagen är inte före fakturadatumet.",
    ),
    (
        "startnummer_ogiltigt",
        "Första fakturanumret är ett heltal från 1 till 999 999 999.",
    ),
    ("rader_saknas", "Fakturan har minst en rad."),
    ("rad_beskrivning_saknas", "Varje rad har en beskrivning."),
    ("rad_antal_saknas", "Varje rad har ett antal."),
    (
        "rad_antal_ogiltigt",
        "Antalet är större än noll och har högst två decimaler.",
    ),
    ("rad_pris_saknas", "Varje rad har ett à-pris."),
    (
        "rad_pris_ogiltigt",
        "À-priset är i kronor exklusive moms, inte negativt och har högst två decimaler.",
    ),
    (
        "rad_rabatt_ogiltig",
        "Rabatten är i procent från 0 till 100 med högst två decimaler.",
    ),
    ("rad_momssats_ogiltig", "Momssatsen är 25, 12, 6 eller 0 %."),
    ("rad_konto_saknas", "Varje rad har ett intäktskonto."),
    ("konto_okant", "Kontot finns i företagets kontoplan."),
    ("konto_inaktivt", "Kontot är aktivt."),
    ("konto_inte_intakt", "Intäktskontot är ett konto i klass 3."),
    ("summa_inte_positiv", "Fakturans summa är större än noll."),
    (
        "foretaget_adress_saknas",
        "Företagets adress (gatuadress, postnummer och ort) är ifylld.",
    ),
    ("foretaget_sate_saknas", "Företagets säte är ifyllt."),
    (
        "foretaget_f_skatt_saknas",
        "Företaget är godkänt för F-skatt.",
    ),
    (
        "foretaget_momsregnr_saknas",
        "Ett företag som är momsregistrerat på fakturadatumet har ett momsregistreringsnummer.",
    ),
    (
        "betalsatt_saknas",
        "Företaget har minst ett betalsätt: bankgiro, plusgiro eller IBAN med BIC.",
    ),
];

/// Något som hindrar utfärdandet, med en kod som formuläret visar vid fältet.
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

/// En rad som klarar spärrarna, med sitt netto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fakturarad {
    pub beskrivning: String,
    pub antal: Antal,
    pub enhet: String,
    /// I öre, exklusive moms.
    pub à_pris: i64,
    pub rabatt: Rabatt,
    pub momssats: Momssats,
    pub konto: Kontonummer,
    /// Antal gånger à-pris minus rabatten, i öre.
    pub netto: i64,
}

/// En faktura som klarar alla spärrar och kan utfärdas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Utfärdbar {
    pub kund: Fakturakund,
    pub fakturadatum: Date,
    /// Leveransdatum eller period som fakturan anger.
    pub leveransdatum: String,
    pub förfallodag: Date,
    pub er_referens: String,
    pub rader: Vec<Fakturarad>,
    pub momsrader: Vec<Momsrad>,
    /// Summan av radernas netto, i öre.
    pub netto: i64,
    pub moms: i64,
    pub total: i64,
    /// Numret i företagets fakturaserie.
    pub nummer: u64,
    /// OCR-referensen med OCR på, annars fakturanumret.
    pub referens: String,
    /// Räkenskapsåret som fakturadatumet avgör.
    pub räkenskapsår: Årsval,
}

impl Utfärdbar {
    /// Verifikatets beskrivning, med motparten (VF2): "Kundfaktura 1007, Kund AB".
    pub fn beskrivning(&self) -> String {
        format!("Kundfaktura {}, {}", self.nummer, self.kund.namn)
    }

    /// Utfärdandets kontering under fakturametoden: 1510 debet med totalen, intäktskontona
    /// kredit med radernas netto per konto (i radernas ordning) och utgående moms kredit per
    /// momssats. 0 % ger ingen momsrad.
    pub fn kontering(&self) -> Vec<verifikat::Rad> {
        let mut rader = vec![rad(Kontonummer::konstant(KUNDFORDRINGAR), self.total)];
        let mut intäkter: Vec<(Kontonummer, i64)> = Vec::new();
        for r in &self.rader {
            match intäkter.iter_mut().find(|(k, _)| *k == r.konto) {
                Some((_, summa)) => *summa += r.netto,
                None => intäkter.push((r.konto, r.netto)),
            }
        }
        rader.extend(intäkter.into_iter().map(|(k, netto)| rad(k, -netto)));
        rader.extend(
            self.momsrader
                .iter()
                .filter_map(|m| Some(rad(m.sats.utgående_momskonto()?, -m.moms))),
        );
        rader.retain(|r| r.debet != Belopp::NOLL || r.kredit != Belopp::NOLL);
        rader
    }
}

/// En verifikatrad med `belopp` i debet om det är positivt och i kredit om det är negativt.
fn rad(konto: Kontonummer, belopp: i64) -> verifikat::Rad {
    let (debet, kredit) = if belopp >= 0 {
        (belopp, 0)
    } else {
        (0, -belopp)
    };
    verifikat::Rad {
        konto,
        debet: Belopp::från_öre(debet),
        kredit: Belopp::från_öre(kredit),
        text: None,
    }
}

/// Summorna i formuläret som de är nu, också med rader som inte är färdiga: för
/// förhandsvisningen och summaraden medan användaren skriver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Beräkning {
    /// Varje rads netto i formulärets ordning, `None` för en tom rad eller en rad vars antal
    /// eller à-pris saknas eller inte går att tolka.
    pub rader: Vec<Option<i64>>,
    pub momsrader: Vec<Momsrad>,
    pub netto: i64,
    pub moms: i64,
    pub total: i64,
}

/// Raden tolkad så långt det går.
struct Tolkad {
    antal: Option<Antal>,
    à_pris: Option<i64>,
    rabatt: Option<Rabatt>,
    momssats: Option<Momssats>,
}

impl NyFakturarad {
    fn tolka(&self) -> Tolkad {
        Tolkad {
            antal: Antal::tolka(&self.antal),
            à_pris: Belopp::tolka(self.à_pris.trim()).map(Belopp::öre),
            rabatt: Rabatt::tolka(&self.rabatt),
            momssats: Momssats::tolka(&self.momssats),
        }
    }

    fn netto(&self) -> Option<(Momssats, i64)> {
        if self.tom() {
            return None;
        }
        let t = self.tolka();
        Some((
            t.momssats?,
            faktura::radnetto(t.antal?, t.à_pris?, t.rabatt?),
        ))
    }
}

impl NyKundfaktura {
    /// Radernas netto, momsen per sats och totalen, med de rader som går att räkna.
    pub fn beräkna(&self) -> Beräkning {
        let netton: Vec<Option<(Momssats, i64)>> =
            self.rader.iter().map(NyFakturarad::netto).collect();
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

    /// Prövar spärrarna vid utfärdande. Spärrarna kommer i fältens ordning, och företagets
    /// sist.
    pub fn kontrollera(&self, f: &Förutsättningar) -> Result<Utfärdbar, Vec<Spärr>> {
        let mut spärrar = Vec::new();

        let kund = self.kontrollera_kund(f, &mut spärrar);
        let fakturadatum = self.kontrollera_fakturadatum(&mut spärrar);
        let årsval = fakturadatum.and_then(|d| {
            let mut fel = Vec::new();
            let val = verifikat::välj_år(d, &f.räkenskapsår, &mut fel);
            spärrar.extend(
                fel.into_iter()
                    .map(|s| Spärr::ny(Fält::Fakturadatum, s.kod, fakturadatumets(&s.text))),
            );
            val
        });
        let förfallodag = self.kontrollera_förfallodag(fakturadatum, kund, &mut spärrar);
        let nummer = self.kontrollera_startnummer(f, &mut spärrar);

        let ifyllda: Vec<(usize, &NyFakturarad)> = self
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
        let antal_ifyllda = ifyllda.len();
        let mut rader = Vec::new();
        for (i, r) in ifyllda {
            rader.extend(kontrollera_rad(i, r, f, &mut spärrar));
        }
        let momsrader = faktura::momsrader(rader.iter().map(|r| (r.momssats, r.netto)));
        let netto: i64 = momsrader.iter().map(|m| m.underlag).sum();
        let moms: i64 = momsrader.iter().map(|m| m.moms).sum();
        let total = netto + moms;
        // Summan prövas när varje ifylld rad klarar sina spärrar.
        if !rader.is_empty() && rader.len() == antal_ifyllda && total <= 0 {
            spärrar.push(Spärr::ny(
                Fält::Summa,
                "summa_inte_positiv",
                "Fakturans summa måste vara större än noll.",
            ));
        }

        kontrollera_säljaren(f, fakturadatum, &mut spärrar);

        match (kund, fakturadatum, årsval, förfallodag, nummer) {
            (
                Some(kund),
                Some(fakturadatum),
                Some(räkenskapsår),
                Some(förfallodag),
                Some(nummer),
            ) if spärrar.is_empty() => {
                let leveransdatum = match self.leveransdatum.trim() {
                    "" => fakturadatum.to_string(),
                    text => text.to_string(),
                };
                Ok(Utfärdbar {
                    kund: kund.clone(),
                    fakturadatum,
                    leveransdatum,
                    förfallodag,
                    er_referens: self.er_referens.trim().to_string(),
                    rader,
                    momsrader,
                    netto,
                    moms,
                    total,
                    nummer,
                    referens: if f.säljare.ocr {
                        faktura::ocr(nummer)
                    } else {
                        nummer.to_string()
                    },
                    räkenskapsår,
                })
            }
            _ => Err(spärrar),
        }
    }

    fn kontrollera_kund<'a>(
        &self,
        f: &'a Förutsättningar,
        spärrar: &mut Vec<Spärr>,
    ) -> Option<&'a Fakturakund> {
        let id = self.kund.trim();
        let kund = id
            .parse::<i64>()
            .ok()
            .and_then(|id| f.kunder.iter().find(|k| k.id == id));
        match kund {
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
            Some(k) if k.adress.trim().is_empty() => {
                spärrar.push(Spärr::ny(
                    Fält::Kund,
                    "kund_adress_saknas",
                    format!("{} saknar adress. Fyll i den i kundregistret.", k.namn),
                ));
                Some(k)
            }
            Some(k) => Some(k),
        }
    }

    fn kontrollera_fakturadatum(&self, spärrar: &mut Vec<Spärr>) -> Option<Date> {
        match self.fakturadatum.trim() {
            "" => {
                spärrar.push(Spärr::ny(
                    Fält::Fakturadatum,
                    "fakturadatum_saknas",
                    "Ange fakturadatum.",
                ));
                None
            }
            text => datum(text, Fält::Fakturadatum, "fakturadatum_ogiltigt", spärrar),
        }
    }

    fn kontrollera_förfallodag(
        &self,
        fakturadatum: Option<Date>,
        kund: Option<&Fakturakund>,
        spärrar: &mut Vec<Spärr>,
    ) -> Option<Date> {
        let dag = match self.förfallodag.trim() {
            "" => {
                return fakturadatum.and_then(|d| {
                    d.checked_add(i64::from(kund?.betalningsvillkor).days())
                        .ok()
                });
            }
            text => datum(text, Fält::Förfallodag, "forfallodag_ogiltig", spärrar)?,
        };
        if fakturadatum.is_some_and(|f| dag < f) {
            spärrar.push(Spärr::ny(
                Fält::Förfallodag,
                "forfallodag_fore_fakturadatum",
                "Förfallodagen kan inte vara före fakturadatumet.",
            ));
            return None;
        }
        Some(dag)
    }

    fn kontrollera_startnummer(
        &self,
        f: &Förutsättningar,
        spärrar: &mut Vec<Spärr>,
    ) -> Option<u64> {
        // Efter det första utfärdandet gäller serien; utan ett valt startnummer det sparade.
        match (f.nästa_nummer, tolka_startnummer(&self.startnummer)) {
            (Some(nästa), _) => Some(nästa),
            (None, Some(Some(start))) => Some(start),
            (None, Some(None)) => Some(f.startnummer),
            (None, None) => {
                spärrar.push(Spärr::ny(
                    Fält::Startnummer,
                    "startnummer_ogiltigt",
                    "Ange första fakturanumret som ett heltal från 1 till 999 999 999.",
                ));
                None
            }
        }
    }

    /// Prövar det som måste gälla för att utkastet ska kunna sparas: datum som är datum, tal
    /// som är tal, en momssats och kontonummer som är kontonummer. Ett utkast får sakna kund,
    /// datum och rader och använda inaktiva konton. Spärrarna har samma koder som vid
    /// utfärdande.
    pub fn som_utkast(&self) -> Result<Utkast, Vec<Spärr>> {
        let mut spärrar = Vec::new();
        let kund = match self.kund.trim() {
            "" => None,
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
        let startnummer = match tolka_startnummer(&self.startnummer) {
            Some(n) => n,
            None => {
                spärrar.push(Spärr::ny(
                    Fält::Startnummer,
                    "startnummer_ogiltigt",
                    "Ange första fakturanumret som ett heltal från 1 till 999 999 999.",
                ));
                None
            }
        };
        let mut rader = Vec::new();
        for (i, r) in self.rader.iter().enumerate().filter(|(_, r)| !r.tom()) {
            let t = r.tolka();
            let mut fel = |fält, kod, text: &str| {
                spärrar.push(Spärr::ny(Fält::Rad(i, fält), kod, text));
            };
            let antal = match r.antal.trim() {
                "" => None,
                _ if t.antal.is_none() => {
                    fel(Radfält::Antal, "rad_antal_ogiltigt", ANTAL_OGILTIGT);
                    None
                }
                _ => t.antal,
            };
            let à_pris = match r.à_pris.trim() {
                "" => None,
                _ if t.à_pris.is_none() => {
                    fel(Radfält::Pris, "rad_pris_ogiltigt", PRIS_OGILTIGT);
                    None
                }
                _ => t.à_pris,
            };
            if t.rabatt.is_none() {
                fel(Radfält::Rabatt, "rad_rabatt_ogiltig", RABATT_OGILTIG);
            }
            if t.momssats.is_none() {
                fel(Radfält::Momssats, "rad_momssats_ogiltig", MOMSSATS_OGILTIG);
            }
            let konto = match r.konto.trim() {
                "" => None,
                text => {
                    let konto = text.parse::<Kontonummer>().ok();
                    if konto.is_none() {
                        fel(
                            Radfält::Konto,
                            "konto_okant",
                            &format!("Konto {text} finns inte i kontoplanen."),
                        );
                    }
                    konto
                }
            };
            rader.push(Utkastrad {
                beskrivning: r.beskrivning.trim().to_string(),
                antal,
                enhet: r.enhet.trim().to_string(),
                à_pris,
                rabatt: t.rabatt.unwrap_or(Rabatt::INGEN),
                momssats: t.momssats.unwrap_or(Momssats::Tjugofem),
                konto,
            });
        }
        if !spärrar.is_empty() {
            return Err(spärrar);
        }
        Ok(Utkast {
            kund,
            fakturadatum,
            leveransdatum: self.leveransdatum.trim().to_string(),
            förfallodag,
            er_referens: self.er_referens.trim().to_string(),
            startnummer,
            rader,
        })
    }
}

const ANTAL_OGILTIGT: &str = "Ange ett antal större än noll med högst två decimaler.";
const PRIS_OGILTIGT: &str = "Ange à-priset i kronor med högst två decimaler.";
const RABATT_OGILTIG: &str = "Ange rabatten i procent, från 0 till 100.";
const MOMSSATS_OGILTIG: &str = "Välj 25, 12, 6 eller 0 %.";

/// `Some(None)` för tom text, `Some(Some(n))` för ett giltigt startnummer, `None` annars.
fn tolka_startnummer(text: &str) -> Option<Option<u64>> {
    let text = text.trim();
    if text.is_empty() {
        return Some(None);
    }
    text.bytes()
        .all(|b| b.is_ascii_digit())
        .then(|| text.parse::<u64>().ok())
        .flatten()
        .filter(|n| (1..=STÖRSTA_STARTNUMMER).contains(n))
        .map(Some)
}

fn datum(text: &str, fält: Fält, kod: &'static str, spärrar: &mut Vec<Spärr>) -> Option<Date> {
    text.parse::<Date>()
        .map_err(|_| spärrar.push(Spärr::ny(fält, kod, "Ange ett datum som ÅÅÅÅ-MM-DD.")))
        .ok()
}

/// Verifikatets text om datumet, sagd om fakturadatumet.
fn fakturadatumets(text: &str) -> String {
    text.replacen("Datumet", "Fakturadatumet", 1)
        .replacen("datumet", "fakturadatumet", 1)
}

fn kontrollera_rad(
    i: usize,
    r: &NyFakturarad,
    f: &Förutsättningar,
    spärrar: &mut Vec<Spärr>,
) -> Option<Fakturarad> {
    let t = r.tolka();
    let mut fel = |fält, kod, text: String| spärrar.push(Spärr::ny(Fält::Rad(i, fält), kod, text));
    let beskrivning = r.beskrivning.trim();
    if beskrivning.is_empty() {
        fel(
            Radfält::Beskrivning,
            "rad_beskrivning_saknas",
            "Beskriv vad raden gäller.".into(),
        );
    }
    let antal = match (r.antal.trim(), t.antal) {
        ("", _) => {
            fel(Radfält::Antal, "rad_antal_saknas", "Ange ett antal.".into());
            None
        }
        (_, Some(a)) if a.hundradelar() > 0 => Some(a),
        _ => {
            fel(Radfält::Antal, "rad_antal_ogiltigt", ANTAL_OGILTIGT.into());
            None
        }
    };
    let à_pris = match (r.à_pris.trim(), t.à_pris) {
        ("", _) => {
            fel(Radfält::Pris, "rad_pris_saknas", "Ange ett à-pris.".into());
            None
        }
        (_, Some(p)) => Some(p),
        _ => {
            fel(Radfält::Pris, "rad_pris_ogiltigt", PRIS_OGILTIGT.into());
            None
        }
    };
    if t.rabatt.is_none() {
        fel(Radfält::Rabatt, "rad_rabatt_ogiltig", RABATT_OGILTIG.into());
    }
    if t.momssats.is_none() {
        fel(
            Radfält::Momssats,
            "rad_momssats_ogiltig",
            MOMSSATS_OGILTIG.into(),
        );
    }
    let konto = match r.konto.trim() {
        "" => {
            fel(
                Radfält::Konto,
                "rad_konto_saknas",
                "Välj ett intäktskonto.".into(),
            );
            None
        }
        text => match text
            .parse::<Kontonummer>()
            .ok()
            .and_then(|k| f.konton.iter().find(|(n, _)| *n == k))
        {
            None => {
                fel(
                    Radfält::Konto,
                    "konto_okant",
                    format!("Konto {text} finns inte i kontoplanen."),
                );
                None
            }
            Some((konto, _)) if konto.klass() != 3 => {
                fel(
                    Radfält::Konto,
                    "konto_inte_intakt",
                    format!("Konto {konto} är inget intäktskonto. Välj ett konto i klass 3."),
                );
                None
            }
            Some((konto, false)) => {
                fel(
                    Radfält::Konto,
                    "konto_inaktivt",
                    format!("Konto {konto} är inaktivt. Aktivera det för att bokföra på det."),
                );
                None
            }
            Some((konto, true)) => Some(*konto),
        },
    };
    let (antal, à_pris, rabatt, momssats, konto) =
        (antal?, à_pris?, t.rabatt?, t.momssats?, konto?);
    if beskrivning.is_empty() {
        return None;
    }
    Some(Fakturarad {
        beskrivning: beskrivning.to_string(),
        antal,
        enhet: r.enhet.trim().to_string(),
        à_pris,
        rabatt,
        momssats,
        konto,
        netto: faktura::radnetto(antal, à_pris, rabatt),
    })
}

fn kontrollera_säljaren(
    f: &Förutsättningar,
    fakturadatum: Option<Date>,
    spärrar: &mut Vec<Spärr>,
) {
    let s = &f.säljare;
    let mut fel = |kod, text: &str| spärrar.push(Spärr::ny(Fält::Företag, kod, text));
    if [&s.gatuadress, &s.postnummer, &s.postort]
        .iter()
        .any(|d| d.trim().is_empty())
    {
        fel(
            "foretaget_adress_saknas",
            "Företagets adress saknas. Fyll i den under Inställningar → Företag.",
        );
    }
    if s.säte.trim().is_empty() {
        fel(
            "foretaget_sate_saknas",
            "Företagets säte saknas. Fyll i det under Inställningar → Företag.",
        );
    }
    if !s.f_skatt {
        fel(
            "foretaget_f_skatt_saknas",
            "Fakturan ska ange att företaget är godkänt för F-skatt. Markera det under \
             Inställningar → Företag.",
        );
    }
    if fakturadatum.is_some_and(|d| f.momsregistrerat(d)) && s.momsregnr.trim().is_empty() {
        fel(
            "foretaget_momsregnr_saknas",
            "Företaget är momsregistrerat men saknar momsregistreringsnummer. Fyll i det under \
             Inställningar → Företag.",
        );
    }
    if !s.har_betalsätt() {
        fel(
            "betalsatt_saknas",
            &format!(
                "{} har inget betalsätt. Fakturan måste ange minst ett: lägg till bankgiro, \
                 plusgiro eller IBAN med BIC under Inställningar → Företag.",
                s.namn
            ),
        );
    }
}

/// En rad i ett sparat utkast. Det som inte är ifyllt saknas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Utkastrad {
    pub beskrivning: String,
    pub antal: Option<Antal>,
    pub enhet: String,
    /// I öre, exklusive moms.
    pub à_pris: Option<i64>,
    pub rabatt: Rabatt,
    pub momssats: Momssats,
    pub konto: Option<Kontonummer>,
}

/// Ett utkast som går att spara: det som är ifyllt går att lagra.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Utkast {
    pub kund: Option<i64>,
    pub fakturadatum: Option<Date>,
    pub leveransdatum: String,
    pub förfallodag: Option<Date>,
    pub er_referens: String,
    /// Första fakturanumret, om det valdes i utkastet.
    pub startnummer: Option<u64>,
    /// Raderna i formulärets ordning, utan helt tomma rader.
    pub rader: Vec<Utkastrad>,
}

impl Utkast {
    /// Utkastet som formulär, för att fortsätta skriva på det.
    pub fn som_formulär(&self) -> NyKundfaktura {
        let text = |d: Option<Date>| d.map(|d| d.to_string()).unwrap_or_default();
        NyKundfaktura {
            kund: self.kund.map(|k| k.to_string()).unwrap_or_default(),
            fakturadatum: text(self.fakturadatum),
            leveransdatum: self.leveransdatum.clone(),
            förfallodag: text(self.förfallodag),
            er_referens: self.er_referens.clone(),
            startnummer: self.startnummer.map(|n| n.to_string()).unwrap_or_default(),
            rader: self
                .rader
                .iter()
                .map(|r| NyFakturarad {
                    beskrivning: r.beskrivning.clone(),
                    antal: r.antal.map(|a| a.to_string()).unwrap_or_default(),
                    enhet: r.enhet.clone(),
                    à_pris: r
                        .à_pris
                        .map(|p| kronor(Belopp::från_öre(p)))
                        .unwrap_or_default(),
                    rabatt: if r.rabatt == Rabatt::INGEN {
                        String::new()
                    } else {
                        r.rabatt.to_string()
                    },
                    momssats: r.momssats.procent().to_string(),
                    konto: r.konto.map(|k| k.to_string()).unwrap_or_default(),
                })
                .collect(),
        }
    }
}

/// Ett belopp som det skrivs i formuläret, utan tusentalsavgränsare och utan `,00`.
fn kronor(belopp: Belopp) -> String {
    let (kr, öre) = (belopp.öre() / 100, belopp.öre() % 100);
    if öre == 0 {
        kr.to_string()
    } else {
        format!("{kr},{öre:02}")
    }
}
