//! En leverantörsfaktura: formuläret, spärrarna vid registrering och konteringen under
//! fakturametoden, samma i webbläsaren och i servern (ADR 0003).
//!
//! Fälten fylls i i fakturans ordning: leverantör, fakturanummer, fakturadatum, förfallodag, OCR
//! eller meddelande, totalbelopp och originalvaluta i fritext. Varje rad har konto, netto,
//! momssats och ett momsbelopp som räknas ur nettot och kan skrivas över. Raderna med moms
//! summerar till totalen. Registreringen bokför kostnaden per rad netto och 2641 moms mot 2440
//! totalen, daterat på fakturadatumet; ett företag som inte är momsregistrerat på
//! fakturadatumet bokför momsen på kostnadskontot.
//!
//! Vid omvänd skattskyldighet (EU-tjänst, EU-vara, bygg eller tjänst från land utanför EU) har
//! fakturan ingen moms: totalen är radernas netto, och radens momsbelopp är den fiktiva moms som
//! köparen redovisar. Registreringen bokför den som avdragsgill ingående moms på 2645 (2647 för
//! bygg) och som utgående moms på 2614/2624/2634 efter momssats. Leverantörens förval föreslås
//! när fakturan inte har något val. Ett företag som inte är momsregistrerat kan inte välja
//! omvänd skattskyldighet. Justeringsparet mot 4599 kommer i etapp 3.
//!
//! **Kreditfaktura från leverantören** (#148): en faktura med negativt totalbelopp och rader med
//! samma tecken, med en frivillig hänvisning till originalet (en registrerad faktura från samma
//! leverantör). Konteringen är registreringens med omvänt tecken, också den fiktiva momsen: 2440
//! i debet mot kostnad och moms i kredit. Med en hänvisning kan raderna fyllas i ur originalets
//! rader, proportionellt vid delkredit ([`NyLeverantörsfaktura::kontera_som_originalet`]).
//!
//! **Upplåst** (#148, Rätta avskrift): en registrerad faktura vars registrering är vänd. Den
//! registreras om med samma regler, och totalen får inte bli mindre än det som redan har
//! betalats ([`Förutsättningar::betalt`]).

use std::fmt;

use jiff::civil::Date;

use crate::belopp::{Belopp, Saldo, formulärtext, tolka_med_tecken};
use crate::fordelning::fördela;
use crate::foretag::{Momsperiod, gäller};
use crate::kontoplan::Kontonummer;
use crate::leverantor::{self, NyLeverantör, OmvändSkattskyldighet};
use crate::rakenskapsar::Räkenskapsår;
use crate::verifikat::{self, Årsval};

/// Leverantörsskulder, som registreringen krediterar.
pub const LEVERANTÖRSSKULDER: u16 = 2440;
/// Debiterad ingående moms.
pub const INGÅENDE_MOMS: u16 = 2641;
/// Fiktiv ingående moms vid omvänd skattskyldighet för EU-tjänst, EU-vara och tjänst från
/// land utanför EU.
pub const FIKTIV_INGÅENDE_MOMS: u16 = 2645;
/// Fiktiv ingående moms vid omvänd skattskyldighet inom byggsektorn.
pub const FIKTIV_INGÅENDE_MOMS_BYGG: u16 = 2647;

/// Kontot för den fiktiva utgående momsen vid omvänd skattskyldighet med satsen `sats`
/// procent: 2614, 2624 eller 2634. Ingen för 0 %.
pub fn fiktiv_utgående_moms(sats: u8) -> Option<u16> {
    match sats {
        25 => Some(2614),
        12 => Some(2624),
        6 => Some(2634),
        _ => None,
    }
}

/// Kontot för den fiktiva ingående momsen, eller inget utan omvänd skattskyldighet.
pub fn fiktiv_ingående_moms(omvänd: OmvändSkattskyldighet) -> Option<u16> {
    match omvänd {
        OmvändSkattskyldighet::Ingen => None,
        OmvändSkattskyldighet::Bygg => Some(FIKTIV_INGÅENDE_MOMS_BYGG),
        OmvändSkattskyldighet::EuTjänst
        | OmvändSkattskyldighet::EuVara
        | OmvändSkattskyldighet::UtanförEu => Some(FIKTIV_INGÅENDE_MOMS),
    }
}

/// Momssatserna i procent, i den ordning väljaren visar dem.
pub const MOMSSATSER: [u8; 4] = [25, 12, 6, 0];
/// Leverantörsvalets värde för en ny leverantör som läggs upp från utkastet.
pub const NY: &str = "ny";

/// Momsen på `netto` med satsen `sats` procent, avrundad till närmaste öre (halva uppåt).
pub fn moms(netto: Belopp, sats: u8) -> Belopp {
    let öre = i128::from(netto.öre()) * i128::from(sats);
    Belopp::från_öre(i64::try_from((öre + 50) / 100).unwrap_or(i64::MAX))
}

/// Momsen på `netto` öre med satsen `sats` procent, avrundad till närmaste öre; ett negativt
/// netto (en kreditfaktura) avrundas som det positiva, med omvänt tecken.
pub fn moms_med_tecken(netto: i64, sats: u8) -> i64 {
    let m = moms(Belopp::från_öre(netto.abs()), sats).öre();
    if netto < 0 { -m } else { m }
}

/// En rad i formuläret, som text. Ett tomt momsbelopp räknas ur nettot och momssatsen.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NyRad {
    pub konto: String,
    pub netto: String,
    /// Procent: `25`, `12`, `6` eller `0`.
    pub momssats: String,
    pub moms: String,
}

impl NyRad {
    /// En rad utan konto, netto och moms räknas inte; momssatsen har alltid ett värde.
    pub fn tom(&self) -> bool {
        [&self.konto, &self.netto, &self.moms]
            .iter()
            .all(|f| f.trim().is_empty())
    }
}

