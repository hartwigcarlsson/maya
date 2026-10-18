//! Egen testbinär, eftersom testet ändrar processens `TZ`.

use jiff::{Timestamp, civil::date};
use maya_core::Klocka;

#[test]
#[allow(unsafe_code)]
fn datum_raknas_i_stockholm_aven_nar_vardens_tz_ar_en_annan() {
    // SAFETY: det enda testet i binären, och ingen annan tråd läser miljön.
    unsafe { std::env::set_var("TZ", "America/Los_Angeles") };

    // 22:30 UTC är 00:30 nästa dag i Stockholm sommartid (+02) och 15:30 samma dag i Los Angeles.
    let sommar: Timestamp = "2026-03-31T22:30:00Z".parse().unwrap();
    assert_eq!(Klocka::fast(sommar).idag(), date(2026, 4, 1));

    // 23:30 UTC är 00:30 nästa dag i Stockholm normaltid (+01).
    let vinter: Timestamp = "2026-12-31T23:30:00Z".parse().unwrap();
    assert_eq!(Klocka::fast(vinter).idag(), date(2027, 1, 1));

    // 22:30 UTC är fortfarande samma dag i Stockholm på vintern.
    let kväll: Timestamp = "2026-12-30T22:30:00Z".parse().unwrap();
    assert_eq!(Klocka::fast(kväll).idag(), date(2026, 12, 30));
}
