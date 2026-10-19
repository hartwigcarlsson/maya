//! Tabellen över automatkonteringar (S2): för varje flöde vad som debiteras och krediteras och
//! när. Systemdokumentationen återger tabellens rader ordagrant som specen skriver dem
//! (`tests/golden/automatkonteringar.md`), med egna stycken runt om för den som läser den.
//!
//! Tabellen byggs här ur samma konstanter och funktioner som flödena konterar med
//! ([`KUNDFORDRINGAR`], [`Momssats::utgående_momskonto`], [`fiktiv_utgående_moms`] och de andra),
//! så att ett konto som byts i konteringen byts i dokumentationen. Varje ruta är text med
//! kontona och rollerna utmärkta ([`Del`]), och `core/tests/it/automatkonteringar.rs` bokför
//! varje flöde och prövar att verifikatets konton är de som raden anger.

use crate::betalning::{KURSFÖRLUST, KURSVINST, STÖRSTA_ÖRESUTJÄMNING, ÖRESUTJÄMNING};
use crate::faktura::Momssats;
use crate::ingaende_balans::PRELIMINÄRT_RESULTAT;
use crate::kontoplan::Kontonummer;
use crate::kundfaktura::KUNDFORDRINGAR;
use crate::kundforlust::KUNDFÖRLUSTER;
use crate::leverantorsfaktura::{
    FIKTIV_INGÅENDE_MOMS, FIKTIV_INGÅENDE_MOMS_BYGG, INGÅENDE_MOMS, LEVERANTÖRSSKULDER,
    fiktiv_utgående_moms,
};

/// Flödet som en rad i tabellen beskriver, i tabellens ordning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Flöde {
    // Etapp 1
    Rättelse,
    RegistreraOm,
    PreliminärIngåendeBalans,
    // Etapp 2: fakturorna
    UtfärdaKundfaktura,
    KundbetalningBokförd,
    KundbetalningInteBokförd,
    KundkreditBokförd,
    KundkreditInteBokförd,
    ÅterbetalningTillKund,
    Kundförlust,
    ÅterkallaKundförlust,
    RegistreraLeverantörsfaktura,
    RegistreraOmvändSkattskyldighet,
    LeverantörsbetalningBokförd,
    LeverantörsbetalningInteBokförd,
    LeverantörskreditBokförd,
    LeverantörskreditInteBokförd,
    ÅterbetalningFrånLeverantör,
    ÖresutjämningKund,
    ÖresutjämningLeverantör,
    Kursdifferens,
    RättaIFlödet,
    T2,
}

impl Flöde {
    /// Flödena i tabellens ordning.
    pub const ALLA: [Flöde; 23] = [
        Flöde::Rättelse,
        Flöde::RegistreraOm,
        Flöde::PreliminärIngåendeBalans,
        Flöde::UtfärdaKundfaktura,
        Flöde::KundbetalningBokförd,
        Flöde::KundbetalningInteBokförd,
        Flöde::KundkreditBokförd,
        Flöde::KundkreditInteBokförd,
        Flöde::ÅterbetalningTillKund,
        Flöde::Kundförlust,
        Flöde::ÅterkallaKundförlust,
        Flöde::RegistreraLeverantörsfaktura,
        Flöde::RegistreraOmvändSkattskyldighet,
        Flöde::LeverantörsbetalningBokförd,
        Flöde::LeverantörsbetalningInteBokförd,
        Flöde::LeverantörskreditBokförd,
        Flöde::LeverantörskreditInteBokförd,
        Flöde::ÅterbetalningFrånLeverantör,
        Flöde::ÖresutjämningKund,
        Flöde::ÖresutjämningLeverantör,
        Flöde::Kursdifferens,
        Flöde::RättaIFlödet,
        Flöde::T2,
    ];
}

