//! Dokumenten som JSON för mallarna. Allt som mallen visar formateras här.

use maya_domain::belopp::{Belopp, Saldo};
use maya_domain::kontoplan::{ink2, klassnamn, om_numret};
use maya_domain::rapport::Radslag;
use serde_json::{Value, json};

use crate::bokforing::{self, Grundbok, Huvudbok};
use crate::{Behandlingshistorik, Kontoplan, Rapport, Sidfot, Signatur, Verifikat};

/// Ett dokument som Maya kan skriva som PDF. Varje variant har en egen mall.
// Ett dokument skapas en gång och lånas till `skriv`; storleken spelar ingen roll.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dokument {
    Verifikat(Verifikat),
    Kontoplan(Kontoplan),
    Behandlingshistorik(Behandlingshistorik),
    Grundbok(Grundbok),
    Huvudbok(Huvudbok),
    Rapport(Rapport),
    Systemdokumentation(maya_domain::systemdokumentation::Systemdokumentation),
}

impl Dokument {
    /// Mallens filnamn i `mallar/`.
    pub(crate) fn mall(&self) -> &'static str {
        match self {
            Dokument::Verifikat(_) => "verifikat.typ",
            Dokument::Kontoplan(_) => "kontoplan.typ",
            Dokument::Behandlingshistorik(_) => "behandlingshistorik.typ",
            Dokument::Grundbok(_) => "grundbok.typ",
            Dokument::Huvudbok(_) => "huvudbok.typ",
            Dokument::Rapport(_) => "rapport.typ",
            Dokument::Systemdokumentation(_) => "systemdokumentation.typ",
        }
    }

    /// Allt mallen läser ur `sys.inputs.data`: `sidfot` och `dokument`.
    pub(crate) fn data(&self, sidfot: &Sidfot) -> Value {
        let dokument = match self {
            Dokument::Verifikat(v) => verifikat(v),
            Dokument::Kontoplan(k) => kontoplan(k),
            Dokument::Behandlingshistorik(b) => behandlingshistorik(b),
            Dokument::Grundbok(g) => bokforing::grundbok(g),
            Dokument::Huvudbok(h) => bokforing::huvudbok(h),
            Dokument::Rapport(r) => rapport(r),
            Dokument::Systemdokumentation(s) => crate::systemdokumentation::systemdokumentation(s),
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

pub(crate) use maya_domain::tid::minut as tid;

use maya_domain::tid::sekund;

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
            "debet": r.debet.eller_tomt(),
            "kredit": r.kredit.eller_tomt(),
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

fn behandlingshistorik(b: &Behandlingshistorik) -> Value {
    let antal = match b.poster.len() {
        1 => "1 post".to_string(),
        n => format!("{n} poster"),
    };
    json!({
        "foretag": { "namn": b.företag.namn, "orgnr": b.företag.orgnr },
        "urval": b.urval,
        "antal": antal,
        "poster": b.poster.iter().map(|p| json!({
            "tid": sekund(p.tid),
            "aktor": p.aktör,
            "typ": p.typ,
            "objekt": p.objekt,
            "beskrivning": p.beskrivning,
        })).collect::<Vec<_>>(),
    })
}

fn rapport(r: &Rapport) -> Value {
    let rapport = &r.rapport;
    let mut anmärkningar = Vec::new();
    if rapport.preliminär_ingående_balans() {
        anmärkningar.push(
            "Ingående balans är preliminär: föregående räkenskapsår är inte stängt, så den räknas \
             ur dess utgående balans och resultatet som inte är ombokat står på 2099.",
        );
    }
    if rapport.första_året() {
        anmärkningar.push(
            "Räkenskapsåret är företagets första i Maya, så det finns inga jämförelsesiffror.",
        );
    }
    anmärkningar.push("Bara bokförda verifikat. Belopp i kronor.");
    let slag = |s: Radslag| match s {
        Radslag::Sida => "sida",
        Radslag::Rubrik => "rubrik",
        Radslag::Underrubrik => "underrubrik",
        Radslag::Ink2Rad => "rad",
        Radslag::Konto => "konto",
        Radslag::Summa => "summa",
        Radslag::Delsumma => "delsumma",
        Radslag::Slutsumma => "slutsumma",
        Radslag::ÅretsResultat => "arets-resultat",
        Radslag::Kontroll => "kontroll",
    };
    json!({
        "foretag": { "namn": r.företag.namn, "orgnr": r.företag.orgnr },
        "titel": rapport.urval.typ.namn(),
        "ar": format!("{} – {}", rapport.år.start(), rapport.år.slut()),
        "period": rapport.period(),
        "anmarkningar": anmärkningar,
        "kolumner": rapport.kolumner.iter().map(|k| json!({
            "rubrik": k.rubrik,
            "underrubrik": k.underrubrik,
            "foregaende": k.föregående,
        })).collect::<Vec<_>>(),
        "rader": rapport.rader.iter().map(|rad| json!({
            "slag": slag(rad.slag),
            "text": match (rad.konto, &rad.ink2) {
                (Some(k), _) => format!("{k} {}", rad.text),
                (None, Some(ink2)) => format!("{ink2} {}", rad.text),
                (None, None) => rad.text.clone(),
            },
            "varden": rad.värden.iter().map(|v| match v {
                Some(öre) => Saldo::från_öre(*öre).to_string(),
                None => "–".to_string(),
            }).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
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
