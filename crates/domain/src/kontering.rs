//! Rätta kontering på en utfärdad kundfaktura (#142): användaren väljer ett nytt intäktskonto
//! per rad. Utfärdandets verifikat vänds med en rättelse och ett nytt utfärdandeverifikat
//! bokförs med dagens datum, med samma fordran och moms och intäkterna på de nya kontona.
//! Fakturan, numret och PDF:en ändras inte.
//!
//! Spärrarna har samma koder som intäktskontot på en fakturarad när fakturan utfärdas.

use crate::belopp::Belopp;
use crate::kontoplan::Kontonummer;
use crate::kundfaktura::{Fält, Radfält, Spärr, intäktskontot, rad};
use crate::verifikat::Rad;

/// Spärrarna när konteringen rättas, som Systemdokumentationen återger (S2): koden och regeln.
pub const KONTERINGSSPÄRRAR: &[(&str, &str)] = &[
    ("rad_konto_saknas", "Varje rad har ett intäktskonto."),
    ("konto_okant", "Kontot finns i företagets kontoplan."),
    ("konto_inaktivt", "Kontot är aktivt."),
    ("konto_inte_intakt", "Intäktskontot är ett konto i klass 3."),
    (
        "kontering_rader",
        "Ett konto väljs för varje rad på fakturan.",
    ),
    (
        "kontering_oforandrad",
        "Minst en rad får ett annat konto än det den är bokförd på.",
    ),
];

/// En rad på den utfärdade fakturan som den är bokförd nu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Intäktsrad {
    pub beskrivning: String,
    /// Radens netto i öre.
    pub netto: i64,
    /// Intäktskontot som raden är bokförd på.
    pub konto: Kontonummer,
}

/// Formuläret Rätta kontering som användaren skrev det.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NyKontering {
    /// Det nya intäktskontot för varje rad, i fakturans ordning.
    pub konton: Vec<String>,
    /// Valfri orsak, som rättelsens beskrivning får.
    pub orsak: String,
}

impl NyKontering {
    /// De nya kontona i radernas ordning, när varje rad har ett aktivt intäktskonto i
    /// kontoplanen `konton` och minst en rad får ett nytt konto.
    pub fn kontrollera(
        &self,
        rader: &[Intäktsrad],
        konton: &[(Kontonummer, bool)],
    ) -> Result<Vec<Kontonummer>, Vec<Spärr>> {
        if self.konton.len() != rader.len() {
            return Err(vec![Spärr::ny(
                Fält::Rader,
                "kontering_rader",
                "Välj ett konto för varje rad på fakturan.",
            )]);
        }
        let mut spärrar = Vec::new();
        let mut nya = Vec::new();
        for (i, text) in self.konton.iter().enumerate() {
            match intäktskontot(text, konton) {
                Ok(konto) => nya.push(konto),
                Err((kod, text)) => {
                    spärrar.push(Spärr::ny(Fält::Rad(i, Radfält::Konto), kod, text));
                }
            }
        }
        if !spärrar.is_empty() {
            return Err(spärrar);
        }
        if rader.iter().zip(&nya).all(|(r, k)| r.konto == *k) {
            return Err(vec![Spärr::ny(
                Fält::Rader,
                "kontering_oforandrad",
                "Välj ett annat konto på minst en rad.",
            )]);
        }
        Ok(nya)
    }
}

/// Det nya utfärdandeverifikatets rader: verifikatet som det är bokfört nu, `nuvarande`, där
/// raderna på fakturaradernas intäktskonton byts mot radernas netto per nytt konto, i
/// fakturaradernas ordning och där den första intäktsraden stod. Fordran, momsen och allt annat
/// följer med som det är.
pub fn ny_kontering(nuvarande: &[Rad], rader: &[Intäktsrad], nya: &[Kontonummer]) -> Vec<Rad> {
    let mut intäkter: Vec<(Kontonummer, i64)> = Vec::new();
    for (r, konto) in rader.iter().zip(nya) {
        match intäkter.iter_mut().find(|(k, _)| k == konto) {
            Some((_, summa)) => *summa += r.netto,
            None => intäkter.push((*konto, r.netto)),
        }
    }
    let mut intäktsrader: Vec<Rad> = intäkter
        .into_iter()
        .map(|(k, netto)| rad(k, -netto))
        .filter(|r| r.debet != Belopp::NOLL || r.kredit != Belopp::NOLL)
        .collect();
    let mut kontering = Vec::new();
    for r in nuvarande {
        if rader.iter().any(|f| f.konto == r.konto) {
            kontering.append(&mut intäktsrader);
        } else {
            kontering.push(r.clone());
        }
    }
    kontering.append(&mut intäktsrader);
    kontering
}
