//! Systemdokumentationen som dokument (S1–S6): avsnitt med block av text, listor och tabeller,
//! och de inbyggda behandlingsreglerna som den återger.
//!
//! `core` sätter ihop avsnitten ur företagets data och reglerna här; webben visar samma
//! [`Systemdokumentation`] som sida och `maya-pdf` skriver den som PDF. Reglerna står här som
//! text bredvid konstanterna de beskriver, så att texten följer med när en regel ändras.
//!
//! Tabellen över automatkonteringar läses ur `data/regler/automatkonteringar.md`, som är
//! avsnittet "Automatkonteringar" i specen ordagrant. Ändras tabellen ändras den där.

use serde::{Deserialize, Serialize};

use crate::att_gora::VARNING_EFTER_DAGAR;
use crate::ingaende_balans::PRELIMINÄRT_RESULTAT;
use crate::kontoplan::flödenas_konton;
use crate::verifikat::SPÄRRAR;

/// Hela systemdokumentationen för ett företag.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Systemdokumentation {
    pub företag: String,
    /// Organisationsnumret som det visas, t.ex. `556036-0793`.
    pub orgnr: String,
    /// Mayas version, t.ex. `0.1.0`.
    pub version: String,
    /// INK2-tabellens utgåva.
    pub ink2: String,
    /// BAS-versionen som kontoplanen bygger på.
    pub bas: String,
    pub avsnitt: Vec<Avsnitt>,
}

