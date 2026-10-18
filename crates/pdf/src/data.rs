//! Dokumenten som JSON för mallarna. Allt som mallen visar formateras här.

use jiff::Timestamp;
use maya_domain::belopp::Belopp;
use maya_domain::kontoplan::{ink2, klassnamn, om_numret};
use serde_json::{Value, json};

use crate::{Kontoplan, Sidfot, Signatur, Verifikat};

/// Ett dokument som Maya kan skriva som PDF. Varje variant har en egen mall.
// Ett dokument skapas en gång och lånas till `skriv`; storleken spelar ingen roll.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dokument {
    Verifikat(Verifikat),
    Kontoplan(Kontoplan),
}

impl Dokument {
    /// Mallens filnamn i `mallar/`.
    pub(crate) fn mall(&self) -> &'static str {
        match self {
            Dokument::Verifikat(_) => "verifikat.typ",
            Dokument::Kontoplan(_) => "kontoplan.typ",
        }
    }

    /// Allt mallen läser ur `sys.inputs.data`: `sidfot` och `dokument`.
    pub(crate) fn data(&self, sidfot: &Sidfot) -> Value {
        let dokument = match self {
            Dokument::Verifikat(v) => verifikat(v),
            Dokument::Kontoplan(k) => kontoplan(k),
        };
        json!({
            "sidfot": {
                "tidpunkt": tid(sidfot.tidpunkt),
                "anvandare": sidfot.användare,
                "version": env!("CARGO_PKG_VERSION"),
                "ink2": ink2::UTGÅVA,
            },
            "dokument": dokument,
        })
    }
}

/// Tid i Stockholm, till minuten.
fn tid(t: Timestamp) -> String {
    t.to_zoned(maya_domain::tid::stockholm())
        .strftime("%Y-%m-%d %H:%M")
        .to_string()
}

/// Tomt för noll, som i dokumentvyn.
fn belopp(b: Belopp) -> String {
    if b == Belopp::NOLL {
        String::new()
    } else {
        b.to_string()
    }
}

fn signatur(s: &Signatur) -> Value {
    json!({ "tid": tid(s.tid), "namn": s.namn, "kanal": s.kanal })
}

fn storlek(byte: u64) -> String {
    const KB: u64 = 1024;
    match byte {
        b if b < KB => format!("{b} B"),
        b if b < KB * KB => format!("{} kB", b.div_ceil(KB)),
        b => {
            let tiondelar = (b * 10).div_ceil(KB * KB);
            format!("{},{} MB", tiondelar / 10, tiondelar % 10)
        }
    }
}

fn verifikat(v: &Verifikat) -> Value {
    let summa =
        |f: fn(&crate::Rad) -> Belopp| Belopp::från_öre(v.rader.iter().map(|r| f(r).öre()).sum());
    json!({
        "foretag": { "namn": v.företag.namn, "orgnr": v.företag.orgnr },
        "nummer": v.nummer,
        "beskrivning": v.beskrivning,
        "summa": summa(|r| r.debet).to_string(),
        "datum": v.datum.map(|d| d.to_string()),
        "period": v.period,
        "skapad": signatur(&v.skapad),
        "bokford": v.bokförd.as_ref().map(signatur),
        "rader": v.rader.iter().map(|r| json!({
            "konto": r.konto.map(|k| k.to_string()).unwrap_or_default(),
            "kontonamn": r.kontonamn,
            "text": r.text.clone().unwrap_or_default(),
            "debet": belopp(r.debet),
            "kredit": belopp(r.kredit),
        })).collect::<Vec<_>>(),
        "debet": summa(|r| r.debet).to_string(),
        "kredit": summa(|r| r.kredit).to_string(),
        "underlag": v.underlag.iter().map(|u| json!({
            "filnamn": u.filnamn,
            "sha256": u.sha256,
            "storlek": storlek(u.storlek),
            "komplettering": u.komplettering.as_ref().map(signatur),
        })).collect::<Vec<_>>(),
        "historik": v.historik.iter().map(|h| json!({
            "tid": tid(h.tid),
            "typ": h.typ,
            "namn": h.namn,
            "kanal": h.kanal,
        })).collect::<Vec<_>>(),
    })
}

fn kontoplan(k: &Kontoplan) -> Value {
    let mut konton: Vec<_> = k.konton.iter().collect();
    konton.sort_by_key(|k| k.nummer);
    let mut klasser: Vec<Value> = Vec::new();
    let mut aktuell: Option<(u8, Vec<Value>)> = None;
    for konto in konton {
        let klass = konto.nummer.klass();
        if aktuell.as_ref().is_some_and(|(k, _)| *k != klass)
            && let Some((k, rader)) = aktuell.take()
        {
            klasser.push(json!({ "rubrik": format!("{k} {}", klassnamn(k)), "konton": rader }));
        }
        let ink2 = om_numret(konto.nummer)
            .ink2
            .iter()
            .map(|r| format!("{} {}", r.rad(), r.benämning()))
            .collect::<Vec<_>>()
            .join(" / ");
        aktuell
            .get_or_insert_with(|| (klass, Vec::new()))
            .1
            .push(json!({
                "nummer": konto.nummer.to_string(),
                "namn": konto.namn,
                "aktiv": konto.aktiv,
                "ink2": ink2,
            }));
    }
    if let Some((k, rader)) = aktuell {
        klasser.push(json!({ "rubrik": format!("{k} {}", klassnamn(k)), "konton": rader }));
    }
    json!({
        "foretag": { "namn": k.företag.namn, "orgnr": k.företag.orgnr },
        "bas_version": k.bas_version,
        "antal": k.konton.len(),
        "klasser": klasser,
    })
}

#[cfg(test)]
mod tests {
    use super::storlek;

    #[test]
    fn storlekar() {
        assert_eq!(storlek(512), "512 B");
        assert_eq!(storlek(48_213), "48 kB");
        assert_eq!(storlek(1_234_567), "1,2 MB");
    }
}