/// Ett konto som raden anger med sin roll i stället för sitt nummer: det beror på fakturan,
/// betalningen eller verifikatet som flödet utgår från.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Roll {
    /// Intäktskontona på kundfakturans rader (klass 3).
    Intäkt,
    /// Kostnadskontona på leverantörsfakturans rader.
    Kostnad,
    /// Betalkontot som betalningen anger, ett aktivt konto i klass 1 ([`crate::betalning::BETALKONTO`] som
    /// förval).
    Betalkonto,
    /// Utgående moms per momssats.
    UtgåendeMoms,
    /// Ingående moms.
    IngåendeMoms,
    /// Fiktiv ingående moms vid omvänd skattskyldighet.
    FiktivIngåendeMoms,
    /// Fiktiv utgående moms vid omvänd skattskyldighet per momssats.
    FiktivUtgåendeMoms,
    /// Paret av fiktiv ingående och utgående moms vid omvänd skattskyldighet. Det summerar
    /// till noll och står på båda sidor.
    FiktivMoms,
    /// Raderna i debet i verifikatet som flödet utgår från (originalet, förlusten eller
    /// registreringen).
    OriginaletsDebet,
    /// Raderna i kredit i verifikatet som flödet utgår från.
    OriginaletsKredit,
    /// En rättelse av flödets verifikat: varje rad med sidorna bytta.
    Rättelse,
}

impl Roll {
    /// Kontona som rollen kan vara, när de är givna av regeln och inte av fakturan eller
    /// verifikatet; annars inga.
    pub fn konton(self) -> Vec<Kontonummer> {
        match self {
            Roll::UtgåendeMoms => utgående_moms(),
            Roll::IngåendeMoms => vec![Kontonummer::konstant(INGÅENDE_MOMS)],
            Roll::FiktivIngåendeMoms => fiktiv_ingående_moms(),
            Roll::FiktivUtgåendeMoms => fiktiv_utgående(),
            Roll::FiktivMoms => fiktiv_ingående_moms()
                .into_iter()
                .chain(fiktiv_utgående())
                .collect(),
            Roll::Intäkt
            | Roll::Kostnad
            | Roll::Betalkonto
            | Roll::OriginaletsDebet
            | Roll::OriginaletsKredit
            | Roll::Rättelse => Vec::new(),
        }
    }
}

/// En del av en ruta i tabellen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Del {
    /// Text med `**fetstil**` och `*kursiv*` som i Markdown.
    Text(String),
    /// Ett konto som flödet bokför på.
    Konto(Kontonummer),
    /// Konton efter momssats, skrivna `2611/2621/2631`.
    Konton(Vec<Kontonummer>),
    /// Ett konto med sin roll, skrivet med texten.
    Roll(Roll, &'static str),
}

/// En ruta i tabellen.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ruta(pub Vec<Del>);

impl Ruta {
    /// Rutan som Markdown, som i specen.
    pub fn markdown(&self) -> String {
        self.0
            .iter()
            .map(|d| match d {
                Del::Text(t) => t.clone(),
                Del::Konto(k) => k.get().to_string(),
                Del::Konton(k) => k
                    .iter()
                    .map(|k| k.get().to_string())
                    .collect::<Vec<_>>()
                    .join("/"),
                Del::Roll(_, t) => (*t).to_string(),
            })
            .collect()
    }

    /// Kontona som rutan anger med nummer, i ordning.
    pub fn konton(&self) -> Vec<Kontonummer> {
        self.0
            .iter()
            .flat_map(|d| match d {
                Del::Konto(k) => vec![*k],
                Del::Konton(k) => k.clone(),
                Del::Text(_) | Del::Roll(..) => Vec::new(),
            })
            .collect()
    }

    /// Rollerna som rutan anger, i ordning.
    pub fn roller(&self) -> Vec<Roll> {
        self.0
            .iter()
            .filter_map(|d| match d {
                Del::Roll(r, _) => Some(*r),
                _ => None,
            })
            .collect()
    }
}

/// En rad i tabellen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Automatkontering {
    pub flöde: Flöde,
    /// Flödets namn, första kolumnen.
    pub namn: Ruta,
    pub debet: Ruta,
    pub kredit: Ruta,
    pub villkor: Ruta,
}

/// Tabellens kolumner.
pub const KOLUMNER: [&str; 4] = ["Flöde", "Debet", "Kredit", "Villkor"];

fn konto(nummer: u16) -> Del {
    Del::Konto(Kontonummer::konstant(nummer))
}