/// Formuläret som användaren fyllde i, som text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NyLeverantörsfaktura {
    /// Id:t för en leverantör, [`NY`] för en ny leverantör från utkastet, eller tomt.
    pub leverantör: String,
    /// Den nya leverantören, när `leverantör` är [`NY`].
    pub ny_leverantör: NyLeverantör,
    pub fakturanummer: String,
    /// `ÅÅÅÅ-MM-DD`.
    pub fakturadatum: String,
    pub förfallodag: String,
    /// OCR eller meddelande.
    pub referens: String,
    /// Totalbeloppet i SEK.
    pub totalbelopp: String,
    /// Koden för en [`OmvändSkattskyldighet`]. Tomt: leverantörens förval.
    pub omvänd_skattskyldighet: String,
    /// Originalvaluta och originalbelopp i fritext, t.ex. `EUR 54,00`.
    pub originalvaluta: String,
    pub rader: Vec<NyRad>,
    /// Id:t för originalet som en kreditfaktura krediterar (#148), eller tomt.
    pub krediterar: String,
}

/// En leverantör som formuläret kan välja.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Leverantörsval {
    pub id: String,
    pub namn: String,
    pub standardkonto: Option<Kontonummer>,
    /// Leverantörens förvalda omvända skattskyldighet.
    pub omvänd_skattskyldighet: OmvändSkattskyldighet,
}

/// En annan registrerad faktura i företaget, som dubblettregeln prövas mot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registrerad {
    pub id: String,
    pub leverantör: String,
    pub fakturanummer: String,
    pub fakturadatum: Date,
    /// I öre; negativt för en kreditfaktura.
    pub total: i64,
}

/// En registrerad faktura som en kreditfaktura från samma leverantör kan hänvisa till (#148):
/// inte själv en kreditfaktura.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Krediterbar {
    pub id: String,
    pub leverantör: String,
    pub fakturanummer: String,
    /// I öre.
    pub total: i64,
    pub omvänd_skattskyldighet: OmvändSkattskyldighet,
    /// Raderna som registrerades.
    pub rader: Vec<Rad>,
}

/// Det som spärrarna prövas mot, läst i samma transaktion som registreringen.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Förutsättningar {
    /// Kontona i kontoplanen och om de är aktiva.
    pub konton: Vec<(Kontonummer, bool)>,
    pub räkenskapsår: Vec<Räkenskapsår>,
    /// Momsperiodens historik: från vilket datum varje momsperiod gäller.
    pub momsperioder: Vec<(Date, Momsperiod)>,
    /// Leverantörerna som kan väljas: de aktiva i registret.
    pub leverantörer: Vec<Leverantörsval>,
    /// Företagets andra registrerade leverantörsfakturor.
    pub registrerade: Vec<Registrerad>,
    /// Fakturorna som en kreditfaktura kan hänvisa till (#148).
    pub krediterbara: Vec<Krediterbar>,
    /// Det som redan har betalats av en upplåst faktura, i öre (#148): betalningarna i
    /// reskontran. Noll för ett utkast.
    pub betalt: i64,
}

impl Förutsättningar {
    /// Om företaget är momsregistrerat på `datum`.
    pub fn momsregistrerat(&self, datum: Date) -> bool {
        gäller(&self.momsperioder, datum)
            .or_else(|| self.momsperioder.first().map(|(_, m)| *m))
            .is_none_or(|m| m != Momsperiod::EjMomsregistrerad)
    }

    /// Om företaget är momsregistrerat på fakturadatumet i text, eller enligt den senaste
    /// momsperioden när datumet inte går att tolka.
    pub fn momsregistrerat_på(&self, fakturadatum: &str) -> bool {
        match fakturadatum.trim().parse::<Date>() {
            Ok(d) => self.momsregistrerat(d),
            Err(_) => self
                .momsperioder
                .iter()
                .max_by_key(|(d, _)| *d)
                .is_none_or(|(_, m)| *m != Momsperiod::EjMomsregistrerad),
        }
    }
}

/// Fältet som en spärr gäller. Raderna räknas från 0 i formulärets ordning, tomma medräknade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Fält {
    Leverantör,
    /// Ett fält i den nya leverantörens streckade kort.
    NyLeverantör(leverantor::Fält),
    Fakturanummer,
    Fakturadatum,
    Förfallodag,
    Totalbelopp,
    OmvändSkattskyldighet,
    /// Hänvisningen till originalet (#148).
    Krediterar,
    /// Raderna som helhet: inga rader, eller summan mot totalen.
    Rader,
    Konto(usize),
    Netto(usize),
    Momssats(usize),
    Moms(usize),
    /// Konteringens egna konton (2440, 2641 och den fiktiva momsens konton).
    Kontering,
}

/// En genväg som löser spärren.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Åtgärd {
    /// Byt fakturanumret till det här, med suffix ("Lägg till suffix -2").
    Fakturanummer(String),
    /// Aktivera kontot i kontoplanen.
    Aktivera(Kontonummer),
}

/// Något som hindrar registreringen, med en kod som formuläret visar vid fältet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spärr {
    pub fält: Fält,
    pub kod: &'static str,
    pub text: String,
    pub åtgärd: Option<Åtgärd>,
}

impl fmt::Display for Spärr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// Något som bör granskas men inte spärrar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Varning {
    pub kod: &'static str,
    pub text: String,
}

