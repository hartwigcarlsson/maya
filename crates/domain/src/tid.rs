//! Mayas tid: alla datum, frister och jobb räknas i `Europe/Stockholm`, oavsett värdens
//! tidszon. Tidszonsdatabasen för Stockholm är inbyggd vid kompileringen, så den behövs inte
//! på värden och fungerar i WASM.

use jiff::Timestamp;
use jiff::civil::Date;
use jiff::tz::TimeZone;

/// Tidszonen `Europe/Stockholm`.
pub fn stockholm() -> TimeZone {
    static STOCKHOLM: TimeZone = jiff::tz::get!("Europe/Stockholm");
    STOCKHOLM.clone()
}

/// Tidpunkten i Stockholm till minuten, som Maya visar tider: `2026-03-14 10:05`.
pub fn minut(t: Timestamp) -> String {
    t.to_zoned(stockholm())
        .strftime("%Y-%m-%d %H:%M")
        .to_string()
}

/// Tidpunkten i Stockholm till sekunden, som behandlingshistorikens exporter: `2026-03-14
/// 10:05:12`.
pub fn sekund(t: Timestamp) -> String {
    t.to_zoned(stockholm())
        .strftime("%Y-%m-%d %H:%M:%S")
        .to_string()
}

/// Datumet i Stockholm vid tidpunkten `nu`.
pub fn datum(nu: Timestamp) -> Date {
    nu.to_zoned(stockholm()).date()
}
