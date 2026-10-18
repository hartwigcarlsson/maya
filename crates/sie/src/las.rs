//! Läsaren: en SIE4-fil i CP437 till [`Sie4`]. Den tål det standarden kräver att en läsare tål:
//! tomma rader, LF eller CRLF, citattecken runt alla fält, okända poster och extra fält i slutet
//! av en post. `#RTRANS` och `#BTRANS` hoppas över, så att bara `#TRANS` (slutläget) läses.
//! En påbörjad `#KSUMMA` kontrolleras, och en summa som inte stämmer eller inte avslutas är ett
//! fel.

use jiff::civil::Date;

use crate::cp437;
use crate::{Konto, Räkenskapsår, Saldopost, Sie4, Transaktion, Verifikation};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum LäsFel {
    #[error("rad {rad}: {orsak}")]
    Rad { rad: usize, orsak: String },
    #[error("filen saknar {0}")]
    Saknar(&'static str),
    #[error("#KSUMMA stämmer inte: filen har {i_filen}, innehållet ger {beräknad}")]
    FelKsumma { i_filen: u32, beräknad: u32 },
    #[error("#KSUMMA påbörjas men avslutas inte")]
    OavslutadKsumma,
}

/// Ett fält som det står i filen, utan citattecken.
enum Token {
    Ord(String),
    /// En objektlista `{…}` med sina fält.
    Lista(Vec<String>),
}

impl Token {
    fn text(&self) -> Option<&str> {
        match self {
            Token::Ord(s) => Some(s),
            Token::Lista(_) => None,
        }
    }
}

/// Läser en fil i CP437.
pub fn läs(bytes: &[u8]) -> Result<Sie4, LäsFel> {
    let mut l = Läsare::default();
    let text = cp437::avkoda(bytes);
    for (i, rad) in text.split('\n').enumerate() {
        l.rad = i + 1;
        l.läs_rad(rad.strip_suffix('\r').unwrap_or(rad))?;
    }
    if l.ksumma.is_some() {
        return Err(LäsFel::OavslutadKsumma);
    }
    if l.i_ver.is_some() {
        return Err(l.fel("verifikationen avslutas inte med }"));
    }
    Ok(Sie4 {
        program: l.program.ok_or(LäsFel::Saknar("#PROGRAM"))?,
        version: l.program_version,
        genererad: l.genererad.ok_or(LäsFel::Saknar("#GEN"))?,
        genererad_av: l.genererad_av,
        orgnr: l.orgnr,
        företagsform: l.ftyp,
        företagsnamn: l.fnamn.ok_or(LäsFel::Saknar("#FNAMN"))?,
        kontoplanstyp: l.kptyp,
        räkenskapsår: l.rar,
        konton: l.konton,
        ingående: l.ib,
        utgående: l.ub,
        resultat: l.res,
        verifikat: l.verifikat,
    })
}

#[derive(Default)]
struct Läsare {
    rad: usize,
    ksumma: Option<crc32fast::Hasher>,
    program: Option<String>,
    program_version: String,
    genererad: Option<Date>,
    genererad_av: Option<String>,
    orgnr: Option<String>,
    ftyp: Option<String>,
    fnamn: Option<String>,
    kptyp: Option<String>,
    rar: Vec<Räkenskapsår>,
    konton: Vec<Konto>,
    ib: Vec<Saldopost>,
    ub: Vec<Saldopost>,
    res: Vec<Saldopost>,
    verifikat: Vec<Verifikation>,
    /// `Some(öppnad)` mellan `#VER` och `}`; `öppnad` när `{` är läst.
    i_ver: Option<bool>,
}

impl Läsare {
    fn fel(&self, orsak: impl Into<String>) -> LäsFel {
        LäsFel::Rad {
            rad: self.rad,
            orsak: orsak.into(),
        }
    }

    fn läs_rad(&mut self, rad: &str) -> Result<(), LäsFel> {
        let rad = rad.trim_matches([' ', '\t']);
        match rad {
            "" => return Ok(()),
            "{" => {
                return match self.i_ver {
                    Some(false) => {
                        self.i_ver = Some(true);
                        Ok(())
                    }
                    _ => Err(self.fel("{ utan #VER")),
                };
            }
            "}" => {
                return match self.i_ver {
                    Some(true) => {
                        self.i_ver = None;
                        Ok(())
                    }
                    _ => Err(self.fel("} utan {")),
                };
            }
            _ => {}
        }
        let tokens = self.tokens(rad)?;
        let Some(Token::Ord(etikett)) = tokens.first() else {
            return Err(self.fel("posten saknar etikett"));
        };
        let skriven = etikett.clone();
        let etikett = etikett.to_ascii_uppercase();
        if !etikett.starts_with('#') {
            return Err(self.fel("posten börjar inte med #"));
        }
        let fält = &tokens[1..];
        if self.i_ver == Some(false) {
            return Err(self.fel("#VER följs inte av {"));
        }
        if etikett == "#KSUMMA" {
            return self.ksumma(fält);
        }
        if let Some(k) = &mut self.ksumma {
            k.update(&cp437::koda(&skriven));
            for t in fält {
                match t {
                    Token::Ord(s) => k.update(&cp437::koda(s)),
                    Token::Lista(l) => l.iter().for_each(|s| k.update(&cp437::koda(s))),
                }
            }
        }
        let i_ver = self.i_ver == Some(true);
        match etikett.as_str() {
            "#TRANS" if i_ver => self.trans(fält),
            "#RTRANS" | "#BTRANS" if i_ver => Ok(()),
            "#TRANS" | "#RTRANS" | "#BTRANS" => Err(self.fel(format!("{etikett} utanför #VER"))),
            _ if i_ver => Err(self.fel(format!("{etikett} inuti #VER"))),
            "#PROGRAM" => {
                self.program = Some(self.ord(fält, 0, "programnamn")?);
                self.program_version = self.valfri(fält, 1).unwrap_or_default();
                Ok(())
            }
            "#GEN" => {
                self.genererad = Some(self.datum(fält, 0)?);
                self.genererad_av = self.valfri(fält, 1);
                Ok(())
            }
            "#ORGNR" => {
                self.orgnr = Some(self.ord(fält, 0, "organisationsnummer")?);
                Ok(())
            }
            "#FTYP" => {
                self.ftyp = Some(self.ord(fält, 0, "företagsform")?);
                Ok(())
            }
            "#FNAMN" => {
                self.fnamn = Some(self.ord(fält, 0, "företagsnamn")?);
                Ok(())
            }
            "#KPTYP" => {
                self.kptyp = Some(self.ord(fält, 0, "kontoplanstyp")?);
                Ok(())
            }
            "#RAR" => {
                let år = Räkenskapsår {
                    årsnr: self.tal(fält, 0, "årsnummer")?,
                    start: self.datum(fält, 1)?,
                    slut: self.datum(fält, 2)?,
                };
                self.rar.push(år);
                Ok(())
            }
            "#KONTO" => {
                let konto = Konto {
                    nummer: self.tal(fält, 0, "kontonummer")?,
                    namn: self.ord(fält, 1, "kontonamn")?,
                    sru: None,
                };
                self.konton.push(konto);
                Ok(())
            }
            "#SRU" => {
                let nummer: u32 = self.tal(fält, 0, "kontonummer")?;
                let sru = self.tal(fält, 1, "SRU-kod")?;
                let Some(konto) = self.konton.iter_mut().find(|k| k.nummer == nummer) else {
                    return Err(self.fel(format!("#SRU för konto {nummer} före dess #KONTO")));
                };
                konto.sru = Some(sru);
                Ok(())
            }
            "#IB" | "#UB" | "#RES" => {
                let post = Saldopost {
                    årsnr: self.tal(fält, 0, "årsnummer")?,
                    konto: self.tal(fält, 1, "kontonummer")?,
                    öre: self.belopp(fält, 2)?,
                };
                match etikett.as_str() {
                    "#IB" => self.ib.push(post),
                    "#UB" => self.ub.push(post),
                    _ => self.res.push(post),
                }
                Ok(())
            }
            "#VER" => self.ver(fält),
            _ => Ok(()),
        }
    }

    fn ksumma(&mut self, fält: &[Token]) -> Result<(), LäsFel> {
        match (self.ksumma.take(), fält.first()) {
            (None, None) => {
                self.ksumma = Some(crc32fast::Hasher::new());
                Ok(())
            }
            (Some(k), Some(_)) => {
                let i_filen: i64 = self.tal(fält, 0, "kontrollsumma")?;
                let beräknad = k.finalize();
                // Summan kan stå med eller utan tecken.
                if i_filen.rem_euclid(1 << 32) == i64::from(beräknad) {
                    Ok(())
                } else {
                    Err(LäsFel::FelKsumma {
                        i_filen: i_filen.rem_euclid(1 << 32) as u32,
                        beräknad,
                    })
                }
            }
            (None, Some(_)) => Err(self.fel("#KSUMMA avslutas utan att ha påbörjats")),
            (Some(_), None) => Err(self.fel("#KSUMMA påbörjas två gånger")),
        }
    }

    fn ver(&mut self, fält: &[Token]) -> Result<(), LäsFel> {
        let ver = Verifikation {
            serie: self.valfri(fält, 0).unwrap_or_default(),
            nummer: match self.valfri(fält, 1) {
                Some(_) => self.tal(fält, 1, "verifikationsnummer")?,
                None => 0,
            },
            datum: self.datum(fält, 2)?,
            text: self.valfri(fält, 3).unwrap_or_default(),
            regdatum: match self.valfri(fält, 4) {
                Some(_) => Some(self.datum(fält, 4)?),
                None => None,
            },
            sign: self.valfri(fält, 5),
            transaktioner: Vec::new(),
        };
        self.verifikat.push(ver);
        self.i_ver = Some(false);
        Ok(())
    }

    fn trans(&mut self, fält: &[Token]) -> Result<(), LäsFel> {
        if let Some(Token::Ord(_)) = fält.get(1) {
            return Err(self.fel("#TRANS saknar objektlista"));
        }
        let tr = Transaktion {
            konto: self.tal(fält, 0, "kontonummer")?,
            öre: self.belopp(fält, 2)?,
            text: self.valfri(fält, 4),
        };
        self.verifikat
            .last_mut()
            .expect("#TRANS läses bara inuti #VER")
            .transaktioner
            .push(tr);
        Ok(())
    }

    /// Fältet `i` som text. Saknas det är det ett fel.
    fn ord(&self, fält: &[Token], i: usize, vad: &str) -> Result<String, LäsFel> {
        match fält.get(i) {
            Some(Token::Ord(s)) => Ok(s.clone()),
            Some(Token::Lista(_)) => Err(self.fel(format!("{vad} är en objektlista"))),
            None => Err(self.fel(format!("{vad} saknas"))),
        }
    }

    /// Fältet `i` om det finns och inte är tomt.
    fn valfri(&self, fält: &[Token], i: usize) -> Option<String> {
        fält
            .get(i)
            .and_then(Token::text)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    }

    fn tal<T: std::str::FromStr>(
        &self, fält: &[Token], i: usize, vad: &str
    ) -> Result<T, LäsFel> {
        let s = self.ord(fält, i, vad)?;
        s.parse()
            .map_err(|_| self.fel(format!("{vad} är inte ett tal: {s}")))
    }

    fn datum(&self, fält: &[Token], i: usize) -> Result<Date, LäsFel> {
        let s = self.ord(fält, i, "datum")?;
        let fel = || self.fel(format!("ogiltigt datum: {s}"));
        if s.len() != 8 || !s.bytes().all(|b| b.is_ascii_digit()) {
            return Err(fel());
        }
        let (å, m, d) = (&s[..4], &s[4..6], &s[6..]);
        Date::new(
            å.parse().map_err(|_| fel())?,
            m.parse().map_err(|_| fel())?,
            d.parse().map_err(|_| fel())?,
        )
        .map_err(|_| fel())
    }

    /// Ett belopp med punkt och högst två decimaler, till öre.
    fn belopp(&self, fält: &[Token], i: usize) -> Result<i64, LäsFel> {
        let s = self.ord(fält, i, "belopp")?;
        let fel = || self.fel(format!("ogiltigt belopp: {s}"));
        let (minus, siffror) = match s.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, s.as_str()),
        };
        let (heltal, decimaler) = siffror.split_once('.').unwrap_or((siffror, ""));
        let bara_siffror = |t: &str| t.bytes().all(|b| b.is_ascii_digit());
        if heltal.is_empty() || !bara_siffror(heltal) || !bara_siffror(decimaler) {
            return Err(fel());
        }
        if decimaler.len() > 2 {
            return Err(self.fel(format!("mer än två decimaler: {s}")));
        }
        let kronor: i64 = heltal.parse().map_err(|_| fel())?;
        let öre: i64 = format!("{decimaler:0<2}").parse().map_err(|_| fel())?;
        let belopp = kronor
            .checked_mul(100)
            .and_then(|k| k.checked_add(öre))
            .ok_or_else(fel)?;
        Ok(if minus { -belopp } else { belopp })
    }

    /// Delar en rad i fält: blanksteg och tabbar skiljer, `"…"` citerar (med `\"` och `\\`),
    /// och `{…}` är en objektlista.
    fn tokens(&self, rad: &str) -> Result<Vec<Token>, LäsFel> {
        let mut tokens = Vec::new();
        let mut tecken = rad.chars().peekable();
        let mut lista: Option<Vec<String>> = None;
        loop {
            while tecken.next_if(|c| *c == ' ' || *c == '\t').is_some() {}
            let Some(&c) = tecken.peek() else { break };
            if c == '{' && lista.is_none() {
                tecken.next();
                lista = Some(Vec::new());
                continue;
            }
            if c == '}' {
                tecken.next();
                match lista.take() {
                    Some(l) => tokens.push(Token::Lista(l)),
                    None => return Err(self.fel("} utan {")),
                }
                continue;
            }
            let mut ord = String::new();
            if c == '"' {
                tecken.next();
                let mut stängd = false;
                while let Some(c) = tecken.next() {
                    match c {
                        '\\' => ord.extend(tecken.next()),
                        '"' => {
                            stängd = true;
                            break;
                        }
                        c => ord.push(c),
                    }
                }
                if !stängd {
                    return Err(self.fel("citattecknet stängs inte"));
                }
            } else {
                while let Some(c) = tecken.next_if(|c| !matches!(c, ' ' | '\t' | '{' | '}' | '"')) {
                    ord.push(c);
                }
            }
            match &mut lista {
                Some(l) => l.push(ord),
                None => tokens.push(Token::Ord(ord)),
            }
        }
        if lista.is_some() {
            return Err(self.fel("objektlistan stängs inte"));
        }
        Ok(tokens)
    }
}
