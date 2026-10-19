//! De inbyggda behandlingsreglerna som Systemdokumentationen återger (S2, S6).

use maya_domain::systemdokumentation::{
    Block, Stil, Tabellrad, Text, automatkonteringar, behandlingsregler, ändringslogg,
};
use maya_domain::verifikat::{Förutsättningar, NyRad, NyttVerifikat, SPÄRRAR, Spärr};

fn rad(konto: &str, debet: &str, kredit: &str) -> NyRad {
    NyRad {
        konto: konto.into(),
        debet: debet.into(),
        kredit: kredit.into(),
        text: String::new(),
    }
}

#[test]
fn automatkonteringarna_återges_ordagrant_som_i_specen() {
    let blocken = automatkonteringar();
    let Some(Block::Tabell(tabell)) = blocken.iter().find(|b| matches!(b, Block::Tabell(_))) else {
        panic!("ingen tabell i {blocken:?}");
    };
    assert_eq!(tabell.kolumner, ["Flöde", "Debet", "Kredit", "Villkor"]);
    let rader: Vec<Vec<String>> = tabell
        .rader
        .iter()
        .map(|r| match r {
            Tabellrad::Rad(celler) => celler.iter().map(Text::ren).collect(),
            Tabellrad::Rubrik(r) => panic!("rubrik {r}"),
        })
        .collect();
    assert_eq!(rader.len(), 3);
    assert_eq!(rader[0][0], "Rättelse av ett bokfört verifikat");
    assert_eq!(
        rader[0][1],
        "för varje rad i originalet: samma konto med originalets kreditbelopp"
    );
    assert_eq!(rader[1][0], "Registrera om (inget bokförs)");
    assert_eq!(rader[1][2], "");
    assert_eq!(
        rader[2][1],
        "IB(k, n) = UB(k, n−1) för varje konto k i klass 1–2"
    );
    assert!(rader[2][3].starts_with("år n−1 finns i Maya och är inte stängt; därtill IB(2099, n)"));

    // Fetstil och kursiv följer med.
    let Tabellrad::Rad(första) = &tabell.rader[0] else {
        unreachable!()
    };
    assert_eq!(första[0].0[0].text, "Rättelse");
    assert_eq!(första[0].0[0].stil, Stil::Fet);
    let Tabellrad::Rad(andra) = &tabell.rader[1] else {
        unreachable!()
    };
    assert!(
        andra[1]
            .0
            .iter()
            .any(|d| d.text == "kopia av" && d.stil == Stil::Kursiv)
    );

    let stycken: Vec<String> = blocken
        .iter()
        .filter_map(|b| match b {
            Block::Stycke(t) => Some(t.ren()),
            _ => None,
        })
        .collect();
    assert!(
        stycken
            .iter()
            .any(|s| s.contains("Saldo räknas med debet som positivt."))
    );
}

#[test]
fn varje_spärr_vid_bokföring_är_dokumenterad() {
    let förutsättningar = Förutsättningar {
        konton: vec![
            (
                maya_domain::kontoplan::Kontonummer::new(1930).unwrap(),
                true,
            ),
            (
                maya_domain::kontoplan::Kontonummer::new(3001).unwrap(),
                false,
            ),
        ],
        räkenskapsår: vec![maya_domain::rakenskapsar::Räkenskapsår::befintligt(
            "2026-01-01".parse().unwrap(),
            "2026-12-31".parse().unwrap(),
        )],
    };
    let formulär = [
        NyttVerifikat::default(),
        NyttVerifikat {
            datum: "14 mars".into(),
            beskrivning: "x".into(),
            rader: vec![rad("", "1", ""), rad("4711", "", "")],
        },
        NyttVerifikat {
            datum: "2025-03-14".into(),
            beskrivning: "x".into(),
            rader: vec![
                rad("1930", "1", "1"),
                rad("3001", "", "2"),
                rad("1930", "1,234", ""),
            ],
        },
        NyttVerifikat {
            datum: "2028-03-14".into(),
            beskrivning: "x".into(),
            rader: vec![rad("1930", "5", ""), rad("1930", "", "2")],
        },
    ];
    let mut koder: Vec<&str> = formulär
        .iter()
        .flat_map(|f| f.kontrollera(&förutsättningar).err().unwrap_or_default())
        .map(|s: Spärr| s.kod)
        .collect();
    let utan_år = Förutsättningar {
        räkenskapsår: Vec::new(),
        ..förutsättningar.clone()
    };
    koder.extend(
        formulär[2]
            .kontrollera(&utan_år)
            .unwrap_err()
            .into_iter()
            .map(|s| s.kod),
    );
    koder.sort();
    koder.dedup();
    let mut dokumenterade: Vec<&str> = SPÄRRAR.iter().map(|(kod, _)| *kod).collect();
    dokumenterade.sort();
    assert_eq!(koder, dokumenterade);
}

#[test]
fn behandlingsreglerna_beskriver_fristerna_och_vad_maya_inte_ser() {
    let text: String = behandlingsregler()
        .iter()
        .map(|b| format!("{b:?}"))
        .collect();
    for väntat in [
        "50 dagar",
        "första arbetsdagen efter verifikatdatumet",
        "aldrig registreras",
        "bokförs direkt utan att först sparas som utkast",
        "2099",
        "rader_for_fa",
        "fore_originalet",
    ] {
        assert!(text.contains(väntat), "saknar {väntat:?}");
    }
}

#[test]
fn ändringsloggen_har_den_här_versionen() {
    let version = env!("CARGO_PKG_VERSION");
    let release: String = version.split('.').take(2).collect::<Vec<_>>().join(".");
    let logg = ändringslogg();
    assert!(
        logg.iter().any(|r| r.version == release),
        "ändringsloggen saknar {release}"
    );
    assert!(logg.iter().all(|r| !r.ändringar.is_empty()));
}

#[test]
fn behandlingsreglerna_har_spärrarna_vid_utfärdande_av_kundfaktura() {
    let blocken = maya_domain::systemdokumentation::behandlingsregler();
    let i = blocken
        .iter()
        .position(
            |b| matches!(b, Block::Underrubrik(t) if t == "Spärrar vid utfärdande av kundfaktura"),
        )
        .expect("underrubriken");
    let Some(Block::Tabell(tabell)) = blocken.get(i + 2) else {
        panic!("en tabell efter stycket");
    };
    assert_eq!(
        tabell.rader.len(),
        maya_domain::kundfaktura::UTFÄRDANDESPÄRRAR.len()
    );
}
