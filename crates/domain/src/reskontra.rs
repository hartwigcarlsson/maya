//! Kund- och leverantörsreskontran: fakturornas status och listornas gruppering. Allt räknas
//! fram ur fakturans total, det som återstår, förfallodagen och dagens datum, och lagras inte.
//!
//! Märkena i etapp 2 är obetald, delbetald, förfallen och betald, med kreditfakturorna (#140)
//! krediterad, tillgodo och kreditfaktura, och kundförlust (#141) för en kundfaktura som en
//! gällande kundförlust har skrivit bort.

use jiff::civil::Date;

use crate::att_gora::dagar_mellan;

/// En utfärdad eller registrerad fakturas status som märke.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Märke {
    /// Inget är betalt.
    Obetald,
    /// En del är betald och resten återstår.
    Delbetald,
    /// Något återstår efter förfallodagen.
    Förfallen,
    /// Inget återstår.
    Betald,
    /// En kundförlust som inte är återkallad har skrivit bort det som återstod (#141).
    Kundförlust,
    // #140 Kreditfakturor
    /// Hela fakturan är krediterad och inget var betalt.
    Krediterad,
    /// Mer är betalt än det som återstår efter krediteringarna: motparten har pengar tillgodo,
    /// som kan återbetalas.
    Tillgodo,
    /// Fakturan är en kreditfaktura, som räknas av mot originalet.
    Kreditfaktura,
    // #148 Rätta avskrift
    /// Leverantörsfakturans registrering är vänd med Rätta avskrift, och den registreras om.
    Upplåst,
    // #143 Externa kundfakturor
    /// Kundfakturan har skapats i ett annat system och registrerats i Maya. Visas bredvid
    /// statusen; en upplåst extern kundfaktura (Rätta registrering) får [`Märke::Upplåst`].
    Extern,
}

impl Märke {
    pub fn namn(self) -> &'static str {
        match self {
            Märke::Obetald => "Obetald",
            Märke::Delbetald => "Delbetald",
            Märke::Förfallen => "Förfallen",
            Märke::Betald => "Betald",
            Märke::Kundförlust => "Kundförlust",
            Märke::Krediterad => "Krediterad",
            Märke::Tillgodo => "Tillgodo",
            Märke::Kreditfaktura => "Kreditfaktura",
            Märke::Upplåst => "Upplåst",
            Märke::Extern => "Extern",
        }
    }

    /// Koden i märkets CSS-klass, `badge-<kod>`.
    pub fn kod(self) -> &'static str {
        match self {
            Märke::Obetald => "obetald",
            Märke::Delbetald => "delbetald",
            Märke::Förfallen => "forfallen",
            Märke::Betald => "betald",
            Märke::Kundförlust => "kundforlust",
            Märke::Krediterad => "krediterad",
            Märke::Tillgodo => "tillgodo",
            Märke::Kreditfaktura => "kreditfaktura",
            Märke::Upplåst => "upplast",
            Märke::Extern => "extern",
        }
    }
}

/// Om något återstår efter förfallodagen. På förfallodagen är fakturan inte förfallen.
pub fn förfallen(återstår: i64, förfallodag: Date, idag: Date) -> bool {
    återstår > 0 && förfallodag < idag
}

/// Märket för en faktura med `total` öre, varav `återstår` öre är obetalt. Ett negativt
/// `återstår` är ett tillgodohavande.
pub fn märke(total: i64, återstår: i64, förfallodag: Date, idag: Date) -> Märke {
    if återstår < 0 {
        Märke::Tillgodo
    } else if återstår == 0 {
        Märke::Betald
    } else if förfallen(återstår, förfallodag, idag) {
        Märke::Förfallen
    } else if återstår < total {
        Märke::Delbetald
    } else {
        Märke::Obetald
    }
}

/// Märket för en utfärdad kundfaktura med `total` öre som kreditfakturor har krediterat
/// `krediterat` öre av (positivt), varav `återstår` öre är obetalt efter krediteringarna,
/// betalningarna och en gällande kundförlust. [`Märke::Kundförlust`] medan en kundförlust gäller
/// fakturan (#141), [`Märke::Krediterad`] när hela fakturan är krediterad och inget var betalt
/// (#140), annars [`märke`] räknat mot det som finns kvar efter krediteringarna, så att ett
/// negativt `återstår` är [`Märke::Tillgodo`]. En kreditfaktura själv får
/// [`Märke::Kreditfaktura`] av anroparen.
pub fn kundfakturans_märke(
    total: i64,
    krediterat: i64,
    återstår: i64,
    kundförlust: bool,
    förfallodag: Date,
    idag: Date,
) -> Märke {
    if kundförlust {
        Märke::Kundförlust
    } else if återstår == 0 && krediterat > 0 && krediterat >= total {
        Märke::Krediterad
    } else {
        märke(total - krediterat, återstår, förfallodag, idag)
    }
}

