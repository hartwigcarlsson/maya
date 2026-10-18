//! Kontoplanens regler: kontonummer, kontoklass och den inbyggda BAS-kontoplanen.

pub mod bas;
pub mod ink2;

/// Kontona som Mayas flöden bokför på, med flödets namn. De är aktiva från början och kan aldrig
/// inaktiveras ("Används av flödet X"). Varje etapp lägger till sina konton.
const FLÖDENAS_KONTON: &[(Kontonummer, &str)] =
    &[(Kontonummer(2099), "Preliminär ingående balans")];

/// Flödenas konton, med flödets namn per konto.
pub fn flödenas_konton() -> &'static [(Kontonummer, &'static str)] {
    FLÖDENAS_KONTON
}

/// Namnet på flödet som bokför på kontot, om något gör det.
pub fn flöde_som_bokför_på(nummer: Kontonummer) -> Option<&'static str> {
    FLÖDENAS_KONTON
        .iter()
        .find(|(n, _)| *n == nummer)
        .map(|(_, flöde)| *flöde)
}

/// Ett fyrsiffrigt kontonummer i BAS klass 1–8.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Kontonummer(u16);

/// Text som inte är ett kontonummer: fyra siffror i klass 1–8.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OgiltigtKontonummer;

impl std::fmt::Display for OgiltigtKontonummer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("inget kontonummer (fyra siffror, klass 1–8)")
    }
}

impl std::str::FromStr for Kontonummer {
    type Err = OgiltigtKontonummer;

    /// Fyra siffror, med blanksteg runt om, i klass 1–8. Det är den enda tolkningen av ett
    /// kontonummer i text.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let text = text.trim();
        if text.len() != 4 || !text.bytes().all(|b| b.is_ascii_digit()) {
            return Err(OgiltigtKontonummer);
        }
        text.parse::<u16>()
            .ok()
            .and_then(Kontonummer::new)
            .ok_or(OgiltigtKontonummer)
    }
}

impl Kontonummer {
    /// Kontonumret, om det är fyrsiffrigt och hör till klass 1–8. Klass 9 (internredovisning)
    /// för Maya inte.
    pub fn new(nummer: u16) -> Option<Self> {
        (1000..=8999).contains(&nummer).then_some(Self(nummer))
    }

    /// Kontonumret utan kontroll, för konstanter som är kända vid kompileringen.
    pub(crate) const fn konstant(nummer: u16) -> Self {
        assert!(nummer >= 1000 && nummer <= 8999);
        Self(nummer)
    }

    pub fn get(self) -> u16 {
        self.0
    }

    /// Kontoklassen, numrets första siffra (1–8).
    pub fn klass(self) -> u8 {
        (self.0 / 1000) as u8
    }

    /// Kontots typ, som kontoklassen avgör.
    pub fn typ(self) -> Kontotyp {
        match self.klass() {
            1 => Kontotyp::Tillgång,
            2 => Kontotyp::EgetKapitalOchSkuld,
            3 => Kontotyp::Intäkt,
            4..=7 => Kontotyp::Kostnad,
            _ => Kontotyp::FinansiellPost,
        }
    }
}

/// Vad ett konto är för slags konto, enligt BAS kontoklasser.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kontotyp {
    /// Klass 1.
    Tillgång,
    /// Klass 2.
    EgetKapitalOchSkuld,
    /// Klass 3, rörelsens intäkter.
    Intäkt,
    /// Klass 4–7, rörelsens kostnader.
    Kostnad,
    /// Klass 8, finansiella och andra intäkter och kostnader, bokslutsdispositioner, skatt och
    /// årets resultat.
    FinansiellPost,
}

impl Kontotyp {
    /// Om kontot hör till balansräkningen (klass 1–2) och inte resultaträkningen (klass 3–8).
    pub fn balanskonto(self) -> bool {
        matches!(self, Kontotyp::Tillgång | Kontotyp::EgetKapitalOchSkuld)
    }
}

impl std::fmt::Display for Kontonummer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Ett kontos saldo i ett räkenskapsår som inte är stängt, i öre med debet positivt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Årssaldo {
    pub start: jiff::civil::Date,
    pub slut: jiff::civil::Date,
    pub saldo: crate::belopp::Saldo,
}

/// Varför ett konto inte kan inaktiveras.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Inaktiveringsspärr {
    /// Ett av Mayas flöden bokför på kontot.
    Flöde(&'static str),
    /// Kontot har saldo skilt från noll i ett räkenskapsår som inte är stängt.
    Saldo(Årssaldo),
}

impl Inaktiveringsspärr {
    pub fn kod(&self) -> &'static str {
        match self {
            Inaktiveringsspärr::Flöde(_) => "flodets_konto",
            Inaktiveringsspärr::Saldo(_) => "har_saldo",
        }
    }
}

