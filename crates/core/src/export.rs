//! Export som CSV för att räkna vidare i ett kalkylprogram, med [`maya_domain::csv`]: UTF-8
//! med BOM, semikolon som avgränsare, decimalkomma och CRLF. Som PDF:erna skrivs filen om vid
//! varje export.

use jiff::civil::Date;
use maya_domain::belopp::Saldo;
use maya_domain::orgnr::Organisationsnummer;

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

/// `behandlingshistorik_<orgnr>_<ÅÅÅÅMMDD>`, utan ändelse: CSV:n och PDF:en heter så.
pub fn behandlingshistorikens_filnamn(orgnr: &Organisationsnummer, idag: Date) -> String {
    format!(
        "behandlingshistorik_{}_{}",
        orgnr.siffror(),
        idag.strftime("%Y%m%d")
    )
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
            "{}.csv",
            behandlingshistorikens_filnamn(&f.orgnr, klocka.idag())
        ),
        innehåll: {
            let mut csv = maya_domain::csv::Csv::new(&[
                "Tidpunkt",
                "Aktör",
                "Kanal",
                "Typ",
                "Objekt",
                "Belopp",
                "Beskrivning",
            ]);
            for rad in rader {
                csv.rad(rad);
            }
            csv.innehåll()
        },
    })
}

pub(crate) use maya_domain::tid::sekund as tidpunkt;

/// Ett belopp som ett kalkylprogram läser som tal, t.ex. `-1234,50`.
pub(crate) fn belopp(saldo: Saldo) -> String {
    maya_domain::csv::belopp(saldo.öre())
}
