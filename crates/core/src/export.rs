//! Export som CSV för att räkna vidare i ett kalkylprogram: UTF-8 med BOM, semikolon som
//! avgränsare, decimalkomma och CRLF. Som PDF:erna skrivs filen om vid varje export.

use maya_domain::belopp::Saldo;

use crate::aktor::Aktör;
use crate::atkomst::ÅtkomstFel;
use crate::behandlingshistorik::{self, Urval};
use crate::foretag::{self, FöretagsId};
use crate::{Databas, Klocka};

/// En färdig CSV-fil med ett filnamn att spara den under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Csv {
    pub filnamn: String,
    pub innehåll: Vec<u8>,
}

/// Behandlingshistoriken i urvalet (A2), en rad per post i den ordning posterna skrevs.
/// Tidpunkten är i Stockholm, till sekunden.
pub async fn behandlingshistorik(
    db: &Databas,
    klocka: &Klocka,
    aktör: &Aktör,
    företag: FöretagsId,
    urval: &Urval,
) -> Result<Csv, ÅtkomstFel> {
    let historik = behandlingshistorik::historik(db, aktör, företag, urval).await?;
    let f = foretag::företaget(db, aktör, företag).await?;
    let rader = historik.poster.iter().map(|p| {
        vec![
            tidpunkt(p.tid),
            p.aktörsnamn(),
            p.aktör.kanal.namn().to_string(),
            p.typ.clone(),
            p.objekt.clone(),
            p.belopp().map(belopp).unwrap_or_default(),
            p.text(),
        ]
    });
    Ok(Csv {
        filnamn: format!(
            "behandlingshistorik_{}_{}.csv",
            f.orgnr.siffror(),
            klocka.idag().strftime("%Y%m%d")
        ),
        innehåll: skriv(
            &[
                "Tidpunkt",
                "Aktör",
                "Kanal",
                "Typ",
                "Objekt",
                "Belopp",
                "Beskrivning",
            ],
            rader,
        ),
    })
}

/// Tid i Stockholm, till sekunden: `2026-03-14 10:05:12`.
pub(crate) fn tidpunkt(t: jiff::Timestamp) -> String {
    t.to_zoned(maya_domain::tid::stockholm())
        .strftime("%Y-%m-%d %H:%M:%S")
        .to_string()
}

/// Ett belopp som ett kalkylprogram läser som tal: decimalkomma, inga tusentalsavgränsare och
/// minus framför, t.ex. `-1234,50`.
pub(crate) fn belopp(saldo: Saldo) -> String {
    let öre = saldo.öre();
    let tecken = if öre < 0 { "-" } else { "" };
    let öre = öre.unsigned_abs();
    format!("{tecken}{},{:02}", öre / 100, öre % 100)
}

/// Rubrikraden och raderna som CSV. Ett fält med semikolon, citattecken eller radbrytning
/// sätts inom citattecken, med citattecknen dubblerade.
pub(crate) fn skriv(rubriker: &[&str], rader: impl IntoIterator<Item = Vec<String>>) -> Vec<u8> {
    let mut ut = String::from("\u{feff}");
    let mut rad = |fält: &mut dyn Iterator<Item = &str>| {
        let fält: Vec<String> = fält.map(citera).collect();
        ut.push_str(&fält.join(";"));
        ut.push_str("\r\n");
    };
    rad(&mut rubriker.iter().copied());
    for r in rader {
        rad(&mut r.iter().map(String::as_str));
    }
    ut.into_bytes()
}

fn citera(fält: &str) -> String {
    if fält.contains([';', '"', '\r', '\n']) {
        format!("\"{}\"", fält.replace('"', "\"\""))
    } else {
        fält.to_string()
    }
}