fn text(t: impl Into<String>) -> Del {
    Del::Text(t.into())
}

fn roll(r: Roll, t: &'static str) -> Del {
    Del::Roll(r, t)
}

fn ruta<const N: usize>(delar: [Del; N]) -> Ruta {
    Ruta(delar.into())
}

/// De momsbärande satserna, 25, 12 och 6 %.
fn satser_med_moms() -> impl Iterator<Item = Momssats> {
    Momssats::ALLA.into_iter().filter(|s| s.procent() > 0)
}

fn utgående_moms() -> Vec<Kontonummer> {
    satser_med_moms()
        .filter_map(Momssats::utgående_momskonto)
        .collect()
}

fn fiktiv_utgående() -> Vec<Kontonummer> {
    satser_med_moms()
        .filter_map(|s| fiktiv_utgående_moms(s.procent()))
        .map(Kontonummer::konstant)
        .collect()
}

fn fiktiv_ingående_moms() -> Vec<Kontonummer> {
    vec![
        Kontonummer::konstant(FIKTIV_INGÅENDE_MOMS),
        Kontonummer::konstant(FIKTIV_INGÅENDE_MOMS_BYGG),
    ]
}

/// En rest som stängs mot öresutjämning, t.ex. "10 kr".
fn största_rest() -> String {
    format!("{} kr", STÖRSTA_ÖRESUTJÄMNING / 100)
}

