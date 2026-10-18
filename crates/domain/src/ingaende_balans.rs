//! Formuläret för den ingående balansen: saldon per balanskonto (klass 1–2) när ett
//! räkenskapsår börjar, där summan måste vara noll (Å2). Formuläret i webbläsaren och servern kör
//! samma kontroll, och varje fel har en kod som formuläret visar vid sitt fält.

use std::fmt;

use crate::belopp::{Belopp, Saldo};
use crate::kontoplan::Kontonummer;

/// En rad i formuläret, som text. Debet och kredit lämnas tomma för ett konto utan saldo.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NySaldorad {
    pub konto: String,
    pub debet: String,
    pub kredit: String,
}

/// Formuläret som användaren fyllde i, som text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NyIngåendeBalans {
    /// Raderna i formulärets ordning. Rader utan belopp räknas inte.
    pub rader: Vec<NySaldorad>,
}

/// Fältet som ett fel gäller. Raderna räknas från 0 i formulärets ordning, också de utan belopp.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Fält {
    Konto(usize),
    Belopp(usize),
    /// Summaraden (differens).
    Summa,
}

/// Något som hindrar att den ingående balansen sparas, med en kod som formuläret visar.
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

/// En ingående balans som klarar kontrollen: saldon skilda från noll, i kontoordning.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KontrolleradIngåendeBalans {
    pub saldon: Vec<(Kontonummer, Saldo)>,
}

impl NyIngåendeBalans {
    /// Prövar formuläret mot kontoplanen (`konton`: kontona och om de är aktiva). Bara aktiva
    /// balanskonton får ett saldo, varje konto en gång, och summan ska vara noll.
    pub fn kontrollera(
        &self,
        konton: &[(Kontonummer, bool)],
    ) -> Result<KontrolleradIngåendeBalans, Vec<Spärr>> {
        let mut spärrar = Vec::new();
        let mut saldon: Vec<(Kontonummer, Saldo)> = Vec::new();
        let mut summa = 0i128;
        for (i, rad) in self.rader.iter().enumerate() {
            let Some(saldo) = kontrollera_belopp(i, rad, &mut spärrar) else {
                continue;
            };
            if saldo == Saldo::NOLL {
                continue;
            }
            summa += i128::from(saldo.öre());
            let Some(konto) = kontrollera_konto(i, &rad.konto, konton, &mut spärrar) else {
                continue;
            };
            if saldon.iter().any(|(k, _)| *k == konto) {
                spärrar.push(Spärr::ny(
                    Fält::Konto(i),
                    "konto_dubblett",
                    format!("Konto {konto} har redan ett saldo."),
                ));
                continue;
            }
            saldon.push((konto, saldo));
        }
        if summa != 0 {
            let differens = Belopp::från_öre(i64::try_from(summa.abs()).unwrap_or(i64::MAX));
            let sida = if summa > 0 { "debet" } else { "kredit" };
            spärrar.push(Spärr::ny(
                Fält::Summa,
                "differens",
                format!("Differens {differens}: {sida} är större. Summan ska vara noll."),
            ));
        }
        if !spärrar.is_empty() {
            spärrar.sort_by_key(|s| match s.fält {
                Fält::Konto(i) | Fält::Belopp(i) => (i, 0),
                Fält::Summa => (usize::MAX, 0),
            });
            return Err(spärrar);
        }
        saldon.sort_by_key(|(k, _)| *k);
        Ok(KontrolleradIngåendeBalans { saldon })
    }
}

/// Kontot som den preliminära ingående balansen för resultatet som inte är ombokat förs till.
pub const PRELIMINÄRT_RESULTAT: Kontonummer = Kontonummer::konstant(2099);

/// Den preliminära ingående balansen ur föregående års utgående balans, enligt tabellen över
/// automatkonteringar: IB(k, n) = UB(k, n−1) för varje konto k i klass 1–2, och därtill
/// IB(2099, n) = UB(2099, n−1) + Σ UB(k, n−1) över klass 3–8. Saldon som blir noll tas bort.
pub fn preliminär(
    utgående: impl IntoIterator<Item = (Kontonummer, Saldo)>,
) -> Vec<(Kontonummer, Saldo)> {
    let resultatkonto = PRELIMINÄRT_RESULTAT;
    let mut ingående: Vec<(Kontonummer, Saldo)> = Vec::new();
    for (konto, saldo) in utgående {
        let konto = if konto.typ().balanskonto() {
            konto
        } else {
            resultatkonto
        };
        match ingående.iter_mut().find(|(k, _)| *k == konto) {
            Some((_, s)) => *s += saldo,
            None => ingående.push((konto, saldo)),
        }
    }
    ingående.retain(|(_, s)| *s != Saldo::NOLL);
    ingående.sort_by_key(|(k, _)| *k);
    ingående
}

fn kontrollera_konto(
    i: usize,
    text: &str,
    konton: &[(Kontonummer, bool)],
    spärrar: &mut Vec<Spärr>,
) -> Option<Kontonummer> {
    let text = text.trim();
    let konto = text.parse::<Kontonummer>().ok();
    if let Some(konto) = konto
        && !konto.typ().balanskonto()
    {
        spärrar.push(Spärr::ny(
            Fält::Konto(i),
            "konto_inte_balanskonto",
            format!("Konto {konto} är inget balanskonto. Bara klass 1–2 har en ingående balans."),
        ));
        return None;
    }
    match konto.and_then(|k| konton.iter().find(|(n, _)| *n == k)) {
        Some((konto, true)) => Some(*konto),
        Some((konto, false)) => {
            spärrar.push(Spärr::ny(
                Fält::Konto(i),
                "konto_inaktivt",
                format!("Konto {konto} är inaktivt. Aktivera det för att ge det ett saldo."),
            ));
            None
        }
        None => {
            spärrar.push(Spärr::ny(
                Fält::Konto(i),
                "konto_okant",
                format!("Konto {text} finns inte i kontoplanen."),
            ));
            None
        }
    }
}

/// Saldot på raden, noll om båda kolumnerna är tomma.
fn kontrollera_belopp(i: usize, rad: &NySaldorad, spärrar: &mut Vec<Spärr>) -> Option<Saldo> {
    let mut tolka = |text: &str| {
        let text = text.trim();
        if text.is_empty() {
            return Some(Belopp::NOLL);
        }
        let belopp = Belopp::tolka(text);
        if belopp.is_none() {
            spärrar.push(Spärr::ny(
                Fält::Belopp(i),
                "belopp_ogiltigt",
                "Ange ett belopp i kronor med högst två decimaler.",
            ));
        }
        belopp
    };
    let (debet, kredit) = (tolka(&rad.debet), tolka(&rad.kredit));
    let (debet, kredit) = (debet?, kredit?);
    if debet != Belopp::NOLL && kredit != Belopp::NOLL {
        spärrar.push(Spärr::ny(
            Fält::Belopp(i),
            "belopp_debet_och_kredit",
            "Ett saldo står antingen i debet eller i kredit.",
        ));
        return None;
    }
    Some(Saldo::av(debet, kredit))
}
