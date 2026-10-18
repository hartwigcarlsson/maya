//! Skrivaren: en 4E-fil i CP437 med CRLF och `#KSUMMA`.

use jiff::civil::Date;

use crate::cp437;
use crate::{Saldopost, Sie4};

/// Ett fält i en post.
pub(crate) enum Fält {
    /// Ett tal, datum eller kod. Skrivs utan citattecken om det inte innehåller blanksteg.
    Ord(String),
    /// Fritext, alltid inom citattecken.
    Text(String),
    /// En objektlista, `{}`.
    Lista,
}

impl Fält {
    /// Fältets innehåll som det räknas i `#KSUMMA`: utan citattecken och klamrar.
    pub(crate) fn innehåll(&self) -> &str {
        match self {
            Fält::Ord(s) | Fält::Text(s) => s,
            Fält::Lista => "",
        }
    }
}

/// Skriver filen. Fritext citeras alltid (`"` och `\` skrivs `\"` och `\\`), och tecken som
/// saknas i CP437 ersätts (se [`crate::cp437`]).
pub fn skriv(fil: &Sie4) -> Vec<u8> {
    let mut ut = Skrivare::default();
    ut.post("#FLAGGA", vec![ord("0")]);
    ut.börja_ksumma();
    ut.post("#PROGRAM", vec![text(&fil.program), text(&fil.version)]);
    ut.post("#FORMAT", vec![ord("PC8")]);
    let mut generering = vec![datum(fil.genererad)];
    generering.extend(fil.genererad_av.as_deref().map(text));
    ut.post("#GEN", generering);
    ut.post("#SIETYP", vec![ord("4")]);
    if let Some(orgnr) = &fil.orgnr {
        ut.post("#ORGNR", vec![ord(orgnr)]);
    }
    if let Some(ftyp) = &fil.företagsform {
        ut.post("#FTYP", vec![ord(ftyp)]);
    }
    ut.post("#FNAMN", vec![text(&fil.företagsnamn)]);
    if let Some(kptyp) = &fil.kontoplanstyp {
        ut.post("#KPTYP", vec![ord(kptyp)]);
    }
    for år in &fil.räkenskapsår {
        ut.post(
            "#RAR",
            vec![ord(&år.årsnr.to_string()), datum(år.start), datum(år.slut)],
        );
    }
    for konto in &fil.konton {
        let nr = ord(&konto.nummer.to_string());
        ut.post("#KONTO", vec![nr, text(&konto.namn)]);
        if let Some(sru) = konto.sru {
            ut.post(
                "#SRU",
                vec![ord(&konto.nummer.to_string()), ord(&sru.to_string())],
            );
        }
    }
    for (etikett, poster) in [
        ("#IB", &fil.ingående),
        ("#UB", &fil.utgående),
        ("#RES", &fil.resultat),
    ] {
        for p in poster {
            ut.post(etikett, saldopost(p));
        }
    }
    for ver in &fil.verifikat {
        let mut fält = vec![
            ord(&ver.serie),
            ord(&ver.nummer.to_string()),
            datum(ver.datum),
            text(&ver.text),
        ];
        match (ver.regdatum, &ver.sign) {
            (Some(reg), Some(sign)) => fält.extend([datum(reg), text(sign)]),
            (Some(reg), None) => fält.push(datum(reg)),
            (None, Some(sign)) => fält.extend([text(""), text(sign)]),
            (None, None) => {}
        }
        ut.post("#VER", fält);
        ut.rad(b"{");
        for tr in &ver.transaktioner {
            let mut fält = vec![ord(&tr.konto.to_string()), Fält::Lista, belopp(tr.öre)];
            if let Some(t) = &tr.text {
                fält.extend([text(""), text(t)]);
            }
            ut.post("#TRANS", fält);
        }
        ut.rad(b"}");
    }
    let summa = ut.ksumma.take().map(|k| k.finalize()).unwrap_or_default();
    ut.post("#KSUMMA", vec![ord(&summa.to_string())]);
    ut.bytes
}

#[derive(Default)]
struct Skrivare {
    bytes: Vec<u8>,
    ksumma: Option<crc32fast::Hasher>,
}

impl Skrivare {
    fn börja_ksumma(&mut self) {
        self.rad(b"#KSUMMA");
        self.ksumma = Some(crc32fast::Hasher::new());
    }

    fn post(&mut self, etikett: &str, fält: Vec<Fält>) {
        if let Some(k) = &mut self.ksumma {
            k.update(etikett.as_bytes());
            for f in &fält {
                k.update(&cp437::koda(f.innehåll()));
            }
        }
        let mut rad = etikett.as_bytes().to_vec();
        for f in &fält {
            rad.push(b' ');
            match f {
                Fält::Ord(s) if !s.is_empty() && !s.contains([' ', '\t', '"', '{', '}']) => {
                    rad.extend(cp437::koda(s));
                }
                Fält::Ord(s) | Fält::Text(s) => {
                    rad.push(b'"');
                    for b in cp437::koda(s) {
                        if b == b'"' || b == b'\\' {
                            rad.push(b'\\');
                        }
                        rad.push(b);
                    }
                    rad.push(b'"');
                }
                Fält::Lista => rad.extend(b"{}"),
            }
        }
        self.rad(&rad);
    }

    fn rad(&mut self, rad: &[u8]) {
        self.bytes.extend(rad);
        self.bytes.extend(b"\r\n");
    }
}

fn ord(s: &str) -> Fält {
    Fält::Ord(s.into())
}

fn text(s: &str) -> Fält {
    Fält::Text(s.into())
}

fn datum(d: Date) -> Fält {
    Fält::Ord(d.strftime("%Y%m%d").to_string())
}

/// Ett belopp i öre med punkt och två decimaler, kredit negativt.
fn belopp(öre: i64) -> Fält {
    let tecken = if öre < 0 { "-" } else { "" };
    let a = öre.unsigned_abs();
    Fält::Ord(format!("{tecken}{}.{:02}", a / 100, a % 100))
}

fn saldopost(p: &Saldopost) -> Vec<Fält> {
    vec![
        ord(&p.årsnr.to_string()),
        ord(&p.konto.to_string()),
        belopp(p.öre),
    ]
}
