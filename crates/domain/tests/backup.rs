use jiff::Timestamp;
use jiff::civil::{Date, Time, date};
use maya_domain::backup::{att_gallra, nästa_körning};

fn ts(s: &str) -> Timestamp {
    s.parse().unwrap()
}

const TRE: Time = Time::constant(3, 0, 0, 0);

#[test]
fn nästa_körning_är_klockslaget_i_stockholm_samma_dag_om_det_inte_har_passerat() {
    // 01:00 i Stockholm (vintertid, UTC+1) → 03:00 samma dag = 02:00 UTC.
    assert_eq!(
        nästa_körning(ts("2026-01-15T00:00:00Z"), TRE),
        ts("2026-01-15T02:00:00Z")
    );
}

#[test]
fn nästa_körning_är_i_morgon_när_klockslaget_har_passerat() {
    // 03:00 i Stockholm sommartid (UTC+2) har passerat kl. 12 UTC → i morgon 01:00 UTC.
    assert_eq!(
        nästa_körning(ts("2026-06-15T12:00:00Z"), TRE),
        ts("2026-06-16T01:00:00Z")
    );
    // Precis på klockslaget räknas som passerat: jobbet kör inte två gånger.
    assert_eq!(
        nästa_körning(ts("2026-06-16T01:00:00Z"), TRE),
        ts("2026-06-17T01:00:00Z")
    );
}

#[test]
fn nästa_körning_följer_sommartiden() {
    // Natten till 29 mars 2026 går klockan från 02:00 till 03:00. 03:00 finns och är 01:00 UTC.
    assert_eq!(
        nästa_körning(ts("2026-03-28T12:00:00Z"), TRE),
        ts("2026-03-29T01:00:00Z")
    );
    // 02:30 finns inte den natten: jobbet går 03:30 sommartid i stället, inte en dag senare.
    assert_eq!(
        nästa_körning(ts("2026-03-28T12:00:00Z"), Time::constant(2, 30, 0, 0)),
        ts("2026-03-29T01:30:00Z")
    );
    // Natten till 25 oktober finns 02:30 två gånger; jobbet går den första.
    assert_eq!(
        nästa_körning(ts("2026-10-24T12:00:00Z"), Time::constant(2, 30, 0, 0)),
        ts("2026-10-25T00:30:00Z")
    );
}

fn dagar(från: Date, antal: i64) -> Vec<Timestamp> {
    (0..antal)
        .map(|i| {
            let d = från.checked_add(jiff::Span::new().days(i)).unwrap();
            d.at(3, 0, 0, 0)
                .to_zoned(maya_domain::tid::stockholm())
                .unwrap()
                .timestamp()
        })
        .collect()
}

#[test]
fn gallringen_behåller_sju_dagliga_och_tolv_månatliga() {
    // En backup per natt i två år, 2025-01-01 – 2026-12-31.
    let alla = dagar(date(2025, 1, 1), 730);
    let gallras = att_gallra(&alla);
    let kvar: Vec<Date> = alla
        .iter()
        .filter(|t| !gallras.contains(t))
        .map(|t| maya_domain::tid::datum(*t))
        .collect();
    let mut väntat: Vec<Date> = (1..=11).map(|m| date(2026, m, 1).last_of_month()).collect();
    väntat.extend((25..=31).map(|d| date(2026, 12, d)));
    assert_eq!(kvar, väntat);
}

#[test]
fn gallringen_behåller_den_senaste_backupen_per_dag_och_räknar_dagar_i_stockholm() {
    // Två backuper samma Stockholmsdag (23:30 UTC 31 dec är 00:30 1 jan i Stockholm).
    let a = ts("2026-01-01T02:00:00Z");
    let b = ts("2026-01-01T12:00:00Z");
    let c = ts("2025-12-31T23:30:00Z");
    let gallras = att_gallra(&[a, b, c]);
    assert_eq!(gallras, vec![c, a]);
}

#[test]
fn gallringen_rör_inget_när_det_finns_färre_än_sju() {
    assert!(att_gallra(&dagar(date(2026, 5, 1), 7)).is_empty());
    assert!(att_gallra(&[]).is_empty());
}
