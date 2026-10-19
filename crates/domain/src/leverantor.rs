//! En leverantör i leverantörsregistret: formuläret och dess regler, samma i webbläsaren och i
//! servern. Svenska nummer prövas med samma regler som företagets egna uppgifter och
//! betaluppgifterna med samma regler som företagets betalsätt.

use crate::betalsatt::{Betalsättsfel, tolka_bankgiro, tolka_bic, tolka_iban, tolka_plusgiro};
use crate::kontoplan::Kontonummer;
use crate::orgnr::Organisationsnummer;

/// Omvänd skattskyldighet som förvalt på leverantörens fakturor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum OmvändSkattskyldighet {
    #[default]
    Ingen,
    EuTjänst,
    EuVara,
    Bygg,
    /// Tjänst från ett land utanför EU.
    UtanförEu,
}

impl OmvändSkattskyldighet {
    pub const ALLA: [OmvändSkattskyldighet; 5] = [
        OmvändSkattskyldighet::Ingen,
        OmvändSkattskyldighet::EuTjänst,
        OmvändSkattskyldighet::EuVara,
        OmvändSkattskyldighet::Bygg,
        OmvändSkattskyldighet::UtanförEu,
    ];

    pub fn kod(self) -> &'static str {
        match self {
            OmvändSkattskyldighet::Ingen => "ingen",
            OmvändSkattskyldighet::EuTjänst => "eu_tjanst",
            OmvändSkattskyldighet::EuVara => "eu_vara",
            OmvändSkattskyldighet::Bygg => "bygg",
            OmvändSkattskyldighet::UtanförEu => "utanfor_eu",
        }
    }

    pub fn namn(self) -> &'static str {
        match self {
            OmvändSkattskyldighet::Ingen => "Ingen",
            OmvändSkattskyldighet::EuTjänst => "EU-tjänst",
            OmvändSkattskyldighet::EuVara => "EU-vara",
            OmvändSkattskyldighet::Bygg => "Bygg",
            OmvändSkattskyldighet::UtanförEu => "Tjänst från land utanför EU",
        }
    }

    pub fn från_kod(kod: &str) -> Option<Self> {
        Self::ALLA.into_iter().find(|o| o.kod() == kod)
    }
}

/// Formulärets fält, med namnen som formuläret använder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Fält {
    Namn,
    Orgnr,
    Gatuadress,
    Postnummer,
    Postort,
    Land,
    Momsregnr,
    Bankgiro,
    Plusgiro,
    Iban,
    Bic,
    Standardkonto,
    OmvändSkattskyldighet,
}

impl Fält {
    pub fn namn(self) -> &'static str {
        match self {
            Fält::Namn => "namn",
            Fält::Orgnr => "orgnr",
            Fält::Gatuadress => "gatuadress",
            Fält::Postnummer => "postnummer",
            Fält::Postort => "postort",
            Fält::Land => "land",
            Fält::Momsregnr => "momsregnr",
            Fält::Bankgiro => "bankgiro",
            Fält::Plusgiro => "plusgiro",
            Fält::Iban => "iban",
            Fält::Bic => "bic",
            Fält::Standardkonto => "standardkonto",
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

/// Formuläret som användaren fyllde i, som text. Tom text betyder att uppgiften saknas; tomt
/// land är Sverige och tom omvänd skattskyldighet är ingen.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NyLeverantör {
    pub namn: String,
    pub orgnr: String,
    pub gatuadress: String,
    pub postnummer: String,
    pub postort: String,
    /// Landskod enligt ISO 3166, t.ex. `SE`.
    pub land: String,
    pub momsregnr: String,
    pub bankgiro: String,
    pub plusgiro: String,
    pub iban: String,
    pub bic: String,
    /// Kontonumret som leverantörens fakturarader får förvalt.
    pub standardkonto: String,
    /// Koden för en [`OmvändSkattskyldighet`].
    pub omvänd_skattskyldighet: String,
}

/// En kontrollerad leverantör, skriven på ett enhetligt sätt.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GiltigLeverantör {
    pub namn: String,
    /// `NNNNNN-NNNN` för en svensk leverantör, annars som det skrevs.
    pub orgnr: String,
    pub gatuadress: String,
    pub postnummer: String,
    pub postort: String,
    pub land: String,
    pub momsregnr: String,
    /// Betaluppgifterna, skrivna som [`crate::betalsatt`] skriver dem; tomma när de saknas.
    pub bankgiro: String,
    pub plusgiro: String,
    pub iban: String,
    pub bic: String,
    pub standardkonto: Option<Kontonummer>,
    pub omvänd_skattskyldighet: OmvändSkattskyldighet,
}