impl std::fmt::Display for Inaktiveringsspärr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Inaktiveringsspärr::Flöde(flöde) => write!(f, "Används av flödet {flöde}"),
            Inaktiveringsspärr::Saldo(s) => write!(
                f,
                "Saldo {} kr i räkenskapsåret {}–{}, som inte är stängt. \
                 Bara konton med nollsaldo kan inaktiveras.",
                s.saldo, s.start, s.slut
            ),
        }
    }
}

/// Om kontot får inaktiveras, givet dess saldo i varje räkenskapsår som inte är stängt.
pub fn kan_inaktiveras(
    nummer: Kontonummer,
    saldon: &[Årssaldo],
) -> Result<(), Inaktiveringsspärr> {
    if let Some(flöde) = flöde_som_bokför_på(nummer) {
        return Err(Inaktiveringsspärr::Flöde(flöde));
    }
    match saldon
        .iter()
        .find(|s| s.saldo != crate::belopp::Saldo::NOLL)
    {
        Some(s) => Err(Inaktiveringsspärr::Saldo(*s)),
        None => Ok(()),
    }
}

/// BAS namn på kontoklassen (1–8).
pub fn klassnamn(klass: u8) -> &'static str {
    match klass {
        1 => "Tillgångar",
        2 => "Eget kapital och skulder",
        3 => "Rörelsens inkomster/intäkter",
        4 => "Utgifter/kostnader för varor, material och vissa köpta tjänster",
        5 | 6 => "Övriga externa rörelseutgifter/kostnader",
        7 => "Utgifter/kostnader för personal, avskrivningar m.m.",
        _ => "Finansiella och andra inkomster/intäkter och utgifter/kostnader",
    }
}

/// Det som följer av ett kontonummer: klass, kontogrupp och INK2-rad. Gäller också egna konton.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OmNumret {
    pub klass: u8,
    pub klassnamn: &'static str,
    /// Numrets två första siffror.
    pub kontogrupp: u8,
    /// Gruppens namn i BAS, om BAS har en rubrik för gruppen.
    pub gruppnamn: Option<&'static str>,
    /// INK2-raderna: en, eller två när saldots tecken avgör (t.ex. 899x på 3.26 eller 3.27).
    /// Tom när kopplingstabellen inte täcker numret.
    pub ink2: Vec<&'static ink2::Ink2Rad>,
}

pub fn om_numret(nummer: Kontonummer) -> OmNumret {
    let kontogrupp = (nummer.get() / 100) as u8;
    let mut rader: Vec<&'static ink2::Ink2Rad> = Vec::new();
    if let Some(koppling) = ink2::koppling(nummer) {
        for plus in [true, false] {
            let rad = koppling.fält(plus).rad();
            if !rader.iter().any(|r| std::ptr::eq(*r, rad)) {
                rader.push(rad);
            }
        }
    }
    OmNumret {
        klass: nummer.klass(),
        klassnamn: klassnamn(nummer.klass()),
        kontogrupp,
        gruppnamn: bas::kontogrupp(kontogrupp),
        ink2: rader,
    }
}

/// Fälten i formuläret för ett eget konto.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KontoFält {
    Nummer,
    Namn,
}

impl KontoFält {
    pub fn namn(self) -> &'static str {
        match self {
            KontoFält::Nummer => "nummer",
            KontoFält::Namn => "namn",
        }
    }
}

/// Ett fel vid ett fält, med en kod som formuläret och servern delar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kontofel {
    pub fält: KontoFält,
    pub kod: &'static str,
    pub text: String,
}

/// Formuläret för ett eget konto, som text. Att numret inte redan finns avgör `core`, som ser
/// företagets kontoplan.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NyttKonto {
    pub nummer: String,
    pub namn: String,
}

impl NyttKonto {
    pub fn kontrollera(&self) -> Result<(Kontonummer, String), Vec<Kontofel>> {
        let mut fel = Vec::new();
        let mut nytt = |fält, kod, text: &str| {
            fel.push(Kontofel {
                fält,
                kod,
                text: text.into(),
            })
        };
        let text = self.nummer.trim();
        let nummer = if text.len() != 4 || !text.bytes().all(|b| b.is_ascii_digit()) {
            nytt(KontoFält::Nummer, "nummer_format", "Ange fyra siffror.");
            None
        } else if text.starts_with('9') {
            nytt(
                KontoFält::Nummer,
                "nummer_klass_9",
                "Klass 9 används inte i Maya.",
            );
            None
        } else if let Some(nummer) = text.parse().ok().and_then(Kontonummer::new) {
            Some(nummer)
        } else {
            nytt(
                KontoFält::Nummer,
                "nummer_klass",
                "Kontonummer börjar på 1–8.",
            );
            None
        };
        let namn = self.namn.trim();
        if namn.is_empty() {
            nytt(KontoFält::Namn, "namn_tom", "Ange ett namn.");
        }
        match nummer {
            Some(nummer) if fel.is_empty() => Ok((nummer, namn.to_string())),
            _ => Err(fel),
        }
    }
}
