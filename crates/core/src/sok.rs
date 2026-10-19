//! Sökningen i ⌘K och sökfältet (story 76): verifikaten, kund- och leverantörsfakturorna,
//! kunderna och leverantörerna i ett företag som en fråga träffar, med reglerna i
//! `maya_domain::sok`. Andra företag söks aldrig.

use maya_domain::belopp::Belopp;
use maya_domain::sok::{Fakturasökbart, Fråga};

use crate::Databas;
use crate::aktor::Aktör;
use crate::atkomst::ÅtkomstFel;
use crate::foretag::FöretagsId;
use crate::kund::{self, Kund};
use crate::kundfaktura::{self, Kundfakturarad};
use crate::leverantor::{self, Leverantör, Urval};
use crate::leverantorsfaktura::{self, Fakturarad};
use crate::verifikat::{self, Verifikatrad};

/// Det som en fråga träffar, varje slag i sin listas ordning.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Träffar {
    pub verifikat: Vec<Verifikatrad>,
    /// Mayas egna och de externa, också kreditfakturor och utkast.
    pub kundfakturor: Vec<Kundfakturarad>,
    pub leverantörsfakturor: Vec<Fakturarad>,
    /// Aktiva och arkiverade kunder; `status` märker de arkiverade.
    pub kunder: Vec<Kund>,
    /// Aktiva och arkiverade leverantörer; `status` märker de arkiverade.
    pub leverantörer: Vec<Leverantör>,
}

/// Söker i företaget. En fakturas nummer, belopp, datum och motpart träffar den; en kund och en
/// leverantör träffas av namnet och organisationsnumret. En tom fråga träffar inget, men
/// åtkomsten prövas ändå.
pub async fn sök(
    db: &Databas,
    aktör: &Aktör,
    företag: FöretagsId,
    fråga: &str,
) -> Result<Träffar, ÅtkomstFel> {
    let verifikat = verifikat::sök(db, aktör, företag, fråga).await?;
    let Some(fråga) = Fråga::tolka(fråga) else {
        return Ok(Träffar::default());
    };
    let belopp = |öre: &[i64]| -> Vec<Belopp> {
        öre.iter()
            .filter(|b| **b != 0)
            .map(|b| Belopp::från_öre(b.abs()))
            .collect()
    };
    let kundfakturor = kundfaktura::lista(db, aktör, företag)
        .await?
        .into_iter()
        .filter(|f| {
            fråga.träffar_faktura(&Fakturasökbart {
                nummer: &f.fakturanummer,
                leverantörsfaktura: false,
                datum: f.fakturadatum.into_iter().chain(f.förfallodag).collect(),
                texter: vec![f.kund.as_str()],
                belopp: belopp(&[f.total, f.återstår]),
            })
        })
        .collect();
    let leverantörsfakturor = leverantorsfaktura::lista(db, aktör, företag)
        .await?
        .into_iter()
        .filter(|f| {
            fråga.träffar_faktura(&Fakturasökbart {
                nummer: &f.fakturanummer,
                leverantörsfaktura: true,
                datum: f.fakturadatum.into_iter().chain(f.förfallodag).collect(),
                texter: vec![f.leverantörsnamn.as_str()],
                belopp: belopp(&[f.total.unwrap_or_default(), f.återstår]),
            })
        })
        .collect();
    let kunder = kund::kunder(db, aktör, företag)
        .await?
        .into_iter()
        .filter(|k| fråga.träffar_text(&format!("{}\n{}", k.namn, k.identitet)))
        .collect();
    let leverantörer = leverantor::lista(db, aktör, företag, Urval::Alla)
        .await?
        .into_iter()
        .filter(|l| fråga.träffar_text(&format!("{}\n{}", l.uppgifter.namn, l.uppgifter.orgnr)))
        .collect();
    Ok(Träffar {
        verifikat,
        kundfakturor,
        leverantörsfakturor,
        kunder,
        leverantörer,
    })
}
