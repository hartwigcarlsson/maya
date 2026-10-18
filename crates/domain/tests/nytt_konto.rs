use maya_domain::kontoplan::{KontoFält, Kontonummer, NyttKonto, om_numret};

fn nytt(nummer: &str, namn: &str) -> NyttKonto {
    NyttKonto {
        nummer: nummer.into(),
        namn: namn.into(),
    }
}

fn koder(konto: NyttKonto) -> Vec<(KontoFält, &'static str)> {
    konto
        .kontrollera()
        .unwrap_err()
        .into_iter()
        .map(|f| (f.fält, f.kod))
        .collect()
}

#[test]
fn ett_fyrsiffrigt_nummer_i_klass_1_till_8_och_ett_namn_ger_kontot() {
    let (nummer, namn) = nytt(" 6542 ", "  Molntjänster ").kontrollera().unwrap();

    assert_eq!(nummer, Kontonummer::new(6542).unwrap());
    assert_eq!(namn, "Molntjänster");
}

#[test]
fn numret_måste_vara_fyra_siffror() {
    assert_eq!(
        koder(nytt("654", "X")),
        [(KontoFält::Nummer, "nummer_format")]
    );
    assert_eq!(
        koder(nytt("65a2", "X")),
        [(KontoFält::Nummer, "nummer_format")]
    );
    assert_eq!(
        koder(nytt("0542", "X")),
        [(KontoFält::Nummer, "nummer_klass")]
    );
}

#[test]
fn klass_9_nekas() {
    let fel = nytt("9100", "Internt").kontrollera().unwrap_err();

    assert_eq!(fel.len(), 1);
    assert_eq!(
        (fel[0].fält, fel[0].kod),
        (KontoFält::Nummer, "nummer_klass_9")
    );
    assert_eq!(fel[0].text, "Klass 9 används inte i Maya.");
}

#[test]
fn namnet_får_inte_vara_tomt() {
    assert_eq!(koder(nytt("6542", "  ")), [(KontoFält::Namn, "namn_tom")]);
}

#[test]
fn numret_ger_klass_kontogrupp_och_ink2_rad() {
    let om = om_numret(Kontonummer::new(1937).unwrap());

    assert_eq!(om.klass, 1);
    assert_eq!(om.klassnamn, "Tillgångar");
    assert_eq!(om.kontogrupp, 19);
    assert_eq!(om.gruppnamn, Some("Kassa och bank"));
    let rader: Vec<_> = om.ink2.iter().map(|r| r.rad()).collect();
    assert_eq!(rader, ["2.26"]);
}

#[test]
fn en_kontogrupp_som_bas_saknar_har_inget_namn() {
    let om = om_numret(Kontonummer::new(3150).unwrap());

    assert_eq!(om.kontogrupp, 31);
    assert_eq!(om.gruppnamn, None);
    assert_eq!(om.klassnamn, "Rörelsens inkomster/intäkter");
}

#[test]
fn ett_konto_med_plus_och_minusfält_visar_båda_raderna() {
    let om = om_numret(Kontonummer::new(8999).unwrap());

    let rader: Vec<_> = om.ink2.iter().map(|r| r.rad()).collect();
    assert_eq!(rader, ["3.26", "3.27"]);
}
