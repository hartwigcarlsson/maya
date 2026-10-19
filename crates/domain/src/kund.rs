//! Kundregistret: en kunds uppgifter, som kundfakturan kopierar in när den utfärdas. Samma
//! kontroll i formuläret och på servern (ADR 0003).

use std::fmt;

use crate::orgnr::{Organisationsnummer, OrgnrFel};

/// Omvänd skattskyldighet som kunden har som förval på sina fakturor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum OmvändSkattskyldighet {
    Ingen,
    /// Byggtjänster inom Sverige.
    Bygg,
    /// Tjänst till en näringsidkare i ett annat EU-land.
    EuTjänst,
}

impl OmvändSkattskyldighet {
    pub const ALLA: [OmvändSkattskyldighet; 3] = [
        OmvändSkattskyldighet::Ingen,
        OmvändSkattskyldighet::Bygg,
        OmvändSkattskyldighet::EuTjänst,
    ];

    /// Koden i formuläret och i databasen.
    pub fn kod(self) -> &'static str {
        match self {
            OmvändSkattskyldighet::Ingen => "ingen",
            OmvändSkattskyldighet::Bygg => "bygg",
            OmvändSkattskyldighet::EuTjänst => "eu_tjanst",
        }
    }

    pub fn från_kod(kod: &str) -> Option<Self> {
        Self::ALLA.into_iter().find(|o| o.kod() == kod)
    }

    pub fn namn(self) -> &'static str {
        match self {
            OmvändSkattskyldighet::Ingen => "Ingen",
            OmvändSkattskyldighet::Bygg => "Bygg inom Sverige",
            OmvändSkattskyldighet::EuTjänst => "EU-tjänst",
        }
    }
}

/// Fälten i kundformuläret.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Fält {
    Namn,
    Identitet,
    Adress,
    Epost,
    Momsregnr,
    Betalningsvillkor,
    OmvändSkattskyldighet,
}

impl Fält {
    /// Fältets namn i formuläret.
    pub fn namn(self) -> &'static str {
        match self {
            Fält::Namn => "namn",
            Fält::Identitet => "identitet",
            Fält::Adress => "adress",
            Fält::Epost => "epost",
            Fält::Momsregnr => "momsregnr",
            Fält::Betalningsvillkor => "betalningsvillkor",
            Fält::OmvändSkattskyldighet => "omvand_skattskyldighet",
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

/// Kundformuläret som användaren fyllde i, som text.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NyKund {
    pub namn: String,
    /// Organisationsnummer eller personnummer.
    pub identitet: String,
    /// Gatuadress, postnummer och ort, en eller flera rader.
    pub adress: String,
    pub epost: String,
    pub momsregnr: String,
    /// Antal dagar från fakturadatum till förfallodag.
    pub betalningsvillkor: String,
    /// En kod ur [`OmvändSkattskyldighet::kod`].
    pub omvänd_skattskyldighet: String,
}

impl Default for NyKund {
    /// Ett tomt formulär med 30 dagars betalningsvillkor och ingen omvänd skattskyldighet.
    fn default() -> Self {
        NyKund {
            namn: String::new(),
            identitet: String::new(),
            adress: String::new(),
            epost: String::new(),
            momsregnr: String::new(),
            betalningsvillkor: "30".into(),
            omvänd_skattskyldighet: OmvändSkattskyldighet::Ingen.kod().into(),
        }
    }
}

/// En kontrollerad kund. Tom text betyder att uppgiften saknas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GiltigKund {
    pub namn: String,
    /// `NNNNNN-NNNN` för ett organisationsnummer, `ÅÅMMDD-NNNN` eller `ÅÅÅÅMMDD-NNNN` för ett
    /// personnummer.
    pub identitet: String,
    pub adress: String,
    pub epost: String,
    /// Utan blanktecken och bindestreck, med stora bokstäver. Ett utländskt nummer godtas.
    pub momsregnr: String,
    pub betalningsvillkor: u16,
    pub omvänd_skattskyldighet: OmvändSkattskyldighet,
}

impl GiltigKund {
    /// Kunden som formulär, för att fortsätta skriva på den.
    pub fn som_formulär(&self) -> NyKund {
        NyKund {
            namn: self.namn.clone(),
            identitet: self.identitet.clone(),
            adress: self.adress.clone(),
            epost: self.epost.clone(),
            momsregnr: self.momsregnr.clone(),
            betalningsvillkor: self.betalningsvillkor.to_string(),
            omvänd_skattskyldighet: self.omvänd_skattskyldighet.kod().into(),
        }
    }
}

