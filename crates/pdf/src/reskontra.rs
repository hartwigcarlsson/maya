//! Reskontran per datum (#151, R4, A2): de öppna posterna per faktura och avstämningen mot
//! 1510 och 2440.

use jiff::civil::Date;
use maya_domain::belopp::Saldo;
use serde_json::{Value, json};

use crate::Företag;
use crate::data::företag;

/// Kund- och leverantörsreskontran vid utgången av en dag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reskontran {
    pub företag: Företag,
    pub dag: Date,
    pub delar: Vec<Reskontradel>,
}

/// En reskontra med avstämningen mot sitt konto. Beloppen är i öre, en fordran respektive en
/// skuld positiv.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reskontradel {
    /// "Kundreskontra" eller "Leverantörsreskontra".
    pub rubrik: String,
    /// T.ex. "1510 Kundfordringar".
    pub konto: String,
    pub poster: Vec<Reskontrapost>,
    pub återstår: i64,
    pub bokfört: i64,
    pub saldo: i64,
    pub differens: i64,
}

/// En öppen post.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reskontrapost {
    pub fakturanummer: String,
    pub motpart: String,
    pub fakturadatum: Date,
    pub förfallodag: Option<Date>,
    pub total: i64,
    pub återstår: i64,
    pub bokfört: i64,
    /// T.ex. "Bokförd" eller "Väntar på T2".
    pub läge: String,
}

fn kr(öre: i64) -> String {
    Saldo::från_öre(öre).to_string()
}

pub(crate) fn reskontran(r: &Reskontran) -> Value {
    json!({
        "foretag": företag(&r.företag),
        "dag": r.dag.to_string(),
        "delar": r.delar.iter().map(|d| json!({
            "rubrik": d.rubrik,
            "konto": d.konto,
            "poster": d.poster.iter().map(|p| json!({
                "fakturanummer": p.fakturanummer,
                "motpart": p.motpart,
                "fakturadatum": p.fakturadatum.to_string(),
                "forfallodag": p.förfallodag.map(|d| d.to_string()).unwrap_or_default(),
                "total": kr(p.total),
                "aterstar": kr(p.återstår),
                "bokfort": kr(p.bokfört),
                "lage": p.läge,
            })).collect::<Vec<_>>(),
            "aterstar": kr(d.återstår),
            "bokfort": kr(d.bokfört),
            "saldo": kr(d.saldo),
            "differens": kr(d.differens),
            "stammer": d.differens == 0,
        })).collect::<Vec<_>>(),
    })
}
