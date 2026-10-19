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
    assert_eq!(rader.len(), 23);
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
    assert_eq!(
        rader[3],
        [
            "Utfärda eller registrera kundfaktura",
            "1510 totalen",
            "intäktskontot per rad netto; 2611/2621/2631 moms",
            "fakturametoden; datum = fakturadatum; omvänd skattskyldighet och 0 % ger ingen \
             momsrad",
        ]
    );
    assert_eq!(rader[22][0], "T2-samlingsverifikat");

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
    assert!(
        stycken
            .iter()
            .any(|s| s.contains("Utgående moms är 2611 (25 %), 2621 (12 %) och 2631 (6 %).")),
        "{stycken:?}"
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

#[test]
fn behandlingsreglerna_har_spärrarna_vid_betalning() {
    let blocken = maya_domain::systemdokumentation::behandlingsregler();
    let i = blocken
        .iter()
        .position(|b| matches!(b, Block::Underrubrik(t) if t == "Spärrar vid betalning av faktura"))
        .expect("underrubriken");
    let Some(Block::Tabell(tabell)) = blocken.get(i + 2) else {
        panic!("en tabell efter stycket");
    };
    assert_eq!(
        tabell.rader.len(),
        maya_domain::betalning::BETALNINGSSPÄRRAR.len()
    );
}

// #141 Kundförlust
#[test]
fn behandlingsreglerna_har_spärrarna_vid_kundförlust() {
    let blocken = maya_domain::systemdokumentation::behandlingsregler();
    let i = blocken
        .iter()
        .position(|b| matches!(b, Block::Underrubrik(t) if t == "Spärrar vid kundförlust"))
        .expect("underrubriken");
    let Some(Block::Tabell(tabell)) = blocken.get(i + 2) else {
        panic!("en tabell efter stycket");
    };
    assert_eq!(
        tabell.rader.len(),
        maya_domain::kundforlust::KUNDFÖRLUSTSPÄRRAR.len()
    );
}

// #140 Kreditfakturor
#[test]
fn behandlingsreglerna_har_spärrarna_när_en_kreditfaktura_skapas() {
    let blocken = maya_domain::systemdokumentation::behandlingsregler();
    let i = blocken
        .iter()
        .position(
            |b| matches!(b, Block::Underrubrik(t) if t == "Spärrar när en kreditfaktura skapas"),
        )
        .expect("underrubriken");
    let Some(Block::Tabell(tabell)) = blocken.get(i + 2) else {
        panic!("en tabell efter stycket");
    };
    assert_eq!(
        tabell.rader.len(),
        maya_domain::kreditfaktura::KREDITERINGSSPÄRRAR.len()
    );
}

// #155 Fakturaflödena i behandlingsreglerna

/// Blocken under underrubriken `rubrik` fram till nästa underrubrik, som text.
fn under(rubrik: &str) -> String {
    let blocken = behandlingsregler();
    let i = blocken
        .iter()
        .position(|b| matches!(b, Block::Underrubrik(t) if t == rubrik))
        .unwrap_or_else(|| panic!("underrubriken {rubrik} saknas"));
    blocken[i + 1..]
        .iter()
        .take_while(|b| !matches!(b, Block::Underrubrik(_)))
        .map(|b| match b {
            Block::Stycke(t) => t.ren(),
            Block::Lista(l) => l.iter().map(Text::ren).collect::<Vec<_>>().join("\n"),
            Block::Tabell(t) => t
                .rader
                .iter()
                .map(|r| match r {
                    Tabellrad::Rad(c) => c.iter().map(Text::ren).collect::<Vec<_>>().join(" | "),
                    Tabellrad::Rubrik(r) => r.clone(),
                })
                .collect::<Vec<_>>()
                .join("\n"),
            b => format!("{b:?}"),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn intäktskontot_följer_momssatsen_och_den_omvända_skattskyldigheten() {
    let t = under("Intäktskonto per momssats");
    for väntat in [
        "25 % | 3001",
        "12 % | 3002",
        "6 % | 3003",
        "0 % | 3004",
        "Omvänd skattskyldighet, bygg | 3231",
        "Omvänd skattskyldighet, EU-tjänst | 3308",
        "Företaget är inte momsregistrerat | 3004",
        "aktivt intäktskonto i klass 3",
    ] {
        assert!(t.contains(väntat), "saknar {väntat:?}:\n{t}");
    }
}

#[test]
fn behandlingsreglerna_beskriver_fakturaflödenas_regler() {
    for (rubrik, väntade) in [
        (
            "Proportionell fördelning",
            &[
                "Under kontantmetoden, vid delkredit, kundförlust och T2",
                "största rest",
                "exakt",
            ][..],
        ),
        (
            "Leverantörens standardkonto",
            &[
                "automatkontering",
                "standardkonto ändrat",
                "rader utan konto",
            ][..],
        ),
        (
            "Kunder och leverantörer",
            &["kopieras in", "statusen ny", "Maya som aktör", "arkiveras"][..],
        ),
        (
            "Bokföringsmetoden och reskontrakontot",
            &[
                "1510",
                "2440",
                "fakturadatumet",
                "fakturor_efter",
                "efter räkenskapsårets slut",
            ][..],
        ),
        (
            "T2 under kontantmetoden",
            &[
                "årets sista dag",
                "Maya",
                "räknas om",
                "upplåst",
                "vänds aldrig",
            ][..],
        ),
        (
            "Status i reskontran",
            &[
                "obetald",
                "delbetald",
                "förfallen",
                "tillgodo",
                "lagras inte",
            ][..],
        ),
        (
            "Spärrar som håller reskontran i takt med bokföringen",
            &[
                "Rätta registrering",
                "Rätta avskrift",
                "bokfört T2-verifikat",
                "Rätta betalning",
                "Återkalla kundförlust",
                "aterbetalning_fore_tillgodo",
                "fakturor_efter",
                "upplåst",
            ][..],
        ),
    ] {
        let t = under(rubrik);
        for väntat in väntade {
            assert!(t.contains(väntat), "{rubrik}: saknar {väntat:?}:\n{t}");
        }
    }
}

#[test]
fn ändringsloggen_för_0_2_beskriver_de_nya_delsystemen_och_automatkonteringarna() {
    let logg = ändringslogg();
    let versioner: Vec<&str> = logg.iter().map(|r| r.version).collect();
    assert_eq!(versioner, ["0.1", "0.2"]);
    let text = logg[1].ändringar.join("\n");
    for väntat in [
        "Kundreskontra och leverantörsreskontra som delsystem",
        "Automatkonteringarna för fakturaflödena",
        "Fakturaserien",
        "startnummer",
        "1510",
        "7960",
        "4599",
        "T2",
        "Leverantörens standardkonto",
        "Avstämningen",
    ] {
        assert!(text.contains(väntat), "saknar {väntat:?}:\n{text}");
    }
}
