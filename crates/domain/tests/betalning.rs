//! Betalningen av en faktura: spärrarna i panelen och konteringen enligt tabellen över
//! automatkonteringar.

use jiff::civil::date;
use maya_domain::belopp::Belopp;
use maya_domain::betalning::{
    BETALNINGSSPÄRRAR, Fält, Förutsättningar, NyBetalning, Reskontra, beskrivning,
};
use maya_domain::kontoplan::Kontonummer;
use maya_domain::rakenskapsar::Räkenskapsår;
use maya_domain::verifikat::Rad;

fn konto(nr: u16) -> Kontonummer {
    Kontonummer::new(nr).unwrap()
}

fn förutsättningar(återstår: i64) -> Förutsättningar {
    Förutsättningar {
        reskontra: Reskontra::Kund,
        återstår,
        konton: vec![
            (konto(1510), true),
            (konto(1910), false),
            (konto(1930), true),
            (konto(1940), true),
            (konto(2440), true),
            (konto(3001), true),
            (konto(3740), true),
        ],
        räkenskapsår: vec![Räkenskapsår::befintligt(
            date(2026, 1, 1),
            date(2026, 12, 31),
        )],
    }
}

fn betalning(betaldag: &str, belopp: &str) -> NyBetalning {
    NyBetalning {
        betaldag: betaldag.into(),
        belopp: belopp.into(),
        konto: "1930".into(),
        öresutjämning: false,
    }
}

fn rad(nr: u16, debet: i64, kredit: i64) -> Rad {
    Rad {
        konto: konto(nr),
        debet: Belopp::från_öre(debet),
        kredit: Belopp::från_öre(kredit),
        text: None,
    }
}

fn koder(b: &NyBetalning, f: &Förutsättningar) -> Vec<&'static str> {
    b.kontrollera(f)
        .err()
        .unwrap_or_default()
        .iter()
        .map(|s| s.kod)
        .collect()
}

#[test]
fn panelen_förväljer_det_som_återstår_dagen_och_1930() {
    let b = NyBetalning::förvald(1_250_050, date(2026, 10, 19));
    assert_eq!(b.betaldag, "2026-10-19");
    assert_eq!(b.belopp, "12 500,50");
    assert_eq!(b.konto, "1930");
    assert!(!b.öresutjämning);
}

#[test]
fn hela_beloppet_bokför_betalkontot_mot_1510() {
    let g = betalning("2026-11-02", "12 500,00")
        .kontrollera(&förutsättningar(1_250_000))
        .unwrap();
    assert_eq!(g.betaldag, date(2026, 11, 2));
    assert_eq!(g.belopp, 1_250_000);
    assert_eq!(g.återstår_efter, 0);
    assert_eq!(
        g.kontering(),
        vec![rad(1930, 1_250_000, 0), rad(1510, 0, 1_250_000)]
    );
}

#[test]
fn en_delbetalning_lämnar_resten_kvar() {
    let g = betalning("2026-11-02", "5000")
        .kontrollera(&förutsättningar(1_250_000))
        .unwrap();
    assert_eq!(g.återstår_efter, 750_000);
    assert_eq!(
        g.kontering(),
        vec![rad(1930, 500_000, 0), rad(1510, 0, 500_000)]
    );
    assert_eq!(betalning("", "5000").rest(1_250_000), Some(750_000));
}

#[test]
fn en_rest_på_högst_10_kr_stängs_mot_öresutjämning_i_samma_verifikat() {
    let b = NyBetalning {
        öresutjämning: true,
        ..betalning("2026-11-02", "12 490")
    };
    let g = b.kontrollera(&förutsättningar(1_250_000)).unwrap();
    assert_eq!(g.öresutjämning, 1_000);
    assert_eq!(g.återstår_efter, 0);
    assert_eq!(
        g.kontering(),
        vec![
            rad(1930, 1_249_000, 0),
            rad(3740, 1_000, 0),
            rad(1510, 0, 1_250_000),
        ]
    );
}

#[test]
fn en_rest_över_10_kr_eller_ingen_rest_kan_inte_öresutjämnas() {
    let för_stor = NyBetalning {
        öresutjämning: true,
        ..betalning("2026-11-02", "12 489,99")
    };
    assert_eq!(
        koder(&för_stor, &förutsättningar(1_250_000)),
        ["oresutjamning_for_stor"]
    );
    let ingen = NyBetalning {
        öresutjämning: true,
        ..betalning("2026-11-02", "12 500")
    };
    assert_eq!(
        koder(&ingen, &förutsättningar(1_250_000)),
        ["oresutjamning_utan_rest"]
    );
}

