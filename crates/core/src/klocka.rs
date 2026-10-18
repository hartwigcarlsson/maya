//! Klockan som användningsfallen läser tiden ur. Datum räknas alltid i `Europe/Stockholm`.

use jiff::Timestamp;
use jiff::civil::Date;

#[derive(Debug, Clone)]
pub struct Klocka(Källa);

#[derive(Debug, Clone)]
enum Källa {
    System,
    #[cfg(feature = "test-support")]
    Fast(Timestamp),
}

impl Klocka {
    /// Systemklockan.
    pub fn system() -> Self {
        Klocka(Källa::System)
    }

    /// En klocka som står still på `nu`, för tester.
    #[cfg(feature = "test-support")]
    pub fn fast(nu: Timestamp) -> Self {
        Klocka(Källa::Fast(nu))
    }

    pub fn nu(&self) -> Timestamp {
        match self.0 {
            Källa::System => Timestamp::now(),
            #[cfg(feature = "test-support")]
            Källa::Fast(nu) => nu,
        }
    }

    /// Dagens datum i Stockholm.
    pub fn idag(&self) -> Date {
        maya_domain::tid::datum(self.nu())
    }
}
