//! Backupens schema och gallring (beslut i #13): en backup per natt på ett klockslag i
//! `Europe/Stockholm`, och 7 dagliga och 12 månatliga sparas.

use std::collections::BTreeSet;

use jiff::civil::{Date, Time};
use jiff::{Span, Timestamp};

use crate::tid::{datum, stockholm};

/// Antal dagar med den dagens senaste backup som sparas.
pub const DAGLIGA: usize = 7;
/// Antal månader med den månadens senaste backup som sparas.
pub const MÅNATLIGA: usize = 12;

/// Nästa tidpunkt efter `nu` då klockan i Stockholm är `klockslag`. Ett klockslag som inte finns
/// när sommartiden börjar flyttas framåt lika mycket som klockan; ett som finns två gånger när
/// den slutar gäller första gången.
pub fn nästa_körning(nu: Timestamp, klockslag: Time) -> Timestamp {
    let mut dag = datum(nu);
    loop {
        let tid = tidpunkt(dag, klockslag);
        if tid > nu {
            return tid;
        }
        dag = dag
            .checked_add(Span::new().days(1))
            .expect("datum inom jiffs intervall");
    }
}

fn tidpunkt(dag: Date, klockslag: Time) -> Timestamp {
    // Compatible: ett glapp flyttas framåt, ett överlapp ger den första.
    stockholm()
        .to_ambiguous_timestamp(dag.to_datetime(klockslag))
        .compatible()
        .expect("tidpunkt inom jiffs intervall")
}

/// Backuperna i `backuper` (tidpunkter) som ska gallras: allt utom den senaste backupen var och
/// en av de 7 senaste dagarna med backup och var och en av de 12 senaste månaderna med backup,
/// räknat i Stockholm. Svaret är sorterat, äldst först.
pub fn att_gallra(backuper: &[Timestamp]) -> Vec<Timestamp> {
    let mut sorterade = backuper.to_vec();
    sorterade.sort_unstable_by(|a, b| b.cmp(a));
    sorterade.dedup();

    let mut behåll = BTreeSet::new();
    let mut dagar = BTreeSet::new();
    let mut månader = BTreeSet::new();
    for tid in &sorterade {
        let dag = datum(*tid);
        if dagar.len() < DAGLIGA && dagar.insert(dag) {
            behåll.insert(*tid);
        }
        let månad = (dag.year(), dag.month());
        if månader.len() < MÅNATLIGA && månader.insert(månad) {
            behåll.insert(*tid);
        }
    }
    let mut gallras: Vec<Timestamp> = sorterade
        .into_iter()
        .filter(|t| !behåll.contains(t))
        .collect();
    gallras.reverse();
    gallras
}
