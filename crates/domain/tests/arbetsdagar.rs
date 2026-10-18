use jiff::civil::{Date, date};
use maya_domain::arbetsdagar::{första_arbetsdagen_efter, är_arbetsdag};

fn frist(datum: Date) -> Date {
    första_arbetsdagen_efter(datum)
}

#[test]
fn en_vanlig_vardag_ger_nästa_dag() {
    assert_eq!(frist(date(2026, 10, 13)), date(2026, 10, 14));
}

#[test]
fn fredag_och_helg_ger_måndag() {
    assert_eq!(frist(date(2026, 10, 16)), date(2026, 10, 19));
    assert_eq!(frist(date(2026, 10, 17)), date(2026, 10, 19));
    assert_eq!(frist(date(2026, 10, 18)), date(2026, 10, 19));
}

#[test]
fn påsken_hoppas_över() {
    // Påskdagen 2026 är 5 april: långfredag 3 april och annandag påsk 6 april.
    assert_eq!(frist(date(2026, 4, 2)), date(2026, 4, 7));
    // Påskdagen 2027 är 28 mars.
    assert_eq!(frist(date(2027, 3, 25)), date(2027, 3, 30));
}

#[test]
fn rörliga_och_fasta_helgdagar_hoppas_över() {
    // Kristi himmelsfärdsdag 14 maj 2026.
    assert_eq!(frist(date(2026, 5, 13)), date(2026, 5, 15));
    // Första maj, en fredag 2026.
    assert_eq!(frist(date(2026, 4, 30)), date(2026, 5, 4));
    // Nationaldagen, en fredag 2025.
    assert_eq!(frist(date(2025, 6, 5)), date(2025, 6, 9));
    // Trettondedag jul, en onsdag 2027.
    assert_eq!(frist(date(2027, 1, 5)), date(2027, 1, 7));
}

#[test]
fn midsommarafton_är_ingen_arbetsdag() {
    // Midsommarafton 2026 är fredag 19 juni.
    assert!(!är_arbetsdag(date(2026, 6, 19)));
    assert_eq!(frist(date(2026, 6, 18)), date(2026, 6, 22));
    // 2027 är midsommarafton fredag 25 juni; fredag 18 juni är en arbetsdag.
    assert!(är_arbetsdag(date(2027, 6, 18)));
    assert_eq!(frist(date(2027, 6, 24)), date(2027, 6, 28));
}

#[test]
fn julafton_juldagarna_och_nyårsafton_hoppas_över() {
    assert_eq!(frist(date(2026, 12, 23)), date(2026, 12, 28));
    // Nyårsafton torsdag, nyårsdagen fredag.
    assert_eq!(frist(date(2026, 12, 30)), date(2027, 1, 4));
    assert!(!är_arbetsdag(date(2026, 12, 31)));
    assert!(!är_arbetsdag(date(2026, 12, 24)));
}
