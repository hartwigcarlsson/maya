//! Grundboken och huvudboken (L4, A2): dokumenten och deras JSON för mallarna.

use jiff::Timestamp;
use jiff::civil::Date;
use maya_domain::belopp::{Belopp, Saldo};
use maya_domain::kontoplan::Kontonummer;
use maya_domain::rakenskapsar::periodnamn;
use serde_json::{Value, json};

use crate::data::{belopp, tid};
use crate::{Företag, Rad};

/// Räkenskapsåret och perioderna som en grundbok eller huvudbok gäller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bokföringsurval {
    pub år_start: Date,
    pub år_slut: Date,
    /// Den första periodens startdatum.
    pub från: Date,
    /// Den sista periodens startdatum.
    pub till: Date,
}

/// Grundboken: de bokförda verifikaten per period i registreringsordning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grundbok {
    pub företag: Företag,
    pub urval: Bokföringsurval,
    pub perioder: Vec<Grundboksperiod>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grundboksperiod {
    /// Periodens startdatum.
    pub start: Date,
    pub verifikat: Vec<Grundbokspost>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grundbokspost {
    /// T.ex. `V5`.
    pub nummer: String,
    pub datum: Date,
    pub beskrivning: String,
    pub bokförd: Timestamp,
    pub bokförd_av: String,
    pub rader: Vec<Rad>,
}

/// Huvudboken: per konto ingående balans, transaktioner med löpande saldo och utgående balans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Huvudbok {
    pub företag: Företag,
    pub urval: Bokföringsurval,
    /// Kontourvalet som det skrevs, tomt för alla konton.
    pub konton_urval: String,
    /// Om årets ingående balans är preliminär.
    pub preliminär: bool,
    pub konton: Vec<Huvudbokskonto>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Huvudbokskonto {
    pub nummer: Kontonummer,
    pub namn: String,
    pub ingående: Saldo,
    pub transaktioner: Vec<Transaktion>,
    pub utgående: Saldo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transaktion {
    pub nummer: String,
    pub datum: Date,
    pub text: String,
    pub debet: Belopp,
    pub kredit: Belopp,
    pub saldo: Saldo,
}

fn urval(u: &Bokföringsurval) -> Value {
    json!({
        "ar": format!("{} – {}", u.år_start, u.år_slut),
        "perioder": if u.från == u.till {
            periodnamn(u.från)
        } else {
            format!("{} – {}", periodnamn(u.från), periodnamn(u.till))
        },
    })
}

fn företag(f: &Företag) -> Value {
    json!({ "namn": f.namn, "orgnr": f.orgnr })
}

pub(crate) fn grundbok(g: &Grundbok) -> Value {
    json!({
        "foretag": företag(&g.företag),
        "urval": urval(&g.urval),
        "perioder": g.perioder.iter().map(|p| json!({
            "namn": periodnamn(p.start),
            "verifikat": p.verifikat.iter().map(|v| json!({
                "nummer": v.nummer,
                "datum": v.datum.to_string(),
                "beskrivning": v.beskrivning,
                "bokford": format!("{} av {}", tid(v.bokförd), v.bokförd_av),
                "summa": Belopp::från_öre(v.rader.iter().map(|r| r.debet.öre()).sum()).to_string(),
                "rader": v.rader.iter().map(|r| json!({
                    "konto": r.konto.map(|k| k.to_string()).unwrap_or_default(),
                    "kontonamn": r.kontonamn,
                    "text": r.text.clone().unwrap_or_default(),
                    "debet": belopp(r.debet),
                    "kredit": belopp(r.kredit),
                })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

pub(crate) fn huvudbok(h: &Huvudbok) -> Value {
    json!({
        "foretag": företag(&h.företag),
        "urval": urval(&h.urval),
        "konton_urval": if h.konton_urval.is_empty() {
            "Alla konton".to_string()
        } else {
            format!("Konton {}", h.konton_urval)
        },
        "ingaende": if h.preliminär { "Ingående balans (preliminär)" } else { "Ingående balans" },
        "konton": h.konton.iter().map(|k| json!({
            "rubrik": format!("{} {}", k.nummer, k.namn),
            "ingaende": k.ingående.to_string(),
            "utgaende": k.utgående.to_string(),
            "transaktioner": k.transaktioner.iter().map(|t| json!({
                "nummer": t.nummer,
                "datum": t.datum.to_string(),
                "text": t.text,
                "debet": belopp(t.debet),
                "kredit": belopp(t.kredit),
                "saldo": t.saldo.to_string(),
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}
