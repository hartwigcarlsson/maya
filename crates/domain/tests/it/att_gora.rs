use jiff::civil::{Date, date};
use maya_domain::att_gora::{
    Frist, Utkastläge, femtiodagarsvarning, frist, länge_okopplat, ordna_utkast, rör_kassan, sedan,
};
use maya_domain::kontoplan::Kontonummer;

const IDAG: Date = date(2026, 10, 14);

fn konton(nr: &[u16]) -> Vec<Kontonummer> {
    nr.iter().map(|n| Kontonummer::new(*n).unwrap()).collect()
}

#[derive(Debug, Clone, PartialEq)]
struct U {
    namn: &'static str,
    datum: Option<Date>,
    konton: Vec<u16>,
}

fn u(namn: &'static str, datum: Option<Date>, konton: &[u16]) -> U {
    U {
        namn,
        datum,
        konton: konton.to_vec(),
    }
}

fn läge(u: &U) -> Utkastläge {
    Utkastläge {
        datum: u.datum,
        kassa: rör_kassan(konton(&u.konton)),
    }
}

fn namn(l: &[U]) -> Vec<&'static str> {
    l.iter().map(|u| u.namn).collect()
}

#[test]
fn kassan_är_grupp_191_också_egna_konton() {
    assert!(rör_kassan(konton(&[5410, 1910])));
    assert!(rör_kassan(konton(&[1919, 3001])));
    assert!(!rör_kassan(konton(&[1930, 3001])));
    assert!(!rör_kassan(konton(&[1900, 1920])));
}

#[test]
fn femtiodagarsvarningen_räknas_från_sista_dagen_i_månaden() {
    // Juli slutar 31 juli; 19 september är 50 dagar senare, 20 september 51.
    let datum = date(2026, 7, 14);
    assert_eq!(femtiodagarsvarning(datum, date(2026, 9, 19)), None);
    assert_eq!(femtiodagarsvarning(datum, date(2026, 9, 20)), Some(51));
    assert_eq!(
        femtiodagarsvarning(date(2026, 7, 31), date(2026, 9, 20)),
        Some(51)
    );
    assert_eq!(
        femtiodagarsvarning(date(2026, 8, 1), date(2026, 9, 20)),
        None
    );
}

#[test]
fn ett_utkast_på_kassan_har_bara_kontantfristen() {
    let gammalt = Utkastläge {
        datum: Some(date(2026, 7, 14)),
        kassa: true,
    };
    assert_eq!(
        frist(&gammalt, IDAG),
        Frist::Kontantfrist {
            senast: date(2026, 7, 15),
            försenad: true
        }
    );
    // Måndag 12 oktober ska bokföras senast tisdag.
    let nytt = Utkastläge {
        datum: Some(date(2026, 10, 12)),
        kassa: true,
    };
    assert_eq!(
        frist(&nytt, IDAG),
        Frist::Kontantfrist {
            senast: date(2026, 10, 13),
            försenad: true
        }
    );
    let i_dag = Utkastläge {
        datum: Some(date(2026, 10, 13)),
        kassa: true,
    };
    assert_eq!(
        frist(&i_dag, IDAG),
        Frist::Kontantfrist {
            senast: date(2026, 10, 14),
            försenad: false
        }
    );
}

#[test]
fn ett_gammalt_utkast_utan_kassa_har_femtiodagarsvarningen() {
    let läge = Utkastläge {
        datum: Some(date(2026, 7, 14)),
        kassa: false,
    };
    assert_eq!(frist(&läge, IDAG), Frist::Femtiodagar { dagar: 75 });
    let utan_datum = Utkastläge {
        datum: None,
        kassa: true,
    };
    assert_eq!(frist(&utan_datum, IDAG), Frist::Ingen);
}

#[test]
fn utkasten_grupperas_per_period_nyast_först_och_kassans_ligger_i_tidslinjen() {
    let utkast = vec![
        u("juli tidigt", Some(date(2026, 7, 14)), &[5410, 1930]),
        u("oktober", Some(date(2026, 10, 2)), &[5410, 1930]),
        u("juli sent", Some(date(2026, 7, 30)), &[]),
        u("utan datum", None, &[1910]),
        u("kassa oktober", Some(date(2026, 10, 12)), &[1910, 3001]),
        u("kassa i dag", Some(date(2026, 10, 14)), &[1930, 1911]),
        u("kassa juni", Some(date(2026, 6, 10)), &[1910, 3001]),
    ];

    let ordning = ordna_utkast(utkast, läge, IDAG);

    let tidslinje: Vec<_> = ordning.kontanta.iter().map(|(f, u)| (*f, u.namn)).collect();
    assert_eq!(
        tidslinje,
        vec![
            (date(2026, 6, 11), "kassa juni"),
            (date(2026, 10, 13), "kassa oktober"),
            (date(2026, 10, 15), "kassa i dag"),
        ]
    );
    let perioder: Vec<_> = ordning
        .perioder
        .iter()
        .map(|p| (p.månad, p.varning, namn(&p.utkast)))
        .collect();
    assert_eq!(
        perioder,
        vec![
            (Some(date(2026, 10, 1)), None, vec!["oktober"]),
            (
                Some(date(2026, 7, 1)),
                Some(75),
                vec!["juli sent", "juli tidigt"]
            ),
            (None, None, vec!["utan datum"]),
        ]
    );
}

#[test]
fn ett_underlag_är_länge_okopplat_efter_30_dagar() {
    assert!(!länge_okopplat(date(2026, 9, 14), IDAG));
    assert!(länge_okopplat(date(2026, 9, 13), IDAG));
}

#[test]
fn när_något_hände_skrivs_som_förfallotexten_för_n_dagar_sedan() {
    assert_eq!(sedan(IDAG, IDAG), "i dag");
    assert_eq!(sedan(date(2026, 10, 13), IDAG), "i går");
    assert_eq!(sedan(date(2026, 3, 5), IDAG), "för 223 dagar sedan");
}
