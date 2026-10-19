//! Rätta kontering på en utfärdad kundfaktura (#142): nya intäktskonton per rad och det nya
//! utfärdandeverifikatets rader.

use maya_domain::belopp::Belopp;
use maya_domain::kontering::{Intäktsrad, KONTERINGSSPÄRRAR, NyKontering, ny_kontering};
use maya_domain::kontoplan::Kontonummer;
use maya_domain::kundfaktura::{Fält, Radfält};
use maya_domain::verifikat::Rad;

fn konto(nr: u16) -> Kontonummer {
    Kontonummer::new(nr).unwrap()
}

fn rad(nr: u16, debet: i64, kredit: i64) -> Rad {
    Rad {
        konto: konto(nr),
        debet: Belopp::från_öre(debet),
        kredit: Belopp::från_öre(kredit),
        text: None,
    }
}

/// Två rader på 3001 (9 500 kr och 500 kr) och en på 3003 (400 kr).
fn rader() -> Vec<Intäktsrad> {
    vec![
        Intäktsrad {
            beskrivning: "Konsulttimmar".into(),
            netto: 950_000,
            konto: konto(3001),
        },
        Intäktsrad {
            beskrivning: "Resa".into(),
            netto: 50_000,
            konto: konto(3001),
        },
        Intäktsrad {
            beskrivning: "Handbok".into(),
            netto: 40_000,
            konto: konto(3003),
        },
    ]
}

/// Utfärdandets verifikat för [`rader`]: 1510, intäkterna per konto och momsen.
fn utfärdandet() -> Vec<Rad> {
    vec![
        rad(1510, 1_252_400, 0),
        rad(3001, 0, 1_000_000),
        rad(3003, 0, 40_000),
        rad(2611, 0, 250_000),
        rad(2631, 0, 2_400),
    ]
}

fn konton() -> Vec<(Kontonummer, bool)> {
    [
        (1510, true),
        (2611, true),
        (3001, true),
        (3002, true),
        (3003, true),
        (3010, false),
    ]
    .into_iter()
    .map(|(k, aktiv)| (konto(k), aktiv))
    .collect()
}

fn formulär(konton: &[&str]) -> NyKontering {
    NyKontering {
        konton: konton.iter().map(|k| k.to_string()).collect(),
        orsak: String::new(),
    }
}

fn koder(f: &NyKontering) -> Vec<(Fält, &'static str)> {
    f.kontrollera(&rader(), &konton())
        .unwrap_err()
        .into_iter()
        .map(|s| (s.fält, s.kod))
        .collect()
}

#[test]
fn ett_nytt_konto_per_rad_ger_kontona_i_radernas_ordning() {
    let nya = formulär(&["3002", "3001", "3003"])
        .kontrollera(&rader(), &konton())
        .unwrap();
    assert_eq!(nya, vec![konto(3002), konto(3001), konto(3003)]);
}

#[test]
fn kontot_ska_vara_ett_aktivt_intäktskonto_i_kontoplanen() {
    assert_eq!(
        koder(&formulär(&["", "1510", "3999"])),
        vec![
            (Fält::Rad(0, Radfält::Konto), "rad_konto_saknas"),
            (Fält::Rad(1, Radfält::Konto), "konto_inte_intakt"),
            (Fält::Rad(2, Radfält::Konto), "konto_okant"),
        ]
    );
    assert_eq!(
        koder(&formulär(&["3010", "3001", "3003"])),
        vec![(Fält::Rad(0, Radfält::Konto), "konto_inaktivt")]
    );
}

#[test]
fn en_rättning_utan_något_nytt_konto_spärras() {
    assert_eq!(
        koder(&formulär(&["3001", "3001", "3003"])),
        vec![(Fält::Rader, "kontering_oforandrad")]
    );
}

#[test]
fn ett_konto_för_varje_rad_krävs() {
    assert_eq!(
        koder(&formulär(&["3002", "3001"])),
        vec![(Fält::Rader, "kontering_rader")]
    );
}

#[test]
fn varje_spärr_står_i_tabellen() {
    let mut koder: Vec<&str> = KONTERINGSSPÄRRAR.iter().map(|(k, _)| *k).collect();
    koder.sort();
    assert_eq!(
        koder,
        vec![
            "kontering_oforandrad",
            "kontering_rader",
            "konto_inaktivt",
            "konto_inte_intakt",
            "konto_okant",
            "rad_konto_saknas",
        ]
    );
}

#[test]
fn den_nya_konteringen_byter_bara_intäktsraderna_och_behåller_fordran_och_momsen() {
    let nya = [konto(3002), konto(3001), konto(3003)];
    assert_eq!(
        ny_kontering(&utfärdandet(), &rader(), &nya),
        vec![
            rad(1510, 1_252_400, 0),
            rad(3002, 0, 950_000),
            rad(3001, 0, 50_000),
            rad(3003, 0, 40_000),
            rad(2611, 0, 250_000),
            rad(2631, 0, 2_400),
        ]
    );
}

#[test]
fn rader_som_hamnar_på_samma_konto_slås_ihop() {
    let nya = [konto(3002), konto(3002), konto(3002)];
    assert_eq!(
        ny_kontering(&utfärdandet(), &rader(), &nya),
        vec![
            rad(1510, 1_252_400, 0),
            rad(3002, 0, 1_040_000),
            rad(2611, 0, 250_000),
            rad(2631, 0, 2_400),
        ]
    );
}

#[test]
fn en_negativ_rad_hamnar_i_debet() {
    let rader = vec![Intäktsrad {
        beskrivning: "Kredit".into(),
        netto: -10_000,
        konto: konto(3001),
    }];
    let utfärdandet = vec![
        rad(1510, 0, 12_500),
        rad(3001, 10_000, 0),
        rad(2611, 2_500, 0),
    ];
    assert_eq!(
        ny_kontering(&utfärdandet, &rader, &[konto(3002)]),
        vec![
            rad(1510, 0, 12_500),
            rad(3002, 10_000, 0),
            rad(2611, 2_500, 0)
        ]
    );
}
