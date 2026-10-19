//! Reglerna bakom Att göra: tidsfristerna för verifikatutkast (T1) och när ett okopplat
//! underlag har legat länge.
//!
//! - **Kontantfristen** (BFL 5:2): ett utkast med minst en rad, debet eller kredit, på ett konto
//!   i grupp 191 (kassa, också egna konton i gruppen) ska bokföras senast första arbetsdagen
//!   efter verifikatdatumet. Ett sådant utkast visar bara kontantfristen, aldrig
//!   50-dagarsvarningen, eftersom BFNAR 2013:2 p. 3.6 inte gäller kontanta betalningar.
//! - **50-dagarsvarningen**: när verifikatdatumets månad tog slut för mer än 50 dagar sedan.
//!
//! Fristerna varnar men spärrar inte, och Maya bokför aldrig automatiskt.

use jiff::civil::{Date, Weekday};

use crate::arbetsdagar::första_arbetsdagen_efter;
use crate::kontoplan::Kontonummer;

/// Antalet dagar efter månadens slut som ett utkast får vänta innan Att göra varnar.
pub const VARNING_EFTER_DAGAR: i32 = 50;

/// Antalet dagar som ett underlag får ligga okopplat innan det märks "30+ dagar".
pub const OKOPPLAT_LÄNGE_DAGAR: i32 = 30;

/// Om något av kontona hör till grupp 191, kassan.
pub fn rör_kassan(konton: impl IntoIterator<Item = Kontonummer>) -> bool {
    konton.into_iter().any(|k| k.get() / 10 == 191)
}

/// Kontantfristen för ett utkast på kassan med verifikatdatumet `datum`.
pub fn kontantfrist(datum: Date) -> Date {
    första_arbetsdagen_efter(datum)
}

/// Antalet dagar sedan verifikatdatumets månad tog slut, om det är fler än 50.
pub fn femtiodagarsvarning(datum: Date, idag: Date) -> Option<i32> {
    let dagar = dagar_mellan(datum.last_of_month(), idag);
    (dagar > VARNING_EFTER_DAGAR).then_some(dagar)
}

/// Om ett underlag som laddades upp `uppladdat` har legat okopplat i mer än 30 dagar.
pub fn länge_okopplat(uppladdat: Date, idag: Date) -> bool {
    dagar_mellan(uppladdat, idag) > OKOPPLAT_LÄNGE_DAGAR
}

/// Det som avgör ett utkasts frist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Utkastläge {
    pub datum: Option<Date>,
    /// Minst en rad på ett konto i grupp 191.
    pub kassa: bool,
}

/// Fristen som Att göra visar för ett utkast.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frist {
    /// Utkastet för pengar in i eller ut ur kassan och ska bokföras senast `senast`.
    Kontantfrist {
        senast: Date,
        försenad: bool,
    },
    /// Verifikatdatumets månad tog slut för `dagar` dagar sedan, fler än 50.
    Femtiodagar {
        dagar: i32,
    },
    Ingen,
}

/// Fristen för ett utkast i dag.
pub fn frist(läge: &Utkastläge, idag: Date) -> Frist {
    match läge.datum {
        Some(datum) if läge.kassa => {
            let senast = kontantfrist(datum);
            Frist::Kontantfrist {
                senast,
                försenad: senast < idag,
            }
        }
        Some(datum) => match femtiodagarsvarning(datum, idag) {
            Some(dagar) => Frist::Femtiodagar { dagar },
            None => Frist::Ingen,
        },
        None => Frist::Ingen,
    }
}

/// Utkasten i en period (en kalendermånad) i spalten Utan datum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Period<T> {
    /// Månadens första dag, eller `None` för utkasten utan verifikatdatum.
    pub månad: Option<Date>,
    /// 50-dagarsvarningen i periodens rubrik: dagarna sedan månaden tog slut.
    pub varning: Option<i32>,
    /// Nyast verifikatdatum först.
    pub utkast: Vec<T>,
}

/// Utkasten som Att göra visar dem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Utkastordning<T> {
    /// Utkasten med kontantfrist, i tidslinjen på fristens dag, tidigaste fristen först.
    pub kontanta: Vec<(Date, T)>,
    /// Övriga utkast per period, nyaste perioden först och perioden utan datum sist.
    pub perioder: Vec<Period<T>>,
}

/// Delar upp utkasten i de med kontantfrist och de övriga per period.
pub fn ordna_utkast<T>(
    utkast: Vec<T>,
    läge: impl Fn(&T) -> Utkastläge,
    idag: Date,
) -> Utkastordning<T> {
    let mut kontanta = Vec::new();
    let mut övriga: Vec<(Option<Date>, T)> = Vec::new();
    for u in utkast {
        let l = läge(&u);
        match frist(&l, idag) {
            Frist::Kontantfrist { senast, .. } => kontanta.push((senast, u)),
            _ => övriga.push((l.datum, u)),
        }
    }
    kontanta.sort_by_key(|(senast, _)| *senast);
    // Nyast först, utan datum sist; sorteringen är stabil.
    övriga.sort_by(|(a, _), (b, _)| match (a, b) {
        (Some(a), Some(b)) => b.cmp(a),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
    let mut perioder: Vec<Period<T>> = Vec::new();
    for (datum, u) in övriga {
        let månad = datum.map(|d| d.first_of_month());
        match perioder.last_mut() {
            Some(p) if p.månad == månad => p.utkast.push(u),
            _ => perioder.push(Period {
                månad,
                varning: datum.and_then(|d| femtiodagarsvarning(d, idag)),
                utkast: vec![u],
            }),
        }
    }
    Utkastordning { kontanta, perioder }
}

/// Antalet dagar från `från` till `till`.
pub fn dagar_mellan(från: Date, till: Date) -> i32 {
    // Ett datumspann har dagar som största enhet.
    från.until(till).map(|s| s.get_days()).unwrap_or_default()
}

/// Hur gammalt något är: "i dag", "i går" eller "12 dagar".
pub fn ålder(från: Date, idag: Date) -> String {
    match dagar_mellan(från, idag) {
        ..=0 => "i dag".to_string(),
        1 => "i går".to_string(),
        n => format!("{n} dagar"),
    }
}

/// När något hände, som förfallotexten skriver det: "i dag", "i går" eller "för 12 dagar
/// sedan".
pub fn sedan(från: Date, idag: Date) -> String {
    match dagar_mellan(från, idag) {
        ..=0 => "i dag".to_string(),
        1 => "i går".to_string(),
        n => format!("för {n} dagar sedan"),
    }
}

/// Datumet med veckodag, t.ex. "onsdag 14 oktober 2026".
pub fn dag_lång(dag: Date) -> String {
    let veckodag = match dag.weekday() {
        Weekday::Monday => "måndag",
        Weekday::Tuesday => "tisdag",
        Weekday::Wednesday => "onsdag",
        Weekday::Thursday => "torsdag",
        Weekday::Friday => "fredag",
        Weekday::Saturday => "lördag",
        Weekday::Sunday => "söndag",
    };
    format!(
        "{veckodag} {} {}",
        dag.day(),
        crate::rakenskapsar::periodnamn(dag)
    )
}
