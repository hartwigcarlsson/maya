//! BAS fria kopplingstabell för INK2 till kontointervall → INK2R-fält.
//!
//! Kolumnerna är Fält-kod (A), Rad Ink 2 (B), Benämning (C) och Konton (D). En rad utan fältkod
//! är en rubrik. En rad med fältkod men utan radnummer är minusfältet till raden ovanför.
//!
//! Kontona i D är mönster (`10xx`, `112x`, `1088`, `30xx-37xx`) med `(exkl. …)` och
//! `(Om netto +)`/`(Om netto -)`, eller `+ `/`– ` först. Ett konto får fältet från det mest
//! specifika mönstret som träffar det (`1573` före `157x`). Om två mönster är lika specifika
//! och inte är ett plus- och ett minusfält stoppar bygget, om inte [`MAYAS_VAL`] avgör det.

use std::fmt::Write as _;

use super::{blad, text};

/// Kopplingstabellen som byggs in. Filnamnet är BAS beteckning på utgåvan.
pub const KÄLLA: &str = "data/ink2/INK2_P1-231002_241119.xlsx";

/// Där tabellen ger samma konton två rader väljer Maya en: (första kontot, sista kontot, rad).
/// 40xx-47xx står både på 3.5 Råvaror och förnödenheter och 3.6 Handelsvaror. BAS 2026 avgör
/// det i kontogruppernas namn: 40 och 42 är handelsvaror, 43–47 "(Råvaror och förnödenheter)".
const MAYAS_VAL: &[(u16, u16, &str)] = &[(4000, 4299, "3.6"), (4300, 4799, "3.5")];

/// Konton i klass 1–8 som tabellen inte täcker, och raden Maya ger dem.
/// - 2010–2079 är eget kapital för enskild firma, handelsbolag, ideella föreningar och
///   stiftelser, som inte lämnar INK2; Maya lägger dem på 2.28 Fritt eget kapital.
/// - 48xx är ny i BAS 2026 (tabellen gäller BAS 2023) och heter "Andra produktionskostnader
///   (Råvaror och förnödenheter)": 3.5.
const MAYAS_TILLÄGG: &[(u16, u16, &str)] = &[(2010, 2079, "2.28"), (4800, 4899, "3.5")];

/// Rubriker som inte är en sida (versaler) eller närmast över raderna, med sin nivå.
const RUBRIKNIVÅER: &[(&str, usize)] = &[
    ("Tillgångar", 1),
    ("Eget kapital och skulder", 1),
    ("Anläggningstillgångar", 2),
    ("Omsättningstillgångar", 2),
    ("Eget kapital", 2),
    ("Obeskattade reserver och avsättningar", 2),
    ("Skulder", 2),
];
const NEDERSTA_RUBRIKNIVÅN: usize = 3;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tecken {
    Alla,
    Plus,
    Minus,
}

#[derive(Clone, Copy, Debug)]
struct Mönster {
    från: u16,
    till: u16,
    /// Antalet bestämda siffror: 2 för `10xx`, 3 för `112x`, 4 för `1088`.
    specificitet: u8,
}

impl Mönster {
    fn träffar(&self, nr: u16) -> bool {
        (self.från..=self.till).contains(&nr)
    }
}

struct Post {
    mönster: Mönster,
    undantag: Vec<Mönster>,
    tecken: Tecken,
    fält: usize,
}

struct Rad {
    rad: String,
    benämning: String,
    rubriker: Vec<String>,
}

struct Fält {
    kod: u16,
    rad: usize,
}

