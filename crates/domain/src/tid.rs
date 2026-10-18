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

/// Datumet i Stockholm vid tidpunkten `nu`.
pub fn datum(nu: Timestamp) -> Date {
    nu.to_zoned(stockholm()).date()
}
