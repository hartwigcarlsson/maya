//! Systemdokumentationen (S1–S6) som JSON för mallen: avsnitten och deras block som de
//! genereras i `maya_core::systemdokumentation`.

use maya_domain::systemdokumentation::{Block, Stil, Systemdokumentation, Tabellrad, Text};
use serde_json::{Value, json};

fn text(t: &Text) -> Value {
    t.0.iter()
        .map(|d| {
            json!({
                "text": d.text,
                "stil": match d.stil {
                    Stil::Vanlig => "vanlig",
                    Stil::Fet => "fet",
                    Stil::Kursiv => "kursiv",
                },
            })
        })
        .collect()
}

fn block(b: &Block) -> Value {
    match b {
        Block::Underrubrik(t) => json!({ "typ": "underrubrik", "text": t }),
        Block::Stycke(t) => json!({ "typ": "stycke", "text": text(t) }),
        Block::Lista(punkter) => {
            json!({ "typ": "lista", "punkter": punkter.iter().map(text).collect::<Vec<_>>() })
        }
        Block::Fakta(rader) => json!({
            "typ": "fakta",
            "rader": rader
                .iter()
                .map(|(rubrik, värde)| json!({ "rubrik": rubrik, "varde": text(värde) }))
                .collect::<Vec<_>>(),
        }),
        Block::Tabell(t) => json!({
            "typ": "tabell",
            "kolumner": t.kolumner,
            "rader": t
                .rader
                .iter()
                .map(|r| match r {
                    Tabellrad::Rubrik(r) => json!({ "rubrik": r }),
                    Tabellrad::Rad(celler) => {
                        json!({ "celler": celler.iter().map(text).collect::<Vec<_>>() })
                    }
                })
                .collect::<Vec<_>>(),
        }),
        Block::Länk { text, href } => json!({ "typ": "lank", "text": text, "href": href }),
    }
}

pub(crate) fn systemdokumentation(s: &Systemdokumentation) -> Value {
    json!({
        "foretag": { "namn": s.företag, "orgnr": s.orgnr },
        "version": s.version,
        "ink2": s.ink2,
        "bas": s.bas,
        "avsnitt": s.avsnitt.iter().map(|a| json!({
            "rubrik": a.rubrik,
            "krav": a.krav.join(", "),
            "block": a.block.iter().map(block).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}