pub fn generera(fil: &str) -> String {
    let blad = blad(fil);
    let (start, _) = blad.start().unwrap();
    let (slut, _) = blad.end().unwrap();
    let rubrikrad = (start..=slut)
        .find(|&r| text(&blad, r, 0).as_deref() == Some("Fält-kod"))
        .expect("rubrikraden Fält-kod saknas");

    let mut rader: Vec<Rad> = Vec::new();
    let mut fält: Vec<Fält> = Vec::new();
    let mut poster: Vec<Post> = Vec::new();
    let mut rubriker: Vec<String> = Vec::new();

    for r in rubrikrad + 1..=slut {
        let kod = text(&blad, r, 0);
        let rad = text(&blad, r, 1);
        let benämning = text(&blad, r, 2);
        let konton = text(&blad, r, 3);
        let Some(kod) = kod else {
            if let Some(rubrik) = benämning {
                let (nivå, rubrik) = rubriknivå(&rubrik);
                assert!(
                    nivå <= rubriker.len(),
                    "{fil}: rubriken {rubrik:?} saknar överrubrik"
                );
                rubriker.truncate(nivå);
                rubriker.push(rubrik);
            }
            continue;
        };
        let kod: u16 = kod
            .parse()
            .unwrap_or_else(|_| panic!("{fil}: fältkod {kod:?}"));
        if let Some(rad) = rad {
            rader.push(Rad {
                rad,
                benämning: benämning.unwrap_or_else(|| panic!("{fil}: fält {kod} saknar namn")),
                rubriker: rubriker.clone(),
            });
        }
        assert!(
            !rader.is_empty(),
            "{fil}: fält {kod} hör inte till någon rad"
        );
        fält.push(Fält {
            kod,
            rad: rader.len() - 1,
        });
        let konton = konton.unwrap_or_else(|| panic!("{fil}: fält {kod} saknar konton"));
        for (mönster, undantag, tecken) in kontolista(&konton) {
            poster.push(Post {
                mönster,
                undantag,
                tecken,
                fält: fält.len() - 1,
            });
        }
    }
    for (namn, _) in RUBRIKNIVÅER {
        assert!(
            rader.iter().any(|r| r.rubriker.iter().any(|x| x == namn)),
            "{fil}: rubriken {namn:?} saknas"
        );
    }

    let intervall = intervall(fil, &rader, &fält, &poster);

    let mut ut = String::from("// Genererad av build.rs ur BAS kopplingstabell. Ändra inte.\n\n");
    let utgåva = fil.rsplit('/').next().unwrap().trim_end_matches(".xlsx");
    writeln!(ut, "/// BAS beteckning på kopplingstabellens utgåva.").unwrap();
    writeln!(ut, "pub const UTGÅVA: &str = {utgåva:?};\n").unwrap();
    ut.push_str("static RADER: &[Ink2Rad] = &[\n");
    for r in &rader {
        writeln!(
            ut,
            "    Ink2Rad {{ rad: {:?}, benämning: {:?}, rubriker: &{:?} }},",
            r.rad, r.benämning, r.rubriker
        )
        .unwrap();
    }
    ut.push_str("];\n\nstatic FÄLT: &[Fält] = &[\n");
    for f in &fält {
        writeln!(ut, "    Fält {{ kod: {}, rad: {} }},", f.kod, f.rad).unwrap();
    }
    ut.push_str("];\n\n/// (första kontot, sista kontot, plusfält, minusfält), sorterade.\n");
    ut.push_str("static INTERVALL: &[(u16, u16, u8, u8)] = &[\n");
    for (från, till, plus, minus) in intervall {
        writeln!(ut, "    ({från}, {till}, {plus}, {minus}),").unwrap();
    }
    ut.push_str("];\n");
    ut
}

/// Rubrikens nivå och namn. Versaler (BALANSRÄKNING) är nivå 0 och skrivs om till gemener.
fn rubriknivå(rubrik: &str) -> (usize, String) {
    if !rubrik.chars().any(char::is_lowercase) {
        let mut tecken = rubrik.chars();
        let första = tecken.next().unwrap();
        return (0, första.to_string() + &tecken.as_str().to_lowercase());
    }
    let nivå = RUBRIKNIVÅER
        .iter()
        .find(|(namn, _)| *namn == rubrik)
        .map_or(NEDERSTA_RUBRIKNIVÅN, |(_, nivå)| *nivå);
    (nivå, rubrik.to_string())
}

/// Varje konto 1000–8999 med sitt plus- och minusfält, ihopslaget till intervall.
fn intervall(
    fil: &str,
    rader: &[Rad],
    fält: &[Fält],
    poster: &[Post],
) -> Vec<(u16, u16, usize, usize)> {
    let fält_på = |rad: &str| {
        fält
            .iter()
            .position(|f| rader[f.rad].rad == rad)
            .unwrap_or_else(|| panic!("{fil}: raden {rad} saknas"))
    };
    let mut använda_val = vec![false; MAYAS_VAL.len()];
    let mut använda_tillägg = vec![false; MAYAS_TILLÄGG.len()];
    let mut ut: Vec<(u16, u16, usize, usize)> = Vec::new();
    let mut lägg_till = |nr: u16, p: usize, m: usize| match ut.last_mut() {
        Some((_, till, lp, lm)) if *till + 1 == nr && (*lp, *lm) == (p, m) => *till = nr,
        _ => ut.push((nr, nr, p, m)),
    };
    for nr in 1000..=8999 {
        let träffar: Vec<&Post> = poster
            .iter()
            .filter(|p| p.mönster.träffar(nr) && !p.undantag.iter().any(|u| u.träffar(nr)))
            .collect();
        let Some(högst) = träffar.iter().map(|p| p.mönster.specificitet).max() else {
            if let Some(i) = MAYAS_TILLÄGG
                .iter()
                .position(|(från, till, _)| (*från..=*till).contains(&nr))
            {
                använda_tillägg[i] = true;
                let f = fält_på(MAYAS_TILLÄGG[i].2);
                lägg_till(nr, f, f);
            }
            continue;
        };
        let mut bästa: Vec<&Post> = träffar
            .into_iter()
            .filter(|p| p.mönster.specificitet == högst)
            .collect();
        bästa.sort_by_key(|p| p.fält);
        bästa.dedup_by_key(|p| (p.fält, p.tecken));

        let alla: Vec<_> = bästa.iter().filter(|p| p.tecken == Tecken::Alla).collect();
        let plus: Vec<_> = bästa.iter().filter(|p| p.tecken == Tecken::Plus).collect();
        let minus: Vec<_> = bästa.iter().filter(|p| p.tecken == Tecken::Minus).collect();
        let (p, m) = match (&alla[..], &plus[..], &minus[..]) {
            ([a], [], []) => (a.fält, a.fält),
            ([], [p], [m]) => (p.fält, m.fält),
            ([], [p], []) => (p.fält, p.fält),
            ([], [], [m]) => (m.fält, m.fält),
            _ => {
                let val = MAYAS_VAL
                    .iter()
                    .position(|(från, till, _)| (*från..=*till).contains(&nr));
                let valt = val.and_then(|i| {
                    alla.iter()
                        .find(|a| rader[fält[a.fält].rad].rad == MAYAS_VAL[i].2)
                        .filter(|_| plus.is_empty() && minus.is_empty())
                });
                let Some(valt) = valt else {
                    let fältkoder: Vec<_> = bästa.iter().map(|p| fält[p.fält].kod).collect();
                    panic!("{fil}: konto {nr} har flera INK2-fält {fältkoder:?}");
                };
                använda_val[val.unwrap()] = true;
                (valt.fält, valt.fält)
            }
        };
        lägg_till(nr, p, m);
    }
    assert!(
        använda_val.iter().chain(&använda_tillägg).all(|&a| a),
        "{fil}: ett av MAYAS_VAL eller MAYAS_TILLÄGG behövs inte längre"
    );
    ut
}