/// Spärrarna vid registrering som Systemdokumentationen återger (S2): koden och regeln. Varje kod
/// som [`NyLeverantörsfaktura::kontrollera`] kan ge står här, och ingen annan.
pub const REGISTRERINGSSPÄRRAR: &[(&str, &str)] = &[
    (
        "leverantor_saknas",
        "Fakturan har en leverantör ur registret eller en ny leverantör.",
    ),
    (
        "ny_leverantor",
        "En ny leverantör klarar samma regler som leverantörsregistret.",
    ),
    ("fakturanummer_saknas", "Leverantörens fakturanummer krävs."),
    (
        "dubblett",
        "Leverantören har ingen annan registrerad faktura med samma nummer.",
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
         senaste, som då skapas i samma transaktion.",
    ),
    ("forfallodag_saknas", "Förfallodag krävs."),
    (
        "forfallodag_ogiltig",
        "Förfallodagen skrivs som ÅÅÅÅ-MM-DD.",
    ),
    (
        "forfallodag_fore_fakturadatum",
        "Förfallodagen är inte före fakturadatumet.",
    ),
    ("total_saknas", "Totalbeloppet krävs."),
    (
        "total_ogiltig",
        "Totalbeloppet är i kronor och har högst två decimaler; negativt för en kreditfaktura.",
    ),
    (
        "under_betalt",
        "En upplåst faktura får ingen total under det som redan har betalats.",
    ),
    (
        "omvand_ogiltig",
        "Omvänd skattskyldighet är ingen, EU-tjänst, EU-vara, bygg eller tjänst från land \
         utanför EU.",
    ),
    (
        "krediterar_okand",
        "En kreditfaktura hänvisar till en registrerad faktura från samma leverantör, som inte \
         själv är en kreditfaktura.",
    ),
    (
        "kredit_inte_negativ",
        "En faktura med hänvisning till ett original är en kreditfaktura med negativ total.",
    ),
    (
        "omvand_ej_momsregistrerad",
        "Omvänd skattskyldighet väljs bara när företaget är momsregistrerat på fakturadatumet.",
    ),
    ("rader_saknas", "Fakturan har minst en konteringsrad."),
    (
        "konto_saknas",
        "Varje rad har ett konto, eller leverantörens standardkonto.",
    ),
    ("konto_okant", "Kontot finns i företagets kontoplan."),
    (
        "konto_inaktivt",
        "Radernas konton, 2440 och 2641 är aktiva, och vid omvänd skattskyldighet 2645 (2647 \
         för bygg) och 2614/2624/2634 efter momssats.",
    ),
    ("netto_saknas", "Varje rad har ett nettobelopp."),
    (
        "netto_ogiltigt",
        "Nettot är i kronor, har högst två decimaler och samma tecken som totalbeloppet.",
    ),
    ("momssats_ogiltig", "Momssatsen är 25, 12, 6 eller 0 %."),
    (
        "moms_ogiltig",
        "Momsbeloppet är i kronor, har högst två decimaler och samma tecken som totalbeloppet.",
    ),
    (
        "summerar_inte",
        "Radernas netto och moms summerar till totalbeloppet; vid omvänd skattskyldighet \
         summerar nettot till totalbeloppet.",
    ),
];

/// En kontrollerad rad.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rad {
    pub konto: Kontonummer,
    /// I öre; negativt på en kreditfaktura.
    pub netto: i64,
    pub momssats: u8,
    /// Momsbeloppet i öre: det beräknade eller det som skrevs över.
    pub moms: i64,
}

/// En leverantörsfaktura som klarar alla spärrar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registrerbar {
    /// Leverantörens id, eller [`NY`].
    pub leverantör: String,
    /// Den nya leverantören, när `leverantör` är [`NY`].
    pub ny_leverantör: Option<leverantor::GiltigLeverantör>,
    pub fakturanummer: String,
    pub fakturadatum: Date,
    pub förfallodag: Date,
    pub referens: String,
    /// I öre; negativt för en kreditfaktura.
    pub total: i64,
    pub originalvaluta: String,
    pub omvänd_skattskyldighet: OmvändSkattskyldighet,
    pub rader: Vec<Rad>,
    /// Originalets id när fakturan är en kreditfaktura med hänvisning (#148).
    pub krediterar: Option<String>,
    /// Om företaget är momsregistrerat på fakturadatumet.
    pub momsregistrerat: bool,
    pub räkenskapsår: Årsval,
}

impl Registrerbar {
    /// Registreringens verifikatrader under fakturametoden: kostnadskontot per rad netto (med
    /// momsen när företaget inte är momsregistrerat), 2641 momsen och 2440 totalen i kredit.
    /// Vid omvänd skattskyldighet är momsen fiktiv: 2645 (2647 för bygg) i debet och
    /// 2614/2624/2634 efter momssats i kredit, efter 2440.
    pub fn kontering(&self) -> Vec<verifikat::Rad> {
        kontering(
            &self.rader,
            self.momsregistrerat,
            self.omvänd_skattskyldighet,
        )
    }
}

fn konto(nr: u16) -> Kontonummer {
    Kontonummer::new(nr).expect("ett kontonummer i klass 1–8")
}

/// En verifikatrad med `öre` i debet, eller i kredit när det är negativt. En kreditfaktura får
/// på så sätt registreringens kontering med debet och kredit bytta.
fn verifikatrad(konto: Kontonummer, öre: i64) -> verifikat::Rad {
    verifikat::Rad {
        konto,
        debet: Belopp::från_öre(öre.max(0)),
        kredit: Belopp::från_öre((-öre).max(0)),
        text: None,
    }
}

fn kontering(
    rader: &[Rad],
    momsregistrerat: bool,
    omvänd: OmvändSkattskyldighet,
) -> Vec<verifikat::Rad> {
    if let Some(ingående) = fiktiv_ingående_moms(omvänd) {
        return omvänd_kontering(rader, ingående);
    }
    let mut ut = Vec::new();
    let mut moms = 0;
    let mut total = 0;
    for r in rader {
        let kostnad = if momsregistrerat {
            moms += r.moms;
            r.netto
        } else {
            r.netto + r.moms
        };
        total += r.netto + r.moms;
        if kostnad != 0 {
            ut.push(verifikatrad(r.konto, kostnad));
        }
    }
    if moms != 0 {
        ut.push(verifikatrad(konto(INGÅENDE_MOMS), moms));
    }
    if total != 0 {
        ut.push(verifikatrad(konto(LEVERANTÖRSSKULDER), -total));
    }
    ut
}

