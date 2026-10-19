//! Kundfakturan som JSON för `mallar/kundfaktura.typ`.

use maya_domain::belopp::Saldo;
use maya_domain::faktura::Rabatt;
use maya_domain::kundfaktura::Momsläge;
use serde_json::{Value, json};

use crate::Kundfaktura;

/// Ett belopp i öre med tecken, som fakturan skriver det: `8 550,00` eller `-400,00`.
fn kr(öre: i64) -> String {
    Saldo::från_öre(öre).to_string()
}

pub(crate) fn kundfaktura(k: &Kundfaktura) -> Value {
    let f = &k.faktura;
    let s = &k.fakturaföretag;
    let mut betalsätt = Vec::new();
    if !s.bankgiro.is_empty() {
        betalsätt.push(json!(["Bankgiro", s.bankgiro]));
    }
    if !s.plusgiro.is_empty() {
        betalsätt.push(json!(["Plusgiro", s.plusgiro]));
    }
    if !s.iban.is_empty() && !s.bic.is_empty() {
        betalsätt.push(json!(["IBAN", s.iban]));
        betalsätt.push(json!(["BIC", s.bic]));
    }
    let har_rabatt = f.rader.iter().any(|r| r.rabatt != Rabatt::INGEN);
    let logotyp = k.logotyp.as_ref().map(|l| match l.mime.as_str() {
        "image/jpeg" => "jpg",
        "image/svg+xml" => "svg",
        _ => "png",
    });
    // Ett företag som inte är momsregistrerat har ingen moms och inget momsreg-nr på fakturan.
    let med_moms = f.momsläge != Momsläge::UtanMoms;
    json!({
        "utkast": k.utkast,
        "med_moms": med_moms,
        "omvand": f.momsläge.fakturatext(),
        // #140: en kreditfakturas hänvisning till originalet (momslagen 17 kap. 24 §).
        "kredit": f.krediterar.as_ref().map(|h| json!({
            "text": h.text(),
            "nummer": h.nummer.to_string(),
        })),
        "logotyp": logotyp,
        "nummer": f.nummer.to_string(),
        "referens": f.referens,
        "ocr": s.ocr,
        "fakturadatum": f.fakturadatum.to_string(),
        "forfallodag": f.förfallodag.to_string(),
        "leveransdatum": f.leveransdatum,
        "er_referens": f.er_referens,
        "saljare": {
            "namn": s.namn,
            "orgnr": s.orgnr,
            "adress": [s.gatuadress.clone(), format!("{} {}", s.postnummer, s.postort)],
            "sate": s.säte,
            "momsregnr": if med_moms { s.momsregnr.as_str() } else { "" },
            "f_skatt": s.f_skatt,
            "betalsatt": betalsätt,
        },
        "kund": {
            "namn": f.kund.namn,
            "adress": f.kund.adress.lines().collect::<Vec<_>>(),
            "momsregnr": if med_moms { f.kund.momsregnr.as_str() } else { "" },
        },
        "har_rabatt": har_rabatt,
        "rader": f.rader.iter().map(|r| json!({
            "beskrivning": r.beskrivning,
            "antal": if r.enhet.is_empty() {
                r.antal.to_string()
            } else {
                format!("{} {}", r.antal, r.enhet)
            },
            "a_pris": kr(r.à_pris),
            "rabatt": if r.rabatt == Rabatt::INGEN {
                String::new()
            } else {
                format!("{} %", r.rabatt)
            },
            "moms": r.momssats.to_string(),
            "netto": kr(r.netto),
        })).collect::<Vec<_>>(),
        "momsrader": f.momsrader.iter().map(|m| json!({
            "text": format!("Moms {} på {}", m.sats, kr(m.underlag)),
            "moms": kr(m.moms),
        })).collect::<Vec<_>>(),
        "netto": kr(f.netto),
        "moms": kr(f.moms),
        "total": kr(f.total),
    })
}