/// Tolkar kolumnen Konton till (mönster, undantag, tecken).
fn kontolista(text: &str) -> Vec<(Mönster, Vec<Mönster>, Tecken)> {
    let text = text.replace('–', "-");
    let (radtecken, text) = if let Some(rest) = text.strip_prefix("+ ") {
        (Some(Tecken::Plus), rest)
    } else if let Some(rest) = text.strip_prefix("- ") {
        (Some(Tecken::Minus), rest)
    } else {
        (None, text.as_str())
    };

    let mut delar: Vec<(Mönster, Vec<Mönster>, Option<Tecken>)> = Vec::new();
    for del in dela(text, ',') {
        let (mönster, mut rest) = del.split_once('(').map_or((del, ""), |(m, r)| (m, r));
        let mut undantag = Vec::new();
        let mut tecken = None;
        while !rest.is_empty() {
            let (inom, efter) = rest
                .split_once(')')
                .unwrap_or_else(|| panic!("{KÄLLA}: parentes saknar slut i {text:?}"));
            let inom = inom.trim();
            if let Some(lista) = inom.strip_prefix("exkl.") {
                for u in dela(lista, ',').iter().flat_map(|u| u.split(" och ")) {
                    undantag.push(mönster_ur(u));
                }
            } else if inom == "Om netto +" {
                tecken = Some(Tecken::Plus);
            } else if inom == "Om netto -" {
                tecken = Some(Tecken::Minus);
            } else {
                panic!("{KÄLLA}: okänd parentes ({inom}) i {text:?}");
            }
            rest = efter.trim().strip_prefix('(').unwrap_or(efter.trim());
        }
        delar.push((mönster_ur(mönster), undantag, tecken));
    }

    // "8113, 8118 (Om netto +)": ett tecken bara på sista delen gäller hela raden.
    let sista_tecken = delar.last().and_then(|d| d.2);
    let bara_sista = delar[..delar.len() - 1].iter().all(|d| d.2.is_none());
    delar
        .into_iter()
        .map(|(m, u, t)| {
            let t = radtecken
                .or(t)
                .or(if bara_sista { sista_tecken } else { None })
                .unwrap_or(Tecken::Alla);
            (m, u, t)
        })
        .collect()
}

/// Delar på `skiljetecken` utanför parenteser.
fn dela(text: &str, skiljetecken: char) -> Vec<&str> {
    let mut delar = Vec::new();
    let (mut djup, mut början) = (0, 0);
    for (i, c) in text.char_indices() {
        match c {
            '(' => djup += 1,
            ')' => djup -= 1,
            c if c == skiljetecken && djup == 0 => {
                delar.push(text[början..i].trim());
                början = i + c.len_utf8();
            }
            _ => {}
        }
    }
    delar.push(text[början..].trim());
    delar
}

/// `10xx`, `112x`, `1088` eller ett intervall av två sådana.
fn mönster_ur(text: &str) -> Mönster {
    let text = text.trim();
    if let Some((a, b)) = text.split_once('-') {
        let (a, b) = (mönster_ur(a), mönster_ur(b));
        assert_eq!(
            a.specificitet, b.specificitet,
            "{KÄLLA}: intervallet {text:?}"
        );
        return Mönster {
            från: a.från,
            till: b.till,
            specificitet: a.specificitet,
        };
    }
    let siffror = text.trim_end_matches('x');
    let x = text.len() - siffror.len();
    assert!(
        text.len() == 4 && siffror.bytes().all(|b| b.is_ascii_digit()) && !siffror.is_empty(),
        "{KÄLLA}: okänt kontomönster {text:?}"
    );
    let bas: u16 = siffror.parse().unwrap();
    let steg = 10u16.pow(x as u32);
    Mönster {
        från: bas * steg,
        till: bas * steg + steg - 1,
        specificitet: (4 - x) as u8,
    }
}
