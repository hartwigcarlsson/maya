//! Arbetsdagar i Sverige, utan inställningar: måndag–fredag utom allmänna helgdagar
//! (lag 1989:253), midsommarafton, julafton och nyårsafton. Kontantfristen (BFL 5:2) räknas med
//! dem.

use jiff::ToSpan;
use jiff::civil::{Date, Weekday, date};

/// Om `dag` är en arbetsdag.
pub fn är_arbetsdag(dag: Date) -> bool {
    !matches!(dag.weekday(), Weekday::Saturday | Weekday::Sunday) && !är_ledig(dag)
}

/// Första arbetsdagen efter `dag`.
pub fn första_arbetsdagen_efter(dag: Date) -> Date {
    let mut d = nästa(dag);
    while !är_arbetsdag(d) {
        d = nästa(d);
    }
    d
}

fn nästa(dag: Date) -> Date {
    dag.checked_add(1.day()).unwrap_or(dag)
}

/// Helgdagar och aftnar som inte är arbetsdagar, oavsett veckodag.
fn är_ledig(dag: Date) -> bool {
    let (år, månad, d) = (dag.year(), dag.month(), dag.day());
    let fasta = matches!(
        (månad, d),
        (1, 1) | (1, 6) | (5, 1) | (6, 6) | (12, 24) | (12, 25) | (12, 26) | (12, 31)
    );
    if fasta {
        return true;
    }
    let påsk = påskdagen(år);
    let från_påsk = |dagar: i64| påsk.checked_add(dagar.days()).ok();
    let rörliga = [
        från_påsk(-2), // långfredagen
        från_påsk(1),  // annandag påsk
        från_påsk(39), // Kristi himmelsfärdsdag
        från_påsk(49), // pingstdagen
    ];
    if rörliga.contains(&Some(dag)) {
        return true;
    }
    // Midsommardagen är lördagen 20–26 juni och midsommarafton dagen före; alla helgons dag är
    // lördagen 31 oktober–6 november.
    let midsommardagen =
        (månad == 6 && (20..=26).contains(&d)) && dag.weekday() == Weekday::Saturday;
    let midsommarafton = (månad == 6 && (19..=25).contains(&d)) && dag.weekday() == Weekday::Friday;
    let alla_helgons =
        ((månad == 10 && d == 31) || (månad == 11 && d <= 6)) && dag.weekday() == Weekday::Saturday;
    midsommardagen || midsommarafton || alla_helgons
}

/// Påskdagen i den gregorianska kalendern (den anonyma algoritmen, Meeus/Jones/Butcher).
fn påskdagen(år: i16) -> Date {
    let y = i32::from(år);
    let a = y % 19;
    let b = y / 100;
    let c = y % 100;
    let d = b / 4;
    let e = b % 4;
    let f = (b + 8) / 25;
    let g = (b - f + 1) / 3;
    let h = (19 * a + b - d - g + 15) % 30;
    let i = c / 4;
    let k = c % 4;
    let l = (32 + 2 * e + 2 * i - h - k) % 7;
    let m = (a + 11 * h + 22 * l) / 451;
    let månad = (h + l - 7 * m + 114) / 31;
    let dag = (h + l - 7 * m + 114) % 31 + 1;
    // Månaden är 3 eller 4 och dagen 1–31, så omvandlingarna kan inte misslyckas.
    date(år, månad as i8, dag as i8)
}