/// Konteringen vid omvänd skattskyldighet: kostnaden per rad netto och den fiktiva ingående
/// momsen på `ingående` i debet, 2440 nettot och den fiktiva utgående momsen per momssats i
/// kredit. En rad med 0 % ger ingen fiktiv moms.
fn omvänd_kontering(rader: &[Rad], ingående: u16) -> Vec<verifikat::Rad> {
    let mut ut: Vec<verifikat::Rad> = rader
        .iter()
        .filter(|r| r.netto != 0)
        .map(|r| verifikatrad(r.konto, r.netto))
        .collect();
    let fiktiv = |sats: u8| -> i64 {
        rader
            .iter()
            .filter(|r| r.momssats == sats)
            .map(|r| r.moms)
            .sum()
    };
    let utgående: Vec<(u16, i64)> = MOMSSATSER
        .into_iter()
        .filter_map(|s| Some((fiktiv_utgående_moms(s)?, fiktiv(s))))
        .filter(|(_, öre)| *öre != 0)
        .collect();
    let moms: i64 = utgående.iter().map(|(_, öre)| öre).sum();
    if moms != 0 {
        ut.push(verifikatrad(konto(ingående), moms));
    }
    let total: i64 = rader.iter().map(|r| r.netto).sum();
    if total != 0 {
        ut.push(verifikatrad(konto(LEVERANTÖRSSKULDER), -total));
    }
    ut.extend(
        utgående
            .into_iter()
            .map(|(nr, öre)| verifikatrad(konto(nr), -öre)),
    );
    ut
}

/// En rad som går att spara i ett utkast: det som är ifyllt går att lagra.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Utkastrad {
    pub konto: Option<Kontonummer>,
    /// I öre.
    pub netto: Option<i64>,
    pub momssats: u8,
    /// I öre; `None`: räknas ur nettot.
    pub moms: Option<i64>,
}

/// Ett utkast som går att spara.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SparbartUtkast {
    pub fakturanummer: String,
    pub fakturadatum: Option<Date>,
    pub förfallodag: Option<Date>,
    pub referens: String,
    /// I öre.
    pub total: Option<i64>,
    /// Det som valdes på fakturan; inget när leverantörens förval gäller.
    pub omvänd_skattskyldighet: Option<OmvändSkattskyldighet>,
    pub originalvaluta: String,
    pub rader: Vec<Utkastrad>,
    /// Originalets id, för en kreditfaktura med hänvisning (#148).
    pub krediterar: Option<i64>,
}

fn spärr(fält: Fält, kod: &'static str, text: impl ToString) -> Spärr {
    Spärr {
        fält,
        kod,
        text: text.to_string(),
        åtgärd: None,
    }
}

fn tolka_datum(
    text: &str,
    fält: Fält,
    ogiltigt: &'static str,
    spärrar: &mut Vec<Spärr>,
) -> Option<Date> {
    text.trim()
        .parse::<Date>()
        .map_err(|_| spärrar.push(spärr(fält, ogiltigt, "Ange ett datum som ÅÅÅÅ-MM-DD.")))
        .ok()
}

/// Ett belopp i öre med tecken; en kreditfaktura har negativa belopp (#148).
fn tolka_belopp(
    text: &str,
    fält: Fält,
    ogiltigt: &'static str,
    spärrar: &mut Vec<Spärr>,
) -> Option<i64> {
    tolka_med_tecken(text)
        .ok_or_else(|| {
            spärrar.push(spärr(
                fält,
                ogiltigt,
                "Ange ett belopp i kronor med högst två decimaler.",
            ))
        })
        .ok()
}

fn tolka_sats(text: &str, i: usize, spärrar: &mut Vec<Spärr>) -> Option<u8> {
    let text = text.trim().trim_end_matches('%').trim();
    let sats = if text.is_empty() {
        Some(25)
    } else {
        text.parse::<u8>().ok().filter(|s| MOMSSATSER.contains(s))
    };
    if sats.is_none() {
        spärrar.push(spärr(
            Fält::Momssats(i),
            "momssats_ogiltig",
            "Välj 25, 12, 6 eller 0 %.",
        ));
    }
    sats
}

impl NyLeverantörsfaktura {
    /// Leverantörens standardkonto: den valda leverantörens, eller den nya leverantörens om det
    /// går att tolka.
    pub fn standardkonto(&self, f: &Förutsättningar) -> Option<Kontonummer> {
        if self.leverantör.trim() == NY {
            return self.ny_leverantör.standardkonto.trim().parse().ok();
        }
        f.leverantörer
            .iter()
            .find(|l| l.id == self.leverantör.trim())
            .and_then(|l| l.standardkonto)
    }

