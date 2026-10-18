//! Tolkar BAS fria utgåvor vid bygget (ADR 0006) till Rust-data i `OUT_DIR`. Ingen XLSX läses
//! vid körning, och domain behöver ingen IO.
//!
//! XLSX-filerna har en utskriftslayout som skiljer sig mellan utgåvorna, så varje fil har sin
//! egen tolkare. Allt som inte känns igen stoppar bygget, så att en ny layout inte tolkas fel i
//! tysthet.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use calamine::{Data, Range, Reader, Xlsx, open_workbook};

#[path = "build/ink2.rs"]
mod ink2;

/// Kontoplanen som byggs in.
const KONTOPLAN: &str = "data/bas/BAS_kontoplan_2026_v2.xlsx";
/// Den senaste fria utgåvan som markerar grundkontona med ■.
const GRUNDKONTON: &str = "data/bas/Kontoplan-BAS-2025.xlsx";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=data");

    let (konton, grupper) = kontoplan_2026(&blad(KONTOPLAN));
    let grundkonton = grundkonton_2025(&blad(GRUNDKONTON));

    let mut ut = String::from("// Genererad av build.rs ur BAS fria utgåvor. Ändra inte.\n\n");
    ut.push_str("static KONTON: &[BasKonto] = &[\n");
    for (nummer, namn) in &konton {
        let grundkonto = grundkonton.contains(nummer);
        writeln!(
            ut,
            "    BasKonto {{ nummer: Kontonummer({nummer}), namn: {namn:?}, grundkonto: {grundkonto} }},"
        )
        .unwrap();
    }
    ut.push_str("];\n\nstatic KONTOGRUPPER: &[(u8, &str)] = &[\n");
    for (grupp, namn) in &grupper {
        writeln!(ut, "    ({grupp}, {namn:?}),").unwrap();
    }
    ut.push_str("];\n");

    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    std::fs::write(out.join("bas.rs"), ut).unwrap();
    std::fs::write(out.join("ink2.rs"), ink2::generera(ink2::KÄLLA)).unwrap();
}

fn blad(fil: &str) -> Range<Data> {
    let mut bok: Xlsx<_> =
        open_workbook(Path::new(fil)).unwrap_or_else(|e| panic!("kan inte öppna {fil}: {e}"));
    bok.worksheet_range_at(0)
        .unwrap_or_else(|| panic!("{fil} har inget blad"))
        .unwrap_or_else(|e| panic!("kan inte läsa {fil}: {e}"))
}

/// Cellens text, med radbrytningar och dubbla mellanslag ur utskriftslayouten borttagna.
fn text(blad: &Range<Data>, rad: u32, kolumn: u32) -> Option<String> {
    let text = match blad.get_value((rad, kolumn))? {
        Data::Empty => return None,
        Data::String(s) => s.split_whitespace().collect::<Vec<_>>().join(" "),
        Data::Int(i) => i.to_string(),
        Data::Float(f) if f.fract() == 0.0 => format!("{f:.0}"),
        annat => panic!(
            "oväntad cell på rad {} kolumn {}: {annat:?}",
            rad + 1,
            kolumn + 1
        ),
    };
    (!text.is_empty()).then_some(text)
}

/// Raden efter rubrikraden med "Huvudkonton", där kontona börjar.
fn första_kontoraden(blad: &Range<Data>) -> u32 {
    let (start, _) = blad.start().unwrap();
    let (slut, bredd) = blad.end().unwrap();
    (start..=slut)
        .find(|&rad| (0..=bredd).any(|k| text(blad, rad, k).as_deref() == Some("Huvudkonton")))
        .expect("rubrikraden Huvudkonton saknas")
        + 1
}

/// Ett fyrsiffrigt kontonummer, eventuellt följt av `#` (ej K2).
fn kontonummer(text: &str) -> Option<u16> {
    let siffror = text.strip_suffix('#').unwrap_or(text);
    (siffror.len() == 4 && siffror.bytes().all(|b| b.is_ascii_digit()))
        .then(|| siffror.parse().unwrap())
}

/// BAS 2026: kolumnerna A–B är huvudkonton och C–D underkonton. Ett tvåsiffrigt tal i A är en
/// kontogrupp med namnet i versaler i B. Ett nummer som står två gånger (huvud- och underkonto,
/// eller 2087 med två namn) blir ett konto med det första namnet.
fn kontoplan_2026(blad: &Range<Data>) -> (BTreeMap<u16, String>, BTreeMap<u8, String>) {
    let mut konton = BTreeMap::new();
    let mut grupper = BTreeMap::new();
    let (slut, _) = blad.end().unwrap();
    for rad in första_kontoraden(blad)..=slut {
        for (nr, namn) in [(0, 1), (2, 3)] {
            let Some(cell) = text(blad, rad, nr) else {
                continue;
            };
            if nr == 0 && cell.len() == 2 && cell.bytes().all(|b| b.is_ascii_digit()) {
                let namn = text(blad, rad, 1)
                    .unwrap_or_else(|| panic!("{KONTOPLAN}: kontogrupp {cell} saknar namn"));
                grupper.insert(cell.parse().unwrap(), gemener(&namn));
                continue;
            }
            let nummer = kontonummer(&cell).unwrap_or_else(|| {
                panic!(
                    "{KONTOPLAN}: oväntat kontonummer {cell:?} på rad {}",
                    rad + 1
                )
            });
            let namn = text(blad, rad, namn)
                .unwrap_or_else(|| panic!("{KONTOPLAN}: konto {nummer} saknar namn"));
            konton.entry(nummer).or_insert(namn);
        }
    }
    assert!(!grupper.is_empty(), "{KONTOPLAN}: inga kontogrupper");
    (konton, grupper)
}

/// "KASSA OCH BANK" som "Kassa och bank".
fn gemener(versaler: &str) -> String {
    let gemener = versaler.to_lowercase();
    let mut tecken = gemener.chars();
    tecken
        .next()
        .map(|första| första.to_uppercase().chain(tecken).collect())
        .unwrap_or_default()
}

/// BAS 2025: kolumn F bär markeringen och G–H kontot. Kolumnerna B–D är tresiffriga
/// huvudkontogrupper och används inte.
fn grundkonton_2025(blad: &Range<Data>) -> BTreeSet<u16> {
    let mut grundkonton = BTreeSet::new();
    let (slut, _) = blad.end().unwrap();
    for rad in första_kontoraden(blad)..=slut {
        let Some(cell) = text(blad, rad, 6) else {
            continue;
        };
        let nummer = kontonummer(&cell).unwrap_or_else(|| {
            panic!(
                "{GRUNDKONTON}: oväntat kontonummer {cell:?} på rad {}",
                rad + 1
            )
        });
        if text(blad, rad, 5).is_some_and(|m| m.contains('■')) {
            grundkonton.insert(nummer);
        }
    }
    assert!(
        !grundkonton.is_empty(),
        "{GRUNDKONTON}: inga grundkonton (■)"
    );
    grundkonton
}
