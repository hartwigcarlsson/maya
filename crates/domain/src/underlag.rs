//! Underlag: originalfilen som styrker en affärshändelse, arkiverad som den togs emot (VF5,
//! ADR 0002). Här finns reglerna som både servern och formuläret använder: storleksgränsen, vilken
//! MIME-typ en fil får och om Maya visar den eller laddar ned den.

/// Ett underlag är högst 25 MB (ADR 0002).
pub const STÖRSTA_STORLEK: usize = 25 * 1024 * 1024;

/// Hur Maya presenterar ett underlag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visning {
    Pdf,
    Bild,
    /// Andra format laddas ned.
    Nedladdning,
}

type KännsIgen = fn(&[u8]) -> bool;

/// Formaten som Maya känner igen på filens första byte, och som visas i Maya.
const KÄNDA: [(&str, KännsIgen); 5] = [
    ("application/pdf", |b| b.starts_with(b"%PDF-")),
    ("image/png", |b| b.starts_with(b"\x89PNG\r\n\x1a\n")),
    ("image/jpeg", |b| b.starts_with(b"\xff\xd8\xff")),
    ("image/gif", |b| {
        b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a")
    }),
    ("image/webp", |b| {
        b.len() >= 12 && b.starts_with(b"RIFF") && &b[8..12] == b"WEBP"
    }),
];

/// MIME-typen som lagras för en fil: den som filens början visar för PDF och bilder, annars
/// webbläsarens typ om den ser ut som en MIME-typ. En fil som påstås vara PDF eller bild men inte
/// är det, eller som saknar typ, blir `application/octet-stream`, så att den aldrig visas.
pub fn mime(angiven: &str, början: &[u8]) -> String {
    if let Some((typ, _)) = KÄNDA.iter().find(|(_, känns_igen)| känns_igen(början)) {
        return (*typ).to_string();
    }
    let angiven = angiven
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let giltig = angiven.split_once('/').is_some_and(|(typ, undertyp)| {
        let tecken = |s: &str| {
            !s.is_empty()
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "!#$&-^_.+".contains(c))
        };
        tecken(typ) && tecken(undertyp)
    });
    if giltig && visning(&angiven) == Visning::Nedladdning {
        angiven
    } else {
        "application/octet-stream".to_string()
    }
}

/// Om ett underlag med MIME-typen visas i Maya. Bara PDF och rasterbilder visas, aldrig format
/// som kan köra skript (HTML, SVG).
pub fn visning(mime: &str) -> Visning {
    match mime {
        "application/pdf" => Visning::Pdf,
        "image/png" | "image/jpeg" | "image/gif" | "image/webp" => Visning::Bild,
        _ => Visning::Nedladdning,
    }
}

/// Storleken som den visas, t.ex. "512 byte", "2 kB" eller "1,5 MB".
pub fn storlek(byte: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * 1024;
    if byte < KB {
        format!("{byte} byte")
    } else if byte < MB {
        format!("{} kB", byte.div_ceil(KB))
    } else {
        let tiondelar = (byte * 10 + MB / 2) / MB;
        format!("{},{} MB", tiondelar / 10, tiondelar % 10)
    }
}