    /// Prövar spärrarna. Spärrarna kommer i fältens ordning.
    pub fn kontrollera(&self, f: &Förutsättningar) -> Result<Registrerbar, Vec<Spärr>> {
        let mut spärrar = Vec::new();
        let leverantör = self.leverantör.trim().to_string();
        let ny_leverantör = if leverantör == NY {
            match self.ny_leverantör.kontrollera(&f.konton) {
                Ok(l) => Some(l),
                Err(fel) => {
                    spärrar.extend(fel.into_iter().map(|fel| {
                        spärr(Fält::NyLeverantör(fel.fält), "ny_leverantor", fel.text)
                    }));
                    None
                }
            }
        } else {
            if !f.leverantörer.iter().any(|l| l.id == leverantör) {
                spärrar.push(spärr(
                    Fält::Leverantör,
                    "leverantor_saknas",
                    "Välj leverantör, eller lägg upp en ny.",
                ));
            }
            None
        };

        let fakturanummer = self.fakturanummer.trim().to_string();
        if fakturanummer.is_empty() {
            spärrar.push(spärr(
                Fält::Fakturanummer,
                "fakturanummer_saknas",
                "Fakturanummer saknas.",
            ));
        } else if let Some(annan) = self.dubblett(f) {
            spärrar.push(Spärr {
                åtgärd: Some(Åtgärd::Fakturanummer(self.med_suffix(f))),
                ..spärr(
                    Fält::Fakturanummer,
                    "dubblett",
                    format!(
                        "{} har redan faktura {} (fakturadatum {}). Om leverantören har \
                         återanvänt numret, lägg till ett suffix.",
                        self.leverantörens_namn(f),
                        annan.fakturanummer,
                        annan.fakturadatum
                    ),
                )
            });
        }

        let fakturadatum = if self.fakturadatum.trim().is_empty() {
            spärrar.push(spärr(
                Fält::Fakturadatum,
                "fakturadatum_saknas",
                "Fakturadatum saknas.",
            ));
            None
        } else {
            tolka_datum(
                &self.fakturadatum,
                Fält::Fakturadatum,
                "fakturadatum_ogiltigt",
                &mut spärrar,
            )
        };
        let årsval = fakturadatum.and_then(|d| {
            let mut fel = Vec::new();
            let år = verifikat::välj_år(d, &f.räkenskapsår, &mut fel);
            spärrar.extend(
                fel.into_iter()
                    .map(|s| spärr(Fält::Fakturadatum, s.kod, s.text)),
            );
            år
        });
        let förfallodag = if self.förfallodag.trim().is_empty() {
            spärrar.push(spärr(
                Fält::Förfallodag,
                "forfallodag_saknas",
                "Förfallodag saknas.",
            ));
            None
        } else {
            tolka_datum(
                &self.förfallodag,
                Fält::Förfallodag,
                "forfallodag_ogiltig",
                &mut spärrar,
            )
        };
        if let (Some(fd), Some(ff)) = (fakturadatum, förfallodag)
            && ff < fd
        {
            spärrar.push(spärr(
                Fält::Förfallodag,
                "forfallodag_fore_fakturadatum",
                "Förfallodagen är före fakturadatumet.",
            ));
        }

        let total = if self.totalbelopp.trim().is_empty() {
            spärrar.push(spärr(
                Fält::Totalbelopp,
                "total_saknas",
                "Totalbelopp saknas.",
            ));
            None
        } else {
            tolka_belopp(
                &self.totalbelopp,
                Fält::Totalbelopp,
                "total_ogiltig",
                &mut spärrar,
            )
        };
        // #148: en upplåst faktura får ingen total under det som redan har betalats, åt något
        // håll: en kreditfaktura med en återbetalning blir inte mindre än den.
        if let Some(t) = total
            && ((f.betalt > 0 && t < f.betalt) || (f.betalt < 0 && t > f.betalt))
        {
            spärrar.push(spärr(
                Fält::Totalbelopp,
                "under_betalt",
                format!(
                    "Totalen är under det som redan har betalats, {} kr.",
                    Saldo::från_öre(f.betalt)
                ),
            ));
        }
        let kredit = total.is_some_and(|t| t < 0);
        let krediterar = self.kontrollera_hänvisning(f, total, &mut spärrar);

        let vald = self.vald_omvänd(&mut spärrar);
        let momsregistrerat = fakturadatum.is_none_or(|d| f.momsregistrerat(d));
        let omvänd = match vald {
            Some(o) if o != OmvändSkattskyldighet::Ingen && !momsregistrerat => {
                spärrar.push(spärr(
                    Fält::OmvändSkattskyldighet,
                    "omvand_ej_momsregistrerad",
                    "Företaget är inte momsregistrerat på fakturadatumet och kan inte välja \
                     omvänd skattskyldighet.",
                ));
                OmvändSkattskyldighet::Ingen
            }
            Some(o) => o,
            None if momsregistrerat => self.förval(f),
            None => OmvändSkattskyldighet::Ingen,
        };
        let fiktiv = fiktiv_ingående_moms(omvänd);

        let standardkonto = self.standardkonto(f);
        let ifyllda: Vec<(usize, &NyRad)> = self
            .rader
            .iter()
            .enumerate()
            .filter(|(_, r)| !r.tom())
            .collect();
        if ifyllda.is_empty() {
            spärrar.push(spärr(
                Fält::Rader,
                "rader_saknas",
                "Lägg till minst en rad med konto och netto.",
            ));
        }
        let mut rader = Vec::new();
        let mut radspärrar = Vec::new();
        for (i, r) in &ifyllda {
            let konto = kontrollera_konto(*i, &r.konto, standardkonto, &f.konton, &mut radspärrar);
            let netto = if r.netto.trim().is_empty() {
                radspärrar.push(spärr(Fält::Netto(*i), "netto_saknas", "Ange nettot."));
                None
            } else {
                tolka_belopp(&r.netto, Fält::Netto(*i), "netto_ogiltigt", &mut radspärrar)
                    .and_then(|n| med_tecken(n, kredit, Fält::Netto(*i), &mut radspärrar))
            };
            let sats = tolka_sats(&r.momssats, *i, &mut radspärrar);
            let moms_ = match (r.moms.trim(), netto, sats) {
                ("", Some(n), Some(s)) => Some(moms_med_tecken(n, s)),
                ("", _, _) => None,
                (text, _, _) => tolka_belopp(text, Fält::Moms(*i), "moms_ogiltig", &mut radspärrar)
                    .and_then(|m| med_tecken(m, kredit, Fält::Moms(*i), &mut radspärrar)),
            };
            if let (Some(konto), Some(netto), Some(momssats), Some(moms)) =
                (konto, netto, sats, moms_)
            {
                rader.push(Rad {
                    konto,
                    netto,
                    momssats,
                    moms,
                });
            }
        }
        spärrar.extend(radspärrar);
        if let Some(total) = total
            && !ifyllda.is_empty()
            && rader.len() == ifyllda.len()
        {
            let summa: i64 = rader
                .iter()
                .map(|r| r.netto + if fiktiv.is_some() { 0 } else { r.moms })
                .sum();
            if summa != total {
                let differens = Belopp::från_öre((summa - total).abs());
                let vad = if fiktiv.is_some() {
                    "Raderna"
                } else {
                    "Raderna med moms"
                };
                spärrar.push(spärr(
                    Fält::Rader,
                    "summerar_inte",
                    format!(
                        "{vad} blir {} kr, totalen är {} kr. Differens {differens}.",
                        Saldo::från_öre(summa),
                        Saldo::från_öre(total)
                    ),
                ));
            }
        }

        let med_moms = |r: &&Rad| r.moms != 0;
        let mut behövs = vec![LEVERANTÖRSSKULDER];
        match fiktiv {
            Some(ingående) => {
                if rader
                    .iter()
                    .filter(med_moms)
                    .any(|r| fiktiv_utgående_moms(r.momssats).is_some())
                {
                    behövs.push(ingående);
                }
                behövs.extend(MOMSSATSER.into_iter().filter_map(|s| {
                    rader
                        .iter()
                        .filter(med_moms)
                        .any(|r| r.momssats == s)
                        .then_some(fiktiv_utgående_moms(s))
                        .flatten()
                }));
            }
            None if momsregistrerat && rader.iter().any(|r| r.moms != 0) => {
                behövs.push(INGÅENDE_MOMS);
            }
            None => {}
        }
        for nr in behövs {
            let konto = Kontonummer::new(nr).expect("ett kontonummer i klass 1–8");
            if !f.konton.iter().any(|(k, aktiv)| *k == konto && *aktiv) {
                spärrar.push(Spärr {
                    åtgärd: Some(Åtgärd::Aktivera(konto)),
                    ..spärr(
                        Fält::Kontering,
                        "konto_inaktivt",
                        format!(
                            "Konto {konto} är inaktivt eller saknas. Registreringen bokför på det."
                        ),
                    )
                });
            }
        }

        match (fakturadatum, förfallodag, total, årsval) {
            (Some(fakturadatum), Some(förfallodag), Some(total), Some(räkenskapsår))
                if spärrar.is_empty() =>
            {
                Ok(Registrerbar {
                    leverantör,
                    ny_leverantör,
                    fakturanummer,
                    fakturadatum,
                    förfallodag,
                    referens: self.referens.trim().to_string(),
                    total,
                    originalvaluta: self.originalvaluta.trim().to_string(),
                    omvänd_skattskyldighet: omvänd,
                    rader,
                    krediterar,
                    momsregistrerat,
                    räkenskapsår,
                })
            }
            _ => Err(spärrar),
        }
    }

