//! Räkenskapsår och perioder. Det första räkenskapsåret får börja vilken dag som helst, ska
//! sluta vid ett månadsslut och vara högst 18 månader (Å1). Perioderna är kalendermånaderna
//! inom året; den första kan vara en del av en månad.

use std::fmt;

use jiff::civil::Date;

/// Det första räkenskapsåret får omfatta högst så här många perioder.
pub const FÖRSTA_ÅRET_HÖGST_MÅNADER: i32 = 18;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Räkenskapsår {
    start: Date,
    slut: Date,
}

/// En kalendermånad inom ett räkenskapsår, eller den del av den som ligger i året.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Period {
    pub start: Date,
    pub slut: Date,
}

/// Varför ett räkenskapsår nekas. Koden visas vid fältet i formuläret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RäkenskapsårsFel {
    SlutFöreStart,
    SlutInteMånadsslut,
    FörLångt,
}

impl RäkenskapsårsFel {
    pub fn kod(self) -> &'static str {
        match self {
            RäkenskapsårsFel::SlutFöreStart => "ar_slut_fore_start",
            RäkenskapsårsFel::SlutInteMånadsslut => "ar_slut_manadsslut",
            RäkenskapsårsFel::FörLångt => "ar_for_langt",
        }
    }

    pub fn från_kod(kod: &str) -> Option<Self> {
        [
            RäkenskapsårsFel::SlutFöreStart,
            RäkenskapsårsFel::SlutInteMånadsslut,
            RäkenskapsårsFel::FörLångt,
        ]
        .into_iter()
        .find(|f| f.kod() == kod)
    }
}

impl fmt::Display for RäkenskapsårsFel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            RäkenskapsårsFel::SlutFöreStart => "Räkenskapsåret kan inte sluta före starten.",
            RäkenskapsårsFel::SlutInteMånadsslut => {
                "Räkenskapsåret ska sluta den sista dagen i en månad."
            }
            RäkenskapsårsFel::FörLångt => {
                "Det första räkenskapsåret får vara högst 18 månader."
            }
        })
    }
}

impl std::error::Error for RäkenskapsårsFel {}

impl Räkenskapsår {
    /// Företagets första räkenskapsår.
    pub fn första(start: Date, slut: Date) -> Result<Self, RäkenskapsårsFel> {
        if slut < start {
            return Err(RäkenskapsårsFel::SlutFöreStart);
        }
        if slut != slut.last_of_month() {
            return Err(RäkenskapsårsFel::SlutInteMånadsslut);
        }
        let månader = (i32::from(slut.year()) * 12 + i32::from(slut.month()))
            - (i32::from(start.year()) * 12 + i32::from(start.month()))
            + 1;
        if månader > FÖRSTA_ÅRET_HÖGST_MÅNADER {
            return Err(RäkenskapsårsFel::FörLångt);
        }
        Ok(Räkenskapsår { start, slut })
    }

    pub fn start(&self) -> Date {
        self.start
    }

    pub fn slut(&self) -> Date {
        self.slut
    }

    /// Årets perioder i ordning.
    pub fn perioder(&self) -> Vec<Period> {
        let mut perioder = Vec::new();
        let mut start = self.start;
        while start <= self.slut {
            let slut = start.last_of_month();
            perioder.push(Period { start, slut });
            start = slut
                .tomorrow()
                .expect("ett datum efter ett månadsslut i året");
        }
        perioder
    }
}
