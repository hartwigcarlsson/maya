//! Kund- och leverantörsreskontran: fakturornas status och listornas gruppering. Allt räknas
//! fram ur fakturans total, det som återstår, förfallodagen och dagens datum, och lagras inte.
//!
//! Märkena i etapp 2 är obetald, delbetald, förfallen och betald; krediterad och tillgodo
//! kommer med kreditfakturorna (#140) och kundförlust med #141.

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
}

impl Märke {
    pub fn namn(self) -> &'static str {
        match self {
            Märke::Obetald => "Obetald",
            Märke::Delbetald => "Delbetald",
            Märke::Förfallen => "Förfallen",
            Märke::Betald => "Betald",
        }
    }

    /// Koden i märkets CSS-klass, `badge-<kod>`.
    pub fn kod(self) -> &'static str {
        match self {
            Märke::Obetald => "obetald",
            Märke::Delbetald => "delbetald",
            Märke::Förfallen => "forfallen",
            Märke::Betald => "betald",
        }
    }
}

/// Om något återstår efter förfallodagen. På förfallodagen är fakturan inte förfallen.
pub fn förfallen(återstår: i64, förfallodag: Date, idag: Date) -> bool {
    återstår > 0 && förfallodag < idag
}

/// Märket för en faktura med `total` öre, varav `återstår` öre är obetalt.
pub fn märke(total: i64, återstår: i64, förfallodag: Date, idag: Date) -> Märke {
    if återstår <= 0 {
        Märke::Betald
    } else if förfallen(återstår, förfallodag, idag) {
        Märke::Förfallen
    } else if återstår < total {
        Märke::Delbetald
    } else {
        Märke::Obetald
    }
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