    /// Varningarna, som inte spärrar: en annan registrerad faktura från samma leverantör med
    /// samma belopp och fakturadatum men ett annat nummer.
    pub fn varningar(&self, f: &Förutsättningar) -> Vec<Varning> {
        let (Ok(datum), Some(total)) = (
            self.fakturadatum.trim().parse::<Date>(),
            tolka_med_tecken(&self.totalbelopp),
        ) else {
            return Vec::new();
        };
        let nummer = self.fakturanummer.trim();
        f.registrerade
            .iter()
            .filter(|r| {
                r.leverantör == self.leverantör.trim()
                    && r.fakturadatum == datum
                    && r.total == total
                    && !r.fakturanummer.eq_ignore_ascii_case(nummer)
            })
            .map(|r| Varning {
                kod: "trolig_dubblett",
                text: format!(
                    "{} har en faktura med samma belopp och datum men nummer {}. Är det samma \
                     faktura?",
                    self.leverantörens_namn(f),
                    r.fakturanummer
                ),
            })
            .collect()
    }

    /// Konteringen så långt den går att räkna, för rutan "Bokförs som" medan användaren skriver.
    /// Rader som inte går att tolka hoppas över.
    pub fn förhandsvisning(&self, f: &Förutsättningar) -> Vec<verifikat::Rad> {
        let standardkonto = self.standardkonto(f);
        let rader: Vec<Rad> = self
            .rader
            .iter()
            .filter(|r| !r.tom())
            .filter_map(|r| {
                let konto = match r.konto.trim() {
                    "" => standardkonto?,
                    text => text.parse().ok()?,
                };
                let netto = tolka_med_tecken(&r.netto)?;
                let sats = tolka_sats(&r.momssats, 0, &mut Vec::new())?;
                let moms_ = match r.moms.trim() {
                    "" => moms_med_tecken(netto, sats),
                    text => tolka_med_tecken(text)?,
                };
                Some(Rad {
                    konto,
                    netto,
                    momssats: sats,
                    moms: moms_,
                })
            })
            .collect();
        let momsregistrerat = self
            .fakturadatum
            .trim()
            .parse::<Date>()
            .map_or(true, |d| f.momsregistrerat(d));
        let omvänd = if momsregistrerat {
            self.omvänd_skattskyldighet(f)
        } else {
            OmvändSkattskyldighet::Ingen
        };
        kontering(&rader, momsregistrerat, omvänd)
    }

    /// Den omvända skattskyldighet som fakturan har: den som valdes, eller leverantörens förval
    /// när inget är valt och företaget är momsregistrerat på fakturadatumet. En okänd kod är
    /// ingen.
    pub fn omvänd_skattskyldighet(&self, f: &Förutsättningar) -> OmvändSkattskyldighet {
        match self.omvänd_skattskyldighet.trim() {
            "" if f.momsregistrerat_på(&self.fakturadatum) => self.förval(f),
            "" => OmvändSkattskyldighet::Ingen,
            kod => OmvändSkattskyldighet::från_kod(kod).unwrap_or_default(),
        }
    }

