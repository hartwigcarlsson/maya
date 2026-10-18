//! Företagets fakturauppgifter under Inställningar → Företag, utöver adress, säte,
//! momsregistreringsnummer och F-skatt i [`crate::foretag::Uppgifter`]: betalsätten, om
//! fakturorna har OCR-referens, logotypen, svarsadressen och standardmeddelandet för
//! fakturamejl. Samma regler i formuläret och på servern.

use crate::betalsatt::{Betalsättsfel, tolka_bankgiro, tolka_bic, tolka_iban, tolka_plusgiro};

/// Fälten i formuläret, i ordning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fält {
    Bankgiro,
    Plusgiro,
    Iban,
    Bic,
    Ocr,
    Svarsadress,
    Standardmeddelande,
    Logotyp,
}

impl Fält {
    /// Fältets namn i formuläret.
    pub fn namn(self) -> &'static str {
        match self {
            Fält::Bankgiro => "bankgiro",
            Fält::Plusgiro => "plusgiro",
            Fält::Iban => "iban",
            Fält::Bic => "bic",
            Fält::Ocr => "ocr",
            Fält::Svarsadress => "svarsadress",
            Fält::Standardmeddelande => "standardmeddelande",
            Fält::Logotyp => "logotyp",
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

    fn betalsätt(fält: Fält, fel: Betalsättsfel) -> Self {
        Fältfel::nytt(fält, fel.kod(), fel)
    }
}

/// Fakturauppgifterna som användaren skrev dem, eller kontrollerade. Tom text betyder att
/// uppgiften saknas.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fakturauppgifter {
    /// `NNN-NNNN` eller `NNNN-NNNN` när det är kontrollerat.
    pub bankgiro: String,
    /// Siffrorna med kontrollsiffran efter ett bindestreck när det är kontrollerat.
    pub plusgiro: String,
    /// I grupper om fyra när det är kontrollerat.
    pub iban: String,
    pub bic: String,
    /// Om fakturorna har en OCR-referens; annars är fakturanumret referensen.
    pub ocr: bool,
    /// Svarsadressen (Reply-To) för fakturamejl.
    pub svarsadress: String,
    /// Meddelandet som fakturamejlen börjar med.
    pub standardmeddelande: String,
}

impl Fakturauppgifter {
    /// Kontrollerar uppgifterna och skriver dem på ett enhetligt sätt. Felen kommer i fältens
    /// ordning. Inget betalsätt krävs här; det krävs först för att utfärda en faktura.
    pub fn kontrollera(&self) -> Result<Fakturauppgifter, Vec<Fältfel>> {
        let mut fel = Vec::new();
        let mut tolka =
            |fält: Fält, text: &str, tolk: fn(&str) -> Result<String, Betalsättsfel>| {
                let text = text.trim();
                if text.is_empty() {
                    return String::new();
                }
                tolk(text).unwrap_or_else(|e| {
                    fel.push(Fältfel::betalsätt(fält, e));
                    text.to_string()
                })
            };
        let bankgiro = tolka(Fält::Bankgiro, &self.bankgiro, tolka_bankgiro);
        let plusgiro = tolka(Fält::Plusgiro, &self.plusgiro, tolka_plusgiro);
        let iban = tolka(Fält::Iban, &self.iban, tolka_iban);
        let bic = tolka(Fält::Bic, &self.bic, tolka_bic);
        if bic.is_empty() && !iban.is_empty() {
            fel.push(Fältfel::nytt(
                Fält::Bic,
                "bic_saknas",
                "Ett IBAN-nummer behöver bankens BIC.",
            ));
        }
        if iban.is_empty() && !bic.is_empty() {
            fel.push(Fältfel::nytt(
                Fält::Iban,
                "iban_saknas",
                "BIC anges tillsammans med ett IBAN-nummer.",
            ));
        }
        let svarsadress = match self.svarsadress.trim() {
            "" => String::new(),
            adress => crate::epost::normalisera(adress).unwrap_or_else(|_| {
                fel.push(Fältfel::nytt(
                    Fält::Svarsadress,
                    "svarsadress_ogiltig",
                    "Ange en e-postadress som faktura@exempel.se.",
                ));
                adress.to_string()
            }),
        };
        if !fel.is_empty() {
            return Err(fel);
        }
        Ok(Fakturauppgifter {
            bankgiro,
            plusgiro,
            iban,
            bic,
            ocr: self.ocr,
            svarsadress,
            standardmeddelande: self.standardmeddelande.trim().to_string(),
        })
    }

    /// Om minst ett betalsätt finns: bankgiro, plusgiro eller IBAN med BIC.
    pub fn har_betalsätt(&self) -> bool {
        !self.bankgiro.is_empty()
            || !self.plusgiro.is_empty()
            || (!self.iban.is_empty() && !self.bic.is_empty())
    }
}

/// Den största logotyp som tas emot.
pub const STÖRSTA_LOGOTYP: usize = 1024 * 1024;

/// Koderna som en logotyp kan nekas med, och texten vid fältet.
pub const LOGOTYPSPÄRRAR: &[(&str, &str)] = &[
    ("logotyp_saknas", "Välj en fil med logotypen."),
    ("logotyp_for_stor", "Logotypen får vara högst 1 MB."),
    (
        "logotyp_format",
        "Logotypen ska vara en PNG-, JPG- eller SVG-fil.",
    ),
];

/// Felet för en kod i [`LOGOTYPSPÄRRAR`], t.ex. när servern skickar tillbaka koden.
pub fn logotypfel(kod: &str) -> Option<Fältfel> {
    LOGOTYPSPÄRRAR
        .iter()
        .find(|(k, _)| *k == kod)
        .map(|(kod, text)| Fältfel::nytt(Fält::Logotyp, kod, *text))
}

/// MIME-typen för en logotyp efter filens innehåll: PNG, JPG eller SVG. Andra format nekas,
/// liksom en tom eller för stor fil.
pub fn kontrollera_logotyp(innehåll: &[u8]) -> Result<&'static str, Fältfel> {
    let fel = |kod: &str| Err(logotypfel(kod).expect("koden finns i LOGOTYPSPÄRRAR"));
    if innehåll.is_empty() {
        return fel("logotyp_saknas");
    }
    if innehåll.len() > STÖRSTA_LOGOTYP {
        return fel("logotyp_for_stor");
    }
    if innehåll.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Ok("image/png");
    }
    if innehåll.starts_with(b"\xff\xd8\xff") {
        return Ok("image/jpeg");
    }
    if är_svg(innehåll) {
        return Ok("image/svg+xml");
    }
    fel("logotyp_format")
}

/// Om filen är en SVG: UTF-8-text vars första element, efter en XML-deklaration, kommentarer
/// och en doctype, är `<svg`.
fn är_svg(innehåll: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(innehåll) else {
        return false;
    };
    let mut rest = text.trim_start_matches('\u{feff}');
    loop {
        rest = rest.trim_start();
        let slut = if rest.starts_with("<?") {
            rest.find("?>").map(|i| i + 2)
        } else if rest.starts_with("<!--") {
            rest.find("-->").map(|i| i + 3)
        } else if rest.starts_with("<!") {
            rest.find('>').map(|i| i + 1)
        } else {
            return rest
                .strip_prefix("<svg")
                .and_then(|r| r.chars().next())
                .is_some_and(|c| c.is_whitespace() || c == '>' || c == '/');
        };
        match slut {
            Some(i) => rest = &rest[i..],
            None => return false,
        }
    }
}