/// Ett avsnitt, med kraven i `docs/bfl-kontroll.md` som det uppfyller.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Avsnitt {
    /// Ankaret på sidan, t.ex. `kontoplan`.
    pub id: String,
    pub rubrik: String,
    /// Krav-id:n, t.ex. `S1`.
    pub krav: Vec<String>,
    pub block: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Block {
    Underrubrik(String),
    Stycke(Text),
    Lista(Vec<Text>),
    /// Rubrik och värde, t.ex. "Version" och "0.1.0".
    Fakta(Vec<(String, Text)>),
    Tabell(Tabell),
    /// En sida eller fil i Maya; `href` är en sökväg på servern.
    Länk {
        text: String,
        href: String,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tabell {
    pub kolumner: Vec<String>,
    pub rader: Vec<Tabellrad>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tabellrad {
    /// En rubrik över raderna under den, över alla kolumner.
    Rubrik(String),
    Rad(Vec<Text>),
}

/// Text med fetstil och kursiv.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Text(pub Vec<Del>);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Del {
    pub text: String,
    pub stil: Stil,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stil {
    Vanlig,
    Fet,
    Kursiv,
}

impl Text {
    /// Texten med `**fetstil**` och `*kursiv*` som i Markdown.
    pub fn markdown(text: &str) -> Text {
        let mut delar = Vec::new();
        let mut rest = text;
        while !rest.is_empty() {
            let (markör, stil) = if rest.starts_with("**") {
                ("**", Stil::Fet)
            } else if rest.starts_with('*') {
                ("*", Stil::Kursiv)
            } else {
                let slut = rest.find('*').unwrap_or(rest.len());
                delar.push(Del {
                    text: rest[..slut].to_string(),
                    stil: Stil::Vanlig,
                });
                rest = &rest[slut..];
                continue;
            };
            let efter = &rest[markör.len()..];
            match efter.find(markör) {
                Some(slut) => {
                    delar.push(Del {
                        text: efter[..slut].to_string(),
                        stil,
                    });
                    rest = &efter[slut + markör.len()..];
                }
                None => {
                    delar.push(Del {
                        text: rest.to_string(),
                        stil: Stil::Vanlig,
                    });
                    rest = "";
                }
            }
        }
        Text(delar)
    }

    /// Texten utan stil.
    pub fn ren(&self) -> String {
        self.0.iter().map(|d| d.text.as_str()).collect()
    }
}

impl From<&str> for Text {
    fn from(text: &str) -> Self {
        Text::from(text.to_string())
    }
}

impl From<String> for Text {
    fn from(text: String) -> Self {
        Text(vec![Del {
            text,
            stil: Stil::Vanlig,
        }])
    }
}

const AUTOMATKONTERINGAR: &str = include_str!("../data/regler/automatkonteringar.md");

/// Tabellen över automatkonteringar med styckena runt den, ordagrant ur specen.
pub fn automatkonteringar() -> Vec<Block> {
    let mut block = Vec::new();
    let mut tabell: Option<Tabell> = None;
    for rad in AUTOMATKONTERINGAR.lines().map(str::trim) {
        if let Some(innehåll) = rad.strip_prefix('|') {
            let celler: Vec<&str> = innehåll
                .strip_suffix('|')
                .unwrap_or(innehåll)
                .split('|')
                .map(str::trim)
                .collect();
            if celler
                .iter()
                .all(|c| c.chars().all(|t| t == '-' || t == ':'))
            {
                continue;
            }
            match &mut tabell {
                None => {
                    tabell = Some(Tabell {
                        kolumner: celler.iter().map(|c| c.to_string()).collect(),
                        rader: Vec::new(),
                    })
                }
                Some(t) => t.rader.push(Tabellrad::Rad(
                    celler.into_iter().map(Text::markdown).collect(),
                )),
            }
            continue;
        }
        if let Some(t) = tabell.take() {
            block.push(Block::Tabell(t));
        }
        if !rad.is_empty() {
            block.push(Block::Stycke(Text::markdown(rad)));
        }
    }
    if let Some(t) = tabell {
        block.push(Block::Tabell(t));
    }
    block
}

/// Spärrarna vid rättelse, utöver spärrarna vid bokföring: koden och regeln.
const RÄTTELSESPÄRRAR: &[(&str, &str)] = &[
    ("redan_rattat", "Ett verifikat kan rättas högst en gång."),
    ("ar_rattelse", "En rättelse kan inte rättas."),
    (
        "fore_originalet",
        "Rättelsens datum är tidigast originalets datum.",
    ),
    (
        "fran_flode",
        "Ett verifikat från ett av Mayas flöden (en faktura) rättas i fakturan.",
    ),
];

fn kodtabell(koder: &[(&str, &str)]) -> Block {
    Block::Tabell(Tabell {
        kolumner: vec!["Kod".into(), "Regel".into()],
        rader: koder
            .iter()
            .map(|(kod, regel)| Tabellrad::Rad(vec![(*kod).into(), (*regel).into()]))
            .collect(),
    })
}

/// Behandlingsreglerna som gäller alla företag (S2): automatkonteringarna, spärrarna,
/// fristerna, preliminär ingående balans och kontoplanens regler.
pub fn behandlingsregler() -> Vec<Block> {
    let mut block = vec![Block::Underrubrik("Automatkonteringar".into())];
    block.extend(automatkonteringar());
    block.extend([
        Block::Underrubrik("Spärrar vid bokföring".into()),
        Block::Stycke(
            "Formuläret och servern prövar samma spärrar, och varje spärr visas vid sitt fält \
             med koden nedan. Ett verifikat som inte klarar dem kan sparas som utkast men inte \
             bokföras. Bokföra godkänner, låser och numrerar verifikatet i ett steg; ett \
             bokfört verifikat ändras aldrig (L1). Periodlåsning finns inte i den här \
             versionen, så alla perioder är öppna."
                .into(),
        ),
        kodtabell(SPÄRRAR),
        Block::Underrubrik("Rättelse".into()),
        Block::Stycke(
            "En rättelse är ett nytt verifikat som vänder originalet enligt tabellen ovan. \
             Beskrivningen blir \"Rättelse av V5\" eller \"Rättelse av V5, <orsak>\", och \
             rättelsen prövas med spärrarna vid bokföring och dessutom:"
                .into(),
        ),
        kodtabell(RÄTTELSESPÄRRAR),
        Block::Underrubrik(
            format!("{VARNING_EFTER_DAGAR}-dagarsvarningen")
                .as_str()
                .into(),
        ),
        Block::Stycke(
            format!(
                "Ett verifikatutkast vars verifikatdatum ligger i en månad som tog slut för mer \
                 än {VARNING_EFTER_DAGAR} dagar sedan visas med en varning under Att göra. \
                 Varningen spärrar inte, och Maya bokför aldrig automatiskt. Ett utkast på \
                 kassan har kontantfristen i stället."
            )
            .into(),
        ),
        Block::Underrubrik("Kontantfristen (T1, BFL 5:2)".into()),
        Block::Stycke(
            "Ett utkast med minst en rad, i debet eller kredit, på ett konto i grupp 191 \
             (kassa, även egna konton i gruppen) ska bokföras senast första arbetsdagen efter \
             verifikatdatumet. Det gäller in- och utbetalningar och flyttar mellan kassa och \
             bank, och utkast från alla kanaler. Arbetsdagar är måndag–fredag utom allmänna \
             helgdagar, midsommarafton, julafton och nyårsafton. Att göra visar utkastet på \
             fristens dag och som försenat efter den, och formuläret visar fristen som en \
             notis. Fristen varnar men spärrar inte, och utkastet visar aldrig \
             50-dagarsvarningen, eftersom BFNAR 2013:2 p. 3.6 inte gäller kontanta \
             betalningar."
                .into(),
        ),
        Block::Stycke("Det här ser Maya inte, och då varnar fristen inte:".into()),
        Block::Lista(vec![
            "en kontant betalning som aldrig registreras i Maya".into(),
            "ett verifikat som bokförs direkt utan att först sparas som utkast; det är \
             bokfört när det registreras, så det finns inget utkast att varna för"
                .into(),
        ]),
        Block::Underrubrik("Preliminär ingående balans".into()),
        Block::Stycke(
            format!(
                "För ett räkenskapsår vars föregående år finns i Maya och inte är stängt räknas \
                 den ingående balansen fram vid varje läsning enligt tabellen ovan och sparas \
                 inte: varje balanskonto i klass 1–2 får föregående års utgående balans, och \
                 resultatet som inte är ombokat (klass 3–8) läggs på {PRELIMINÄRT_RESULTAT}. \
                 Rapporterna märker den som preliminär. SIE4-exporten skriver den som vanliga \
                 #IB-poster utan markering, eftersom SIE 4 saknar en sådan. Det första årets \
                 ingående balans skrivs in i ett formulär där summan måste vara noll, och \
                 varje ändring loggas."
            )
            .into(),
        ),
        Block::Underrubrik("Kontoplanens regler".into()),
        Block::Lista(
            flödenas_konton()
                .iter()
                .map(|(konto, flöde)| {
                    Text::from(format!(
                        "Konto {} används av flödet {flöde}. Det är aktivt från början och kan \
                         inte inaktiveras.",
                        konto.get()
                    ))
                })
                .chain([
                    "Ett konto kan inte inaktiveras när det har ett saldo skilt från noll i ett \
                     räkenskapsår som inte är stängt."
                        .into(),
                    "Konton raderas aldrig, och ett kontonummer ändras aldrig.".into(),
                    "Ett eget konto kan inte skapas på ett befintligt nummer eller i klass 9."
                        .into(),
                    "En ny BAS-version lägger bara till nya konton, som inaktiva, och loggar det \
                     med Maya som aktör."
                        .into(),
                ])
                .collect(),
        ),
        // #135 Kundfakturor
        Block::Underrubrik("Spärrar vid utfärdande av kundfaktura".into()),
        Block::Stycke(
            "Formuläret, förhandsvisningen och servern prövar samma spärrar, och varje spärr \
             visas vid sitt fält med koden nedan. En kundfaktura som inte klarar dem kan sparas \
             som utkast men inte utfärdas. Intäktskontot följer radens momssats (25 % 3001, \
             12 % 3002, 6 % 3003, 0 % 3004) och kan ändras per rad till ett annat aktivt konto \
             i klass 3. Momsen räknas per momssats på summan av radernas netto och avrundas \
             till öre."
                .into(),
        ),
        kodtabell(crate::kundfaktura::UTFÄRDANDESPÄRRAR),
    ]);
    block
}

/// En release i ändringsloggen för behandlingsreglerna.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Release {
    /// Releasen, t.ex. `0.1`.
    pub version: &'static str,
    pub ändringar: &'static [&'static str],
}

/// Ändringsloggen för behandlingsreglerna, äldst först (S6). En release som ändrar en regel
/// lägger till sin rad här; ändringar per företag står i behandlingshistoriken.
const ÄNDRINGSLOGG: &[Release] = &[Release {
    version: "0.1",
    ändringar: &[
        "Första versionen av behandlingsreglerna.",
        "Kontoplanen BAS 2026 och INK2-tabellen INK2_P1-231002_241119.",
        "En verifikationsserie V per räkenskapsår, numrerad vid bokföringen.",
        "Kontoplanens regler: konton raderas aldrig, nya BAS-konton läggs till som inaktiva, \
         och ett konto med saldo kan inte inaktiveras.",
        "Ingående balans skrivs in för det första räkenskapsåret.",
        "Spärrarna vid bokföring och rättelse.",
        "Automatkonteringarna rättelse, registrera om och preliminär ingående balans.",
        "50-dagarsvarningen och kontantfristen för utkast.",
        "Flödets konto 2099 för preliminär ingående balans.",
    ],
}];

/// Ändringsloggen för behandlingsreglerna, äldst först.
pub fn ändringslogg() -> &'static [Release] {
    ÄNDRINGSLOGG
}
