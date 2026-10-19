//! Tabellen över automatkonteringar (S2) byggs ur konstanterna och funktionerna som flödena
//! konterar med, och återges ordagrant som i specen.

use std::collections::HashSet;

use maya_domain::automatkontering::{self, Flöde, Roll};
use maya_domain::kontoplan::flödenas_konton;

/// Specens tabeller för etapp 1 och etapp 2 ordagrant, med styckena runt dem.
const SPECEN: &str = include_str!("golden/automatkonteringar.md");

#[test]
fn tabellen_återges_ordagrant_som_i_specen() {
    let tabell = automatkontering::som_markdown();
    for (rad, (byggd, specen)) in tabell.lines().zip(SPECEN.lines()).enumerate() {
        assert_eq!(byggd, specen, "rad {}", rad + 1);
    }
    assert_eq!(tabell, SPECEN);
}

#[test]
fn varje_flöde_har_en_rad() {
    let flöden: Vec<Flöde> = automatkontering::tabell().iter().map(|r| r.flöde).collect();
    assert_eq!(flöden.len(), 23);
    assert_eq!(flöden.iter().collect::<HashSet<_>>().len(), flöden.len());
    assert_eq!(flöden, Flöde::ALLA);
}

#[test]
fn kontona_som_tabellen_bokför_på_är_flödenas_konton() {
    let flödenas: HashSet<u16> = flödenas_konton().iter().map(|(k, _)| k.get()).collect();
    for rad in automatkontering::tabell() {
        let konton = rad
            .debet
            .konton()
            .into_iter()
            .chain(rad.kredit.konton())
            .chain(rad.debet.roller().into_iter().flat_map(Roll::konton))
            .chain(rad.kredit.roller().into_iter().flat_map(Roll::konton));
        for k in konton {
            assert!(
                flödenas.contains(&k.get()),
                "{:?}: {} är inte ett av flödenas konton",
                rad.flöde,
                k.get()
            );
        }
    }
}

#[test]
fn utfärdandet_krediterar_momsen_på_satsernas_konton() {
    let tabell = automatkontering::tabell();
    let rad = |f: Flöde| tabell.iter().find(|r| r.flöde == f).unwrap();
    let nummer = |k: Vec<maya_domain::kontoplan::Kontonummer>| -> Vec<u16> {
        k.into_iter().map(|k| k.get()).collect()
    };
    let utfärda = rad(Flöde::UtfärdaKundfaktura);
    assert_eq!(nummer(utfärda.debet.konton()), [1510]);
    assert_eq!(nummer(utfärda.kredit.konton()), [2611, 2621, 2631]);
    assert_eq!(utfärda.kredit.roller(), [Roll::Intäkt]);
    let omvänd = rad(Flöde::RegistreraOmvändSkattskyldighet);
    assert_eq!(nummer(omvänd.debet.konton()), [2645, 2647]);
    assert_eq!(nummer(omvänd.kredit.konton()), [2440, 2614, 2624, 2634]);
    assert_eq!(
        nummer(Roll::FiktivUtgåendeMoms.konton()),
        [2614, 2624, 2634]
    );
    assert_eq!(nummer(Roll::UtgåendeMoms.konton()), [2611, 2621, 2631]);
    assert_eq!(nummer(Roll::Betalkonto.konton()), Vec::<u16>::new());
}