impl NyKund {
    /// Kontrollerar formuläret. Bara namnet krävs; det som fylls i måste vara rätt. Felen kommer
    /// i fältens ordning.
    pub fn kontrollera(&self) -> Result<GiltigKund, Vec<Fältfel>> {
        let mut fel = Vec::new();
        let namn = self.namn.trim().to_string();
        if namn.is_empty() {
            fel.push(Fältfel::nytt(
                Fält::Namn,
                "namn_saknas",
                "Kunden måste ha ett namn.",
            ));
        }
        let identitet = match self.identitet.trim() {
            "" => String::new(),
            text => identitet(text).unwrap_or_else(|e| {
                fel.push(Fältfel::nytt(Fält::Identitet, e.kod(), e));
                text.to_string()
            }),
        };
        let epost = match self.epost.trim() {
            "" => String::new(),
            adress => crate::epost::normalisera(adress).unwrap_or_else(|_| {
                fel.push(Fältfel::nytt(
                    Fält::Epost,
                    "epost_ogiltig",
                    "Ange en e-postadress som namn@exempel.se.",
                ));
                adress.to_string()
            }),
        };
        let momsregnr = self
            .momsregnr
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '-')
            .collect::<String>()
            .to_uppercase();
        let villkor = self.betalningsvillkor.trim();
        let betalningsvillkor = if villkor.bytes().all(|b| b.is_ascii_digit()) {
            villkor.parse::<u16>().ok()
        } else {
            None
        };
        if betalningsvillkor.is_none() {
            fel.push(Fältfel::nytt(
                Fält::Betalningsvillkor,
                "betalningsvillkor_ogiltigt",
                "Ange betalningsvillkoret som ett helt antal dagar.",
            ));
        }
        let omvänd = OmvändSkattskyldighet::från_kod(self.omvänd_skattskyldighet.trim());
        if omvänd.is_none() {
            fel.push(Fältfel::nytt(
                Fält::OmvändSkattskyldighet,
                "val_saknas",
                "Välj omvänd skattskyldighet.",
            ));
        }
        match (betalningsvillkor, omvänd) {
            (Some(betalningsvillkor), Some(omvänd_skattskyldighet)) if fel.is_empty() => {
                Ok(GiltigKund {
                    namn,
                    identitet,
                    adress: self
                        .adress
                        .lines()
                        .map(str::trim)
                        .filter(|r| !r.is_empty())
                        .collect::<Vec<_>>()
                        .join("\n"),
                    epost,
                    momsregnr,
                    betalningsvillkor,
                    omvänd_skattskyldighet,
                })
            }
            _ => Err(fel),
        }
    }
}

/// Varför ett organisations- eller personnummer nekas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentitetsFel {
    Format,
    Kontrollsiffra,
    Datum,
}

impl IdentitetsFel {
    pub fn kod(self) -> &'static str {
        match self {
            IdentitetsFel::Format => "identitet_format",
            IdentitetsFel::Kontrollsiffra => "identitet_kontrollsiffra",
            IdentitetsFel::Datum => "personnr_datum",
        }
    }
}

impl fmt::Display for IdentitetsFel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            IdentitetsFel::Format => {
                "Ange ett organisationsnummer (NNNNNN-NNNN) eller ett personnummer \
                 (ÅÅÅÅMMDD-NNNN)."
            }
            IdentitetsFel::Kontrollsiffra => "Kontrollsiffran stämmer inte. Kontrollera numret.",
            IdentitetsFel::Datum => "Personnumret har inget giltigt födelsedatum.",
        })
    }
}

/// Ett organisationsnummer eller, när tredje siffran är under 2 eller numret har tolv siffror,
/// ett personnummer eller samordningsnummer, skrivet med bindestreck.
fn identitet(text: &str) -> Result<String, IdentitetsFel> {
    let siffror: String = text
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-' && *c != '+')
        .collect();
    if !siffror.bytes().all(|b| b.is_ascii_digit()) || !matches!(siffror.len(), 10 | 12) {
        return Err(IdentitetsFel::Format);
    }
    if siffror.len() == 10 {
        match Organisationsnummer::tolka(&siffror) {
            Ok(orgnr) => return Ok(orgnr.to_string()),
            Err(OrgnrFel::Kontrollsiffra) => return Err(IdentitetsFel::Kontrollsiffra),
            Err(OrgnrFel::Längd) => return Err(IdentitetsFel::Format),
            Err(OrgnrFel::TredjeSiffran) => {}
        }
    }
    let s: Vec<u32> = siffror.bytes().map(|b| u32::from(b - b'0')).collect();
    let tio = &s[s.len() - 10..];
    let månad = tio[2] * 10 + tio[3];
    let dag = tio[4] * 10 + tio[5];
    // Ett samordningsnummer har födelsedagen plus 60.
    if !(1..=12).contains(&månad) || !(1..=31).contains(&(dag % 60)) || dag > 91 {
        return Err(IdentitetsFel::Datum);
    }
    if !crate::luhn::stämmer(&siffror[siffror.len() - 10..]) {
        return Err(IdentitetsFel::Kontrollsiffra);
    }
    let n = siffror.len();
    Ok(format!("{}-{}", &siffror[..n - 4], &siffror[n - 4..]))
}

/// Varför en kund inte kan raderas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Raderingsspärr {
    /// Kunden har fakturor; den kan arkiveras i stället.
    HarFakturor,
}

impl Raderingsspärr {
    pub fn kod(self) -> &'static str {
        match self {
            Raderingsspärr::HarFakturor => "har_fakturor",
        }
    }
}

impl fmt::Display for Raderingsspärr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Raderingsspärr::HarFakturor => {
                "Kunden har fakturor och kan inte raderas. Arkivera den i stället."
            }
        })
    }
}

/// En kund kan raderas bara om den saknar fakturor; en kund med fakturor arkiveras.
pub fn kan_raderas(antal_fakturor: u64) -> Result<(), Raderingsspärr> {
    if antal_fakturor == 0 {
        Ok(())
    } else {
        Err(Raderingsspärr::HarFakturor)
    }
}