/// Raderna i tabellen, i ordning.
pub fn tabell() -> Vec<Automatkontering> {
    use Flöde as F;
    use Roll as R;
    let rad = |flöde, namn, debet, kredit, villkor| Automatkontering {
        flöde,
        namn,
        debet,
        kredit,
        villkor,
    };
    let tom = Ruta::default;
    vec![
        // Etapp 1
        rad(
            F::Rättelse,
            ruta([text("**Rättelse** av ett bokfört verifikat")]),
            ruta([roll(
                R::OriginaletsKredit,
                "för varje rad i originalet: samma konto med originalets kreditbelopp",
            )]),
            ruta([roll(
                R::OriginaletsDebet,
                "för varje rad i originalet: samma konto med originalets debetbelopp",
            )]),
            ruta([text(
                "originalet är bokfört, inte rättat, inte en rättelse och inte från ett av Mayas \
                 flöden; radtexten följer med; rättelsen länkas till originalet; datum ≥ \
                 originalets datum och i en period som inte är låst",
            )]),
        ),
        rad(
            F::RegistreraOm,
            ruta([text("**Registrera om** (inget bokförs)")]),
            ruta([text(
                "utkast med originalets rader och underlag, märkt *kopia av*",
            )]),
            tom(),
            ruta([text(
                "bara tillsammans med en rättelse; utkastet följer de vanliga spärrarna",
            )]),
        ),
        rad(
            F::PreliminärIngåendeBalans,
            ruta([text(
                "**Preliminär ingående balans** för år *n* (inget verifikat)",
            )]),
            ruta([text(
                "IB(*k*, *n*) = UB(*k*, *n*−1) för varje konto *k* i klass 1–2",
            )]),
            tom(),
            ruta([
                text("år *n*−1 finns i Maya och är inte stängt; därtill IB("),
                Del::Konto(PRELIMINÄRT_RESULTAT),
                text(", *n*) = UB("),
                Del::Konto(PRELIMINÄRT_RESULTAT),
                text(
                    ", *n*−1) + Σ UB(*k*, *n*−1) över klass 3–8, alltså det resultat som inte \
                     är ombokat; räknas vid läsning och visas som preliminär",
                ),
            ]),
        ),
        // Etapp 2: fakturorna
        rad(
            F::UtfärdaKundfaktura,
            ruta([text("**Utfärda** eller **registrera** kundfaktura")]),
            ruta([konto(KUNDFORDRINGAR), text(" totalen")]),
            ruta([
                roll(R::Intäkt, "intäktskontot per rad"),
                text(" netto; "),
                Del::Konton(utgående_moms()),
                text(" moms"),
            ]),
            ruta([text(
                "fakturametoden; datum = fakturadatum; omvänd skattskyldighet och 0 % ger ingen \
                 momsrad",
            )]),
        ),
        rad(
            F::KundbetalningBokförd,
            ruta([text("**Betalning** av kundfaktura, fordran bokförd")]),
            ruta([roll(R::Betalkonto, "betalkontot")]),
            ruta([konto(KUNDFORDRINGAR)]),
            tom(),
        ),
        rad(
            F::KundbetalningInteBokförd,
            ruta([text("**Betalning** av kundfaktura, fordran inte bokförd")]),
            ruta([roll(R::Betalkonto, "betalkontot")]),
            ruta([
                roll(R::Intäkt, "intäkt"),
                text(" och "),
                roll(R::UtgåendeMoms, "moms"),
                text(" proportionellt"),
            ]),
            ruta([text("kontantmetoden")]),
        ),
        rad(
            F::KundkreditBokförd,
            ruta([text("**Kreditfaktura** till kund, fordran bokförd")]),
            ruta([
                roll(R::Intäkt, "intäktskontot per rad"),
                text(" netto; "),
                roll(R::UtgåendeMoms, "utgående moms per sats"),
            ]),
            ruta([konto(KUNDFORDRINGAR), text(" totalen")]),
            ruta([text("speglar originalet, proportionellt vid delkredit")]),
        ),
        rad(
            F::KundkreditInteBokförd,
            ruta([text("**Kreditfaktura** till kund, fordran inte bokförd")]),
            ruta([
                roll(R::Intäkt, "intäkt"),
                text(" och "),
                roll(R::UtgåendeMoms, "moms"),
                text(" för den del som överstiger det obetalda"),
            ]),
            ruta([konto(KUNDFORDRINGAR), text(" samma belopp")]),
            ruta([text(
                "kontantmetoden; kredit av en obetald del bokförs inte",
            )]),
        ),
        rad(
            F::ÅterbetalningTillKund,
            ruta([text("**Återbetalning** av kundens tillgodohavande")]),
            ruta([konto(KUNDFORDRINGAR)]),
            ruta([roll(R::Betalkonto, "betalkontot")]),
            tom(),
        ),
        rad(
            F::Kundförlust,
            ruta([text("**Kundförlust**, fordran bokförd")]),
            ruta([
                konto(KUNDFÖRLUSTER),
                text(" netto; "),
                roll(R::UtgåendeMoms, "utgående moms per sats"),
                text(" proportionellt"),
            ]),
            ruta([konto(KUNDFORDRINGAR), text(" återstående belopp")]),
            ruta([text(
                "konstaterad förlust med skäl; fordran inte bokförd ger bara status",
            )]),
        ),
        rad(
            F::ÅterkallaKundförlust,
            ruta([text("**Återkalla kundförlust**")]),
            ruta([roll(
                R::OriginaletsKredit,
                "förlustverifikatets kreditrader",
            )]),
            ruta([roll(R::OriginaletsDebet, "förlustverifikatets debetrader")]),
            ruta([text("länkat till förlusten")]),
        ),
        rad(
            F::RegistreraLeverantörsfaktura,
            ruta([text("**Registrera** leverantörsfaktura")]),
            ruta([
                roll(R::Kostnad, "kostnadskontot per rad"),
                text(" netto; "),
                konto(INGÅENDE_MOMS),
                text(" moms"),
            ]),
            ruta([konto(LEVERANTÖRSSKULDER), text(" totalen")]),
            ruta([text(
                "fakturametoden; datum = fakturadatum; företag utan momsregistrering bokför \
                 momsen på kostnadskontot",
            )]),
        ),
        rad(
            F::RegistreraOmvändSkattskyldighet,
            ruta([text(
                "**Registrera** leverantörsfaktura med omvänd skattskyldighet",
            )]),
            ruta([
                roll(R::Kostnad, "kostnadskontot per rad"),
                text(" netto; "),
                konto(FIKTIV_INGÅENDE_MOMS),
                text(" (EU-tjänst, EU-vara, utanför EU) eller "),
                konto(FIKTIV_INGÅENDE_MOMS_BYGG),
                text(" (bygg) fiktiv moms"),
            ]),
            ruta([
                konto(LEVERANTÖRSSKULDER),
                text(" totalen; "),
                Del::Konton(fiktiv_utgående()),
                text(" fiktiv moms"),
            ]),
            ruta([text(
                "fakturametoden; justeringsparet mot 4599 läggs till i etapp 3",
            )]),
        ),
        rad(
            F::LeverantörsbetalningBokförd,
            ruta([text("**Betalning** av leverantörsfaktura, skuld bokförd")]),
            ruta([konto(LEVERANTÖRSSKULDER)]),
            ruta([roll(R::Betalkonto, "betalkontot")]),
            tom(),
        ),
        rad(
            F::LeverantörsbetalningInteBokförd,
            ruta([text(
                "**Betalning** av leverantörsfaktura, skuld inte bokförd",
            )]),
            ruta([
                roll(R::Kostnad, "kostnad"),
                text(" och "),
                roll(R::IngåendeMoms, "ingående moms"),
                text(" (och "),
                roll(R::FiktivIngåendeMoms, "fiktiv moms"),
                text(" vid omvänd skattskyldighet) proportionellt"),
            ]),
            ruta([
                roll(R::Betalkonto, "betalkontot"),
                text(" (och "),
                roll(R::FiktivUtgåendeMoms, "fiktiv utgående moms"),
                text(")"),
            ]),
            ruta([text("kontantmetoden")]),
        ),
        rad(
            F::LeverantörskreditBokförd,
            ruta([text("**Kreditfaktura** från leverantör, skuld bokförd")]),
            ruta([konto(LEVERANTÖRSSKULDER), text(" totalen")]),
            ruta([
                roll(R::OriginaletsDebet, "registreringens debetrader"),
                text(", proportionellt vid delkredit"),
            ]),
            ruta([text(
                "speglar registreringsverifikatet, också fiktiv moms vid omvänd \
                 skattskyldighet",
            )]),
        ),
        rad(
            F::LeverantörskreditInteBokförd,
            ruta([text(
                "**Kreditfaktura** från leverantör, skuld inte bokförd",
            )]),
            ruta([
                konto(LEVERANTÖRSSKULDER),
                text(" för den del som överstiger det obetalda"),
            ]),
            ruta([
                roll(R::Kostnad, "kostnad"),
                text(" och "),
                roll(R::IngåendeMoms, "moms"),
                text(" samma belopp"),
            ]),
            ruta([text(
                "kontantmetoden; kredit av en obetald del bokförs inte",
            )]),
        ),
        rad(
            F::ÅterbetalningFrånLeverantör,
            ruta([text("**Återbetalning** av tillgodohavande hos leverantör")]),
            ruta([roll(R::Betalkonto, "betalkontot")]),
            ruta([konto(LEVERANTÖRSSKULDER)]),
            tom(),
        ),
        rad(
            F::ÖresutjämningKund,
            ruta([text(format!(
                "**Öresutjämning** vid betalning av kundfaktura, rest ≤ {}",
                största_rest()
            ))]),
            ruta([konto(ÖRESUTJÄMNING), text(" resten")]),
            ruta([konto(KUNDFORDRINGAR), text(" resten")]),
            ruta([text(
                "fordran bokförd; i samma verifikat som betalningen; när fordran inte är bokförd \
                 stängs resten utan verifikat",
            )]),
        ),
        rad(
            F::ÖresutjämningLeverantör,
            ruta([text(format!(
                "**Öresutjämning** vid betalning av leverantörsfaktura, rest ≤ {}",
                största_rest()
            ))]),
            ruta([konto(LEVERANTÖRSSKULDER), text(" resten")]),
            ruta([konto(ÖRESUTJÄMNING), text(" resten")]),
            ruta([text(
                "skuld bokförd; i samma verifikat som betalningen; när skulden inte är bokförd \
                 stängs resten utan verifikat",
            )]),
        ),
        rad(
            F::Kursdifferens,
            ruta([text(
                "**Kursdifferens** vid betalning av leverantörsfaktura",
            )]),
            ruta([konto(KURSFÖRLUST), text(" vid förlust")]),
            ruta([konto(KURSVINST), text(" vid vinst")]),
            ruta([text(
                "fakturan har originalvaluta; i samma verifikat som betalningen",
            )]),
        ),
        rad(
            F::RättaIFlödet,
            ruta([text(
                "**Rätta kontering**, **Rätta avskrift**, **Rätta registrering**, **Rätta \
                 betalning**",
            )]),
            ruta([roll(
                R::Rättelse,
                "rättelse enligt etapp 1 av flödets verifikat",
            )]),
            tom(),
            ruta([text(
                "Rätta kontering bokför därtill ett nytt utfärdandeverifikat med dagens datum; \
                 de andra låser upp fakturan eller tar bort betalningen",
            )]),
        ),
        rad(
            F::T2,
            ruta([text("**T2-samlingsverifikat**")]),
            ruta([
                konto(KUNDFORDRINGAR),
                text(" för varje obetald kundfaktura; "),
                roll(R::Kostnad, "kostnad"),
                text(" och "),
                roll(R::IngåendeMoms, "ingående moms"),
                text(" (och "),
                roll(R::FiktivMoms, "fiktiv moms"),
                text(") proportionellt för varje obetald leverantörsfaktura"),
            ]),
            ruta([
                roll(R::Intäkt, "intäkt"),
                text(" och "),
                roll(R::UtgåendeMoms, "utgående moms"),
                text(" proportionellt; "),
                konto(LEVERANTÖRSSKULDER),
                text(" för varje obetald leverantörsfaktura"),
            ]),
            ruta([text(
                "kontantmetoden; datum = årets sista dag; utkast som Maya skapar; vänds inte",
            )]),
        ),
    ]
}