#[test]
fn en_överbetalning_spärras_och_hänvisas_till_ett_manuellt_verifikat() {
    let spärrar = betalning("2026-11-02", "12 500,01")
        .kontrollera(&förutsättningar(1_250_000))
        .unwrap_err();
    assert_eq!(spärrar.len(), 1);
    assert_eq!(spärrar[0].kod, "overbetalning");
    assert_eq!(spärrar[0].fält, Fält::Belopp);
    assert!(spärrar[0].text.contains("12 500,00"), "{}", spärrar[0].text);
    assert!(spärrar[0].text.contains("manuellt verifikat"), "{}", spärrar[0].text);
}

#[test]
fn en_betald_faktura_tar_inte_emot_fler_betalningar() {
    assert_eq!(
        koder(&betalning("2026-11-02", "1"), &förutsättningar(0)),
        ["redan_betald"]
    );
}

#[test]
fn betaldag_belopp_och_konto_prövas_vid_fälten() {
    let f = förutsättningar(10_000);
    assert_eq!(koder(&betalning("", "1"), &f), ["betaldag_saknas"]);
    assert_eq!(koder(&betalning("2/11", "1"), &f), ["betaldag_ogiltig"]);
    assert_eq!(koder(&betalning("2028-01-01", "1"), &f), ["saknat_rakenskapsar"]);
    assert_eq!(koder(&betalning("2025-12-31", "1"), &f), ["fore_forsta_aret"]);
    assert_eq!(koder(&betalning("2026-11-02", ""), &f), ["belopp_saknas"]);
    assert_eq!(koder(&betalning("2026-11-02", "-5"), &f), ["belopp_ogiltigt"]);
    assert_eq!(koder(&betalning("2026-11-02", "0"), &f), ["belopp_ogiltigt"]);
    let med_konto = |k: &str| NyBetalning {
        konto: k.into(),
        ..betalning("2026-11-02", "1")
    };
    assert_eq!(koder(&med_konto(""), &f), ["konto_saknas"]);
    assert_eq!(koder(&med_konto("1999"), &f), ["konto_okant"]);
    assert_eq!(koder(&med_konto("1910"), &f), ["konto_inaktivt"]);
    assert_eq!(koder(&med_konto("3001"), &f), ["konto_inte_klass_1"]);
    assert!(med_konto("1940").kontrollera(&f).is_ok());
}

#[test]
fn betaldagen_kan_ligga_i_året_efter_det_senaste() {
    let g = betalning("2027-01-15", "100")
        .kontrollera(&förutsättningar(10_000))
        .unwrap();
    assert_eq!(g.räkenskapsår.år().start(), date(2027, 1, 1));
}

#[test]
fn en_leverantörsbetalning_bokför_2440_mot_betalkontot() {
    let f = Förutsättningar {
        reskontra: Reskontra::Leverantör,
        ..förutsättningar(1_250_000)
    };
    let b = NyBetalning {
        öresutjämning: true,
        ..betalning("2026-11-02", "12 495")
    };
    assert_eq!(
        b.kontrollera(&f).unwrap().kontering(),
        vec![
            rad(2440, 1_250_000, 0),
            rad(1930, 0, 1_249_500),
            rad(3740, 0, 500),
        ]
    );
}

#[test]
fn verifikatet_har_fakturan_som_referens() {
    assert_eq!(
        beskrivning("kundfaktura 1007, Kund AB"),
        "Betalning av kundfaktura 1007, Kund AB"
    );
}

#[test]
fn varje_spärr_vid_betalning_är_dokumenterad() {
    let f = förutsättningar(1_250_000);
    let mut fall: Vec<NyBetalning> = vec![
        betalning("", ""),
        betalning("x", "x"),
        betalning("2028-01-01", "12 500,01"),
        betalning("2025-01-01", "0"),
    ];
    for k in ["", "1999", "1910", "3001"] {
        fall.push(NyBetalning {
            konto: k.into(),
            ..betalning("2026-11-02", "1")
        });
    }
    for belopp in ["12 400", "12 500"] {
        fall.push(NyBetalning {
            öresutjämning: true,
            ..betalning("2026-11-02", belopp)
        });
    }
    let mut sedda: Vec<&str> = fall
        .iter()
        .flat_map(|b| koder(b, &f))
        .chain(koder(&betalning("2026-11-02", "1"), &förutsättningar(0)))
        .collect();
    sedda.sort();
    sedda.dedup();
    let mut dokumenterade: Vec<&str> = BETALNINGSSPÄRRAR.iter().map(|(k, _)| *k).collect();
    dokumenterade.sort();
    assert_eq!(sedda, dokumenterade);
}