// #148 Kreditfaktura från leverantör
/// Märket för en registrerad leverantörsfaktura med `total` öre som kreditfakturor har
/// krediterat `krediterat` öre av (positivt), varav `återstår` öre är obetalt: samma regler som
/// [`kundfakturans_märke`] utan kundförlust. En kreditfaktura med hänvisning och en upplåst
/// faktura får [`Märke::Kreditfaktura`] och [`Märke::Upplåst`] av anroparen.
pub fn leverantörsfakturans_märke(
    total: i64,
    krediterat: i64,
    återstår: i64,
    förfallodag: Date,
    idag: Date,
) -> Märke {
    kundfakturans_märke(total, krediterat, återstår, false, förfallodag, idag)
}

/// Grupperna i listan, i den ordning de visas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Grupp {
    Förfallna,
    InomSjuDagar,
    Senare,
    Utkast,
    /// Betalda och, när de kommer, krediterade och förluster.
    Avslutade,
}

impl Grupp {
    pub const ALLA: [Grupp; 5] = [
        Grupp::Förfallna,
        Grupp::InomSjuDagar,
        Grupp::Senare,
        Grupp::Utkast,
        Grupp::Avslutade,
    ];

    pub fn rubrik(self) -> &'static str {
        match self {
            Grupp::Förfallna => "Förfallna",
            Grupp::InomSjuDagar => "Förfaller inom 7 dagar",
            Grupp::Senare => "Senare",
            Grupp::Utkast => "Utkast",
            Grupp::Avslutade => "Avslutade",
        }
    }
}

/// Gruppen för en utfärdad eller registrerad faktura. Utkasten är alltid [`Grupp::Utkast`].
pub fn grupp(återstår: i64, förfallodag: Date, idag: Date) -> Grupp {
    if återstår <= 0 {
        Grupp::Avslutade
    } else if förfallen(återstår, förfallodag, idag) {
        Grupp::Förfallna
    } else if dagar_mellan(idag, förfallodag) <= 7 {
        Grupp::InomSjuDagar
    } else {
        Grupp::Senare
    }
}

/// Kopieringschipsen för en leverantörsfaktura att betala (#152): leverantörens bankgiro (BG),
/// plusgiro (PG) och IBAN som de är registrerade, och fakturans referens. En referens med bara
/// siffror är en OCR-referens och visas utan mellanslag; annan text är ett meddelande. Tomma
/// uppgifter ger inget chip.
pub fn kopieringschips(
    bankgiro: &str,
    plusgiro: &str,
    iban: &str,
    referens: &str,
) -> Vec<(&'static str, String)> {
    let mut chips: Vec<(&'static str, String)> =
        [("BG", bankgiro), ("PG", plusgiro), ("IBAN", iban)]
            .into_iter()
            .filter(|(_, v)| !v.trim().is_empty())
            .map(|(k, v)| (k, v.trim().to_string()))
            .collect();
    let referens = referens.trim();
    let siffror: String = referens.chars().filter(|c| !c.is_whitespace()).collect();
    if !siffror.is_empty() && siffror.bytes().all(|b| b.is_ascii_digit()) {
        chips.push(("OCR", siffror));
    } else if !referens.is_empty() {
        chips.push(("Meddelande", referens.to_string()));
    }
    chips
}

/// "förfaller i dag", "förfaller om 5 dagar" eller "förföll för 10 dagar sedan".
pub fn förfallotext(förfallodag: Date, idag: Date) -> String {
    let dagar = |n: i32| {
        if n == 1 {
            "1 dag".to_string()
        } else {
            format!("{n} dagar")
        }
    };
    match dagar_mellan(idag, förfallodag) {
        0 => "förfaller i dag".to_string(),
        n if n > 0 => format!("förfaller om {}", dagar(n)),
        n => format!("förföll för {} sedan", dagar(-n)),
    }
}