    /// Leverantörens förval: den valda leverantörens, eller den nya leverantörens.
    fn förval(&self, f: &Förutsättningar) -> OmvändSkattskyldighet {
        if self.leverantör.trim() == NY {
            return OmvändSkattskyldighet::från_kod(
                self.ny_leverantör.omvänd_skattskyldighet.trim(),
            )
            .unwrap_or_default();
        }
        f.leverantörer
            .iter()
            .find(|l| l.id == self.leverantör.trim())
            .map(|l| l.omvänd_skattskyldighet)
            .unwrap_or_default()
    }

    /// Valet på fakturan, inget när det är tomt; en okänd kod spärrar.
    fn vald_omvänd(&self, spärrar: &mut Vec<Spärr>) -> Option<OmvändSkattskyldighet> {
        match self.omvänd_skattskyldighet.trim() {
            "" => None,
            kod => {
                let o = OmvändSkattskyldighet::från_kod(kod);
                if o.is_none() {
                    spärrar.push(spärr(
                        Fält::OmvändSkattskyldighet,
                        "omvand_ogiltig",
                        "Välj omvänd skattskyldighet.",
                    ));
                }
                o
            }
        }
    }

    /// Prövar det som måste gälla för att utkastet ska kunna sparas: datum som är datum, belopp
    /// som är belopp, kontonummer som är kontonummer och momssatser som finns. Allt annat får
    /// saknas. Spärrarna har samma koder som vid registrering.
    pub fn som_utkast(&self) -> Result<SparbartUtkast, Vec<Spärr>> {
        let mut spärrar = Vec::new();
        let datum = |text: &str, fält, kod, spärrar: &mut Vec<Spärr>| match text.trim() {
            "" => None,
            text => tolka_datum(text, fält, kod, spärrar),
        };
        let fakturadatum = datum(
            &self.fakturadatum,
            Fält::Fakturadatum,
            "fakturadatum_ogiltigt",
            &mut spärrar,
        );
        let förfallodag = datum(
            &self.förfallodag,
            Fält::Förfallodag,
            "forfallodag_ogiltig",
            &mut spärrar,
        );
        let belopp = |text: &str, fält, kod, spärrar: &mut Vec<Spärr>| match text.trim() {
            "" => None,
            text => tolka_belopp(text, fält, kod, spärrar),
        };
        let total = belopp(
            &self.totalbelopp,
            Fält::Totalbelopp,
            "total_ogiltig",
            &mut spärrar,
        );
        let omvänd_skattskyldighet = self.vald_omvänd(&mut spärrar);
        let krediterar = match self.krediterar.trim() {
            "" => None,
            text => {
                let id = text.parse::<i64>().ok();
                if id.is_none() {
                    spärrar.push(spärr(
                        Fält::Krediterar,
                        "krediterar_okand",
                        "Välj fakturan som krediteras.",
                    ));
                }
                id
            }
        };
        let mut rader = Vec::new();
        for (i, r) in self.rader.iter().enumerate().filter(|(_, r)| !r.tom()) {
            let konto = match r.konto.trim() {
                "" => None,
                text => {
                    let konto = text.parse::<Kontonummer>().ok();
                    if konto.is_none() {
                        spärrar.push(spärr(
                            Fält::Konto(i),
                            "konto_okant",
                            format!("Konto {text} finns inte i kontoplanen."),
                        ));
                    }
                    konto
                }
            };
            let netto = belopp(&r.netto, Fält::Netto(i), "netto_ogiltigt", &mut spärrar);
            let momssats = tolka_sats(&r.momssats, i, &mut spärrar).unwrap_or(25);
            let moms = belopp(&r.moms, Fält::Moms(i), "moms_ogiltig", &mut spärrar);
            rader.push(Utkastrad {
                konto,
                netto,
                momssats,
                moms,
            });
        }
        if !spärrar.is_empty() {
            return Err(spärrar);
        }
        Ok(SparbartUtkast {
            fakturanummer: self.fakturanummer.trim().to_string(),
            fakturadatum,
            förfallodag,
            referens: self.referens.trim().to_string(),
            total,
            omvänd_skattskyldighet,
            originalvaluta: self.originalvaluta.trim().to_string(),
            rader,
            krediterar,
        })
    }

    /// Prövar hänvisningen till originalet (#148): den finns bland de krediterbara fakturorna
    /// från samma leverantör, och fakturan är då en kreditfaktura med negativ total.
    fn kontrollera_hänvisning(
        &self,
        f: &Förutsättningar,
        total: Option<i64>,
        spärrar: &mut Vec<Spärr>,
    ) -> Option<String> {
        let id = self.krediterar.trim();
        if id.is_empty() {
            return None;
        }
        if total.is_some_and(|t| t >= 0) {
            spärrar.push(spärr(
                Fält::Krediterar,
                "kredit_inte_negativ",
                "En kreditfaktura har en negativ total. Ta bort hänvisningen eller skriv totalen \
                 med minus.",
            ));
        }
        match self.originalet(f) {
            Some(o) => Some(o.id.clone()),
            None => {
                spärrar.push(spärr(
                    Fält::Krediterar,
                    "krediterar_okand",
                    "Fakturan som krediteras är inte en registrerad faktura från leverantören.",
                ));
                None
            }
        }
    }