impl GiltigLeverantör {
    /// Leverantören som formulär, för att ändra den.
    pub fn som_formulär(&self) -> NyLeverantör {
        NyLeverantör {
            namn: self.namn.clone(),
            orgnr: self.orgnr.clone(),
            gatuadress: self.gatuadress.clone(),
            postnummer: self.postnummer.clone(),
            postort: self.postort.clone(),
            land: self.land.clone(),
            momsregnr: self.momsregnr.clone(),
            bankgiro: self.bankgiro.clone(),
            plusgiro: self.plusgiro.clone(),
            iban: self.iban.clone(),
            bic: self.bic.clone(),
            standardkonto: self
                .standardkonto
                .map(|k| k.get().to_string())
                .unwrap_or_default(),
            omvänd_skattskyldighet: self.omvänd_skattskyldighet.kod().into(),
        }
    }
}

impl NyLeverantör {
    /// Kontrollerar formuläret mot företagets konton (nummer och om kontot är aktivt). Felen
    /// kommer i fältens ordning.
    pub fn kontrollera(
        &self,
        konton: &[(Kontonummer, bool)],
    ) -> Result<GiltigLeverantör, Vec<Fältfel>> {
        let mut fel = Vec::new();
        let mut nytt = |fält, kod, text: &str| {
            fel.push(Fältfel {
                fält,
                kod,
                text: text.into(),
            })
        };
        let namn = self.namn.trim().to_string();
        if namn.is_empty() {
            nytt(Fält::Namn, "namn_saknas", "Leverantören måste ha ett namn.");
        }
        let land = match self.land.trim().to_uppercase() {
            l if l.is_empty() => Some("SE".to_string()),
            l if l.len() == 2 && l.bytes().all(|b| b.is_ascii_uppercase()) => Some(l),
            _ => None,
        };
        let svensk = land.as_deref() == Some("SE");
        let orgnr = match self.orgnr.trim() {
            "" => String::new(),
            text if svensk => match Organisationsnummer::tolka(text) {
                Ok(nr) => nr.to_string(),
                Err(e) => {
                    nytt(Fält::Orgnr, e.kod(), &e.to_string());
                    text.to_string()
                }
            },
            text => text.to_string(),
        };
        let postnummer = postnummer(&self.postnummer, svensk).unwrap_or_else(|text| {
            nytt(
                Fält::Postnummer,
                "postnummer_ogiltigt",
                "Postnumret ska ha fem siffror.",
            );
            text
        });
        if land.is_none() {
            nytt(
                Fält::Land,
                "land_ogiltigt",
                "Ange landet som landskod med två bokstäver, t.ex. SE eller DE.",
            );
        }
        let momsregnr = kompakt(&self.momsregnr);
        if !momsregnr.is_empty() && !momsregnr_formen(&momsregnr, svensk) {
            nytt(
                Fält::Momsregnr,
                "momsregnr_ogiltigt",
                if svensk {
                    "Momsregistreringsnumret är SE och tolv siffror, t.ex. SE556036079301."
                } else {
                    "Momsregistreringsnumret börjar med landskoden, t.ex. DE123456789."
                },
            );
        }
        // Betaluppgifterna prövas som företagets betalsätt (#134): IBAN och BIC tillsammans.
        let mut tolka =
            |fält: Fält, text: &str, tolk: fn(&str) -> Result<String, Betalsättsfel>| match text
                .trim()
            {
                "" => String::new(),
                text => tolk(text).unwrap_or_else(|e| {
                    nytt(fält, e.kod(), &e.to_string());
                    String::new()
                }),
            };
        let bankgiro = tolka(Fält::Bankgiro, &self.bankgiro, tolka_bankgiro);
        let plusgiro = tolka(Fält::Plusgiro, &self.plusgiro, tolka_plusgiro);
        let iban = tolka(Fält::Iban, &self.iban, tolka_iban);
        let bic = tolka(Fält::Bic, &self.bic, tolka_bic);
        if self.bic.trim().is_empty() && !self.iban.trim().is_empty() {
            nytt(
                Fält::Bic,
                "bic_saknas",
                "Ett IBAN-nummer behöver bankens BIC.",
            );
        }
        if self.iban.trim().is_empty() && !self.bic.trim().is_empty() {
            nytt(
                Fält::Iban,
                "iban_saknas",
                "BIC anges tillsammans med ett IBAN-nummer.",
            );
        }
        let standardkonto = match self.standardkonto.trim() {
            "" => None,
            text => match text.parse::<Kontonummer>() {
                Err(_) => {
                    nytt(
                        Fält::Standardkonto,
                        "konto_ogiltigt",
                        "Ett konto har fyra siffror, t.ex. 5410.",
                    );
                    None
                }
                Ok(nr) => match konton.iter().find(|(k, _)| *k == nr) {
                    None => {
                        nytt(
                            Fält::Standardkonto,
                            "konto_finns_inte",
                            "Kontot finns inte i kontoplanen.",
                        );
                        None
                    }
                    Some((_, false)) => {
                        nytt(
                            Fält::Standardkonto,
                            "konto_inaktivt",
                            "Kontot är inaktivt. Aktivera det i kontoplanen först.",
                        );
                        None
                    }
                    Some((_, true)) => Some(nr),
                },
            },
        };
        let omvänd_skattskyldighet = match self.omvänd_skattskyldighet.trim() {
            "" => OmvändSkattskyldighet::Ingen,
            kod => OmvändSkattskyldighet::från_kod(kod).unwrap_or_else(|| {
                nytt(
                    Fält::OmvändSkattskyldighet,
                    "val_saknas",
                    "Välj omvänd skattskyldighet.",
                );
                OmvändSkattskyldighet::Ingen
            }),
        };
        if !fel.is_empty() {
            return Err(fel);
        }
        Ok(GiltigLeverantör {
            namn,
            orgnr,
            gatuadress: self.gatuadress.trim().to_string(),
            postnummer,
            postort: self.postort.trim().to_string(),
            land: land.unwrap_or_default(),
            momsregnr,
            bankgiro,
            plusgiro,
            iban,
            bic,
            standardkonto,
            omvänd_skattskyldighet,
        })
    }
}