/// Styckena före tabellen, som Markdown, skrivna för den som läser Systemdokumentationen.
/// Tabellens rader är specens, ordagrant.
pub fn inledning() -> Vec<String> {
    let moms = satser_med_moms()
        .filter_map(|s| s.utgående_momskonto().map(|k| format!("{} ({s})", k.get())))
        .collect::<Vec<_>>();
    let fiktiv = fiktiv_utgående()
        .iter()
        .map(|k| k.get().to_string())
        .collect::<Vec<_>>();
    vec![
        "Tabellen visar vad Maya debiterar och krediterar i varje flöde, och när. Belopp är i \
         SEK. Saldo räknas med debet som positivt."
            .into(),
        format!(
            "*Netto* är radernas belopp exklusive moms, *moms* är momsbeloppet per sats. \
             Utgående moms bokförs på {}. Fiktiv utgående moms vid omvänd skattskyldighet \
             bokförs på {} efter momssats.",
            uppräkning(&moms),
            uppräkning(&fiktiv)
        ),
    ]
}

/// Stycket efter tabellen.
pub const AVSLUTNING: &str = "Senare versioner av Maya lägger till sina rader (moms, lön, \
     avskrivningar, stängning) i samma tabell.";

/// `a, b och c`.
fn uppräkning(delar: &[String]) -> String {
    match delar {
        [] => String::new(),
        [ensam] => ensam.clone(),
        [början @ .., sista] => format!("{} och {sista}", början.join(", ")),
    }
}

/// Hela avsnittet som Markdown: styckena och tabellen.
pub fn som_markdown() -> String {
    let mut ut = Vec::new();
    for stycke in inledning() {
        ut.push(stycke);
        ut.push(String::new());
    }
    ut.push(format!("| {} |", KOLUMNER.join(" | ")));
    ut.push(format!("|{}", "---|".repeat(KOLUMNER.len())));
    for r in tabell() {
        let celler = [&r.namn, &r.debet, &r.kredit, &r.villkor].map(Ruta::markdown);
        // En tom ruta skrivs `| |`, som i specen.
        let mut rad = String::from("|");
        for c in celler {
            if c.is_empty() {
                rad.push_str(" |");
            } else {
                rad.push_str(&format!(" {c} |"));
            }
        }
        ut.push(rad);
    }
    ut.push(String::new());
    ut.push(AVSLUTNING.into());
    ut.push(String::new());
    ut.join("\n")
}