    /// Originalet som hänvisningen pekar på, från samma leverantör.
    pub fn originalet<'a>(&self, f: &'a Förutsättningar) -> Option<&'a Krediterbar> {
        f.krediterbara
            .iter()
            .find(|k| k.id == self.krediterar.trim() && k.leverantör == self.leverantör.trim())
    }

    /// Raderna ur originalet (#148), för en kreditfaktura med hänvisning och negativ total:
    /// totalen fördelas på originalets rader i proportion till deras andel av originalets total,
    /// och varje rads del på netto och moms på samma sätt, avrundat så att summan stämmer exakt
    /// ([`fördela`]). Vid omvänd skattskyldighet är totalen nettot, och den fiktiva momsen
    /// fördelas i samma proportion. Fakturan får originalets omvända skattskyldighet. `None`
    /// utan hänvisning, utan original eller utan negativ total.
    pub fn kontera_som_originalet(&self, f: &Förutsättningar) -> Option<NyLeverantörsfaktura> {
        let kredit = tolka_med_tecken(&self.totalbelopp).filter(|t| *t < 0)?;
        let o = self.originalet(f)?;
        if o.total <= 0 || o.rader.is_empty() {
            return None;
        }
        let fiktiv = fiktiv_ingående_moms(o.omvänd_skattskyldighet).is_some();
        let (netto, moms): (Vec<i64>, Vec<i64>) = if fiktiv {
            let nettovikter: Vec<i64> = o.rader.iter().map(|r| r.netto).collect();
            let momsvikter: Vec<i64> = o.rader.iter().map(|r| r.moms).collect();
            // Den fiktiva momsen ingår inte i totalen: krediten av den är originalets fiktiva
            // moms gånger krediten delad med originalets netto, avrundad till närmaste öre.
            let summa_moms: i64 = momsvikter.iter().sum();
            let täljare = i128::from(kredit.abs()) * i128::from(summa_moms) * 2;
            let nämnare = i128::from(o.total) * 2;
            let moms_kredit = -i64::try_from((täljare + nämnare / 2) / nämnare).unwrap_or(0);
            (
                fördela(kredit, &nettovikter),
                fördela(moms_kredit, &momsvikter),
            )
        } else {
            let radtotaler = fördela(
                kredit,
                &o.rader.iter().map(|r| r.netto + r.moms).collect::<Vec<_>>(),
            );
            o.rader
                .iter()
                .zip(radtotaler)
                .map(|(r, t)| {
                    let delar = fördela(t, &[r.netto, r.moms]);
                    (delar[0], delar[1])
                })
                .unzip()
        };
        let rader = o
            .rader
            .iter()
            .zip(netto.into_iter().zip(moms))
            .map(|(r, (n, m))| NyRad {
                konto: r.konto.get().to_string(),
                netto: formulärtext(n),
                momssats: r.momssats.to_string(),
                moms: formulärtext(m),
            })
            .collect();
        Some(NyLeverantörsfaktura {
            omvänd_skattskyldighet: o.omvänd_skattskyldighet.kod().to_string(),
            rader,
            ..self.clone()
        })
    }

    fn leverantörens_namn(&self, f: &Förutsättningar) -> String {
        if self.leverantör.trim() == NY {
            return self.ny_leverantör.namn.trim().to_string();
        }
        f.leverantörer
            .iter()
            .find(|l| l.id == self.leverantör.trim())
            .map_or_else(|| "Leverantören".to_string(), |l| l.namn.clone())
    }

    fn tagna(&self, f: &Förutsättningar) -> Vec<String> {
        f.registrerade
            .iter()
            .filter(|r| r.leverantör == self.leverantör.trim())
            .map(|r| r.fakturanummer.to_lowercase())
            .collect()
    }

    fn dubblett<'a>(&self, f: &'a Förutsättningar) -> Option<&'a Registrerad> {
        let nummer = self.fakturanummer.trim();
        f.registrerade.iter().find(|r| {
            r.leverantör == self.leverantör.trim() && r.fakturanummer.eq_ignore_ascii_case(nummer)
        })
    }

    /// Fakturanumret med det första lediga suffixet: `-2`, `-3` …
    fn med_suffix(&self, f: &Förutsättningar) -> String {
        let tagna = self.tagna(f);
        let nummer = self.fakturanummer.trim();
        (2..)
            .map(|n| format!("{nummer}-{n}"))
            .find(|n| !tagna.contains(&n.to_lowercase()))
            .expect("ett ledigt suffix")
    }
}

/// `öre` om det har samma tecken som fakturan (negativt eller noll på en kreditfaktura, annars
/// noll eller positivt), annars en spärr med fältets kod.
fn med_tecken(öre: i64, kredit: bool, fält: Fält, spärrar: &mut Vec<Spärr>) -> Option<i64> {
    if (kredit && öre > 0) || (!kredit && öre < 0) {
        let kod = match fält {
            Fält::Moms(_) => "moms_ogiltig",
            _ => "netto_ogiltigt",
        };
        spärrar.push(spärr(
            fält,
            kod,
            if kredit {
                "En kreditfaktura har negativa belopp på raderna."
            } else {
                "Beloppet är negativt men totalen är det inte. En kreditfaktura har negativ total."
            },
        ));
        return None;
    }
    Some(öre)
}

fn kontrollera_konto(
    i: usize,
    text: &str,
    standardkonto: Option<Kontonummer>,
    konton: &[(Kontonummer, bool)],
    spärrar: &mut Vec<Spärr>,
) -> Option<Kontonummer> {
    let text = text.trim();
    let konto = if text.is_empty() {
        match standardkonto {
            Some(k) => k,
            None => {
                spärrar.push(spärr(Fält::Konto(i), "konto_saknas", "Välj ett konto."));
                return None;
            }
        }
    } else {
        match text.parse::<Kontonummer>() {
            Ok(k) => k,
            Err(_) => {
                spärrar.push(spärr(
                    Fält::Konto(i),
                    "konto_okant",
                    format!("Konto {text} finns inte i kontoplanen."),
                ));
                return None;
            }
        }
    };
    match konton.iter().find(|(k, _)| *k == konto) {
        Some((_, true)) => Some(konto),
        Some((_, false)) => {
            spärrar.push(Spärr {
                åtgärd: Some(Åtgärd::Aktivera(konto)),
                ..spärr(
                    Fält::Konto(i),
                    "konto_inaktivt",
                    format!("Konto {konto} är inaktivt. Aktivera det för att bokföra på det."),
                )
            });
            None
        }
        None => {
            spärrar.push(spärr(
                Fält::Konto(i),
                "konto_okant",
                format!("Konto {konto} finns inte i kontoplanen."),
            ));
            None
        }
    }
}