fn kompakt(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect::<String>()
        .to_uppercase()
}

/// Ett svenskt postnummer har fem siffror och skrivs `NNN NN`; ett utländskt skrivs som det
/// skrevs. `Err` bär texten när ett svenskt nummer inte har fem siffror.
fn postnummer(text: &str, svensk: bool) -> Result<String, String> {
    if !svensk {
        return Ok(text.trim().to_string());
    }
    let siffror: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    if siffror.is_empty() {
        Ok(siffror)
    } else if siffror.len() == 5 && siffror.bytes().all(|b| b.is_ascii_digit()) {
        Ok(format!("{} {}", &siffror[..3], &siffror[3..]))
    } else {
        Err(siffror)
    }
}

/// Ett svenskt momsregistreringsnummer är `SE` och tolv siffror, som företagets eget; ett
/// utländskt börjar med två bokstäver och har två till tretton bokstäver eller siffror till.
fn momsregnr_formen(nr: &str, svensk: bool) -> bool {
    let b = nr.as_bytes();
    if svensk {
        b.len() == 14 && nr.starts_with("SE") && b[2..].iter().all(u8::is_ascii_digit)
    } else {
        (4..=15).contains(&b.len())
            && b[..2].iter().all(u8::is_ascii_uppercase)
            && b[2..].iter().all(u8::is_ascii_alphanumeric)
    }
}
