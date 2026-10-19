//! Systemdokumentationen som dokument (S1–S6): avsnitt med block av text, listor och tabeller,
//! och de inbyggda behandlingsreglerna som den återger.
//!
//! `core` sätter ihop avsnitten ur företagets data och reglerna här; webben visar samma
//! [`Systemdokumentation`] som sida och `maya-pdf` skriver den som PDF. Reglerna står här som
//! text bredvid konstanterna de beskriver, så att texten följer med när en regel ändras.
//!
//! Tabellen över automatkonteringar byggs i [`crate::automatkontering`] ur kontona som flödena
//! konterar med och återges ordagrant som i specen.

use serde::{Deserialize, Serialize};

use crate::att_gora::VARNING_EFTER_DAGAR;
use crate::automatkontering;
use crate::faktura::Momssats;
use crate::ingaende_balans::PRELIMINÄRT_RESULTAT;
use crate::kontoplan::flödenas_konton;
use crate::kundfaktura::{KUNDFORDRINGAR, Momsläge};
use crate::leverantorsfaktura::LEVERANTÖRSSKULDER;
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

/// Tabellen över automatkonteringar med styckena runt den, ordagrant som i specen. Den byggs ur
/// [`crate::automatkontering`], alltså ur kontona som flödena konterar med.
pub fn automatkonteringar() -> Vec<Block> {
    let mut block: Vec<Block> = automatkontering::inledning()
        .iter()
        .map(|s| Block::Stycke(Text::markdown(s)))
        .collect();
    block.push(Block::Tabell(Tabell {
        kolumner: automatkontering::KOLUMNER.map(String::from).into(),
        rader: automatkontering::tabell()
            .iter()
            .map(|r| {
                Tabellrad::Rad(
                    [&r.namn, &r.debet, &r.kredit, &r.villkor]
                        .map(|ruta| Text::markdown(&ruta.markdown()))
                        .into(),
                )
            })
            .collect(),
    }));
    block.push(Block::Stycke(Text::markdown(automatkontering::AVSLUTNING)));
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
    // #150
    (
        "t2_vands_inte",
        "T2-verifikatet under kontantmetoden vänds inte; fakturorna i det betalas mot 1510 och \
         2440.",
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
        // #145 Leverantörsfakturor
        Block::Underrubrik("Spärrar vid registrering av leverantörsfaktura".into()),
        Block::Stycke(
            "Formuläret och servern prövar samma spärrar. En leverantörsfaktura som inte klarar \
             dem kan sparas som utkast men inte registreras. En annan registrerad faktura från \
             samma leverantör med samma belopp och fakturadatum men ett annat nummer ger en \
             varning som inte spärrar. Leverantörens standardkonto fyller rader utan konto."
                .into(),
        ),
        kodtabell(crate::leverantorsfaktura::REGISTRERINGSSPÄRRAR),
        // #135 Kundfakturor
        Block::Underrubrik("Spärrar vid utfärdande av kundfaktura".into()),
        Block::Stycke(
            "Formuläret, förhandsvisningen och servern prövar samma spärrar, och varje spärr \
             visas vid sitt fält med koden nedan. En kundfaktura som inte klarar dem kan sparas \
             som utkast men inte utfärdas. Intäktskontot följer radens momssats (se \
             Intäktskonto per momssats) och kan ändras per rad till ett annat aktivt konto \
             i klass 3. Momsen räknas per momssats på summan av radernas netto och avrundas \
             till öre."
                .into(),
        ),
        kodtabell(crate::kundfaktura::UTFÄRDANDESPÄRRAR),
        // #142 Rätta kontering
        Block::Underrubrik("Spärrar när konteringen av en kundfaktura rättas".into()),
        Block::Stycke(
            "Rätta kontering väljer ett nytt intäktskonto per rad på en utfärdad kundfaktura. \
             Utfärdandets verifikat vänds med en rättelse och ett nytt utfärdandeverifikat \
             bokförs med dagens datum i samma transaktion, med samma fordran och moms och \
             intäkterna på de nya kontona. Fakturan, numret och PDF:en ändras inte."
                .into(),
        ),
        kodtabell(crate::kontering::KONTERINGSSPÄRRAR),
        // #139 Betalningar
        Block::Underrubrik("Spärrar vid betalning av faktura".into()),
        Block::Stycke(
            "Panelen och servern prövar samma spärrar. Beloppet är förvalt det som återstår, och \
             betalkontot 1930, som kan ändras till ett annat aktivt konto i klass 1. En \
             delbetalning lämnar resten kvar; en rest på högst 10 kr kan stängas mot \
             öresutjämning (3740) i samma verifikat. Rätta betalning vänder betalningens \
             verifikat med en rättelse och tar bort betalningen ur reskontran."
                .into(),
        ),
        kodtabell(crate::betalning::BETALNINGSSPÄRRAR),
        // #141 Kundförlust
        Block::Underrubrik("Spärrar vid kundförlust".into()),
        Block::Stycke(
            "Bara en konstaterad kundförlust bokförs i fakturan, med skäl (konkurs, ackord, \
             utmätning utan resultat eller annat med fritext) och datum. Den gäller det som \
             återstår: 6351 debiteras med nettots andel och varje utgående momskonto med sin \
             andel, fördelat proportionellt mot utfärdandets netto och moms, och 1510 \
             krediteras. När fordran inte är bokförd blir förlusten bara en status. Återkalla \
             kundförlust vänder förlustens verifikat med ett nytt verifikat daterat i dag, så \
             att fakturan kan betalas igen. En befarad kundförlust bokförs som ett manuellt \
             verifikat."
                .into(),
        ),
        kodtabell(crate::kundforlust::KUNDFÖRLUSTSPÄRRAR),
        // #140 Kreditfakturor
        Block::Underrubrik("Spärrar när en kreditfaktura skapas".into()),
        Block::Stycke(
            "Kreditfaktura… på en utfärdad kundfaktura krediterar hela fakturan eller ett antal \
             per rad och skapar ett utkast med originalets rader med negativt antal, \
             originalets momsläge, kund och intäktskonton och en hänvisning till originalet. \
             Kreditfakturan utfärdas med samma spärrar som en faktura och nästa nummer i samma \
             serie, och dessutom spärras den om summan inte är negativ eller om den krediterar \
             mer än det som finns kvar att kreditera. Verifikatet speglar originalet: \
             intäktskontona och utgående moms i debet och 1510 i kredit. Är fakturan redan \
             betald får kunden ett tillgodohavande, som återbetalas som en betalning åt andra \
             hållet: 1510 i debet mot betalkontot."
                .into(),
        ),
        kodtabell(crate::kreditfaktura::KREDITERINGSSPÄRRAR),
        // #143 Externa kundfakturor
        Block::Underrubrik("Spärrar när en extern kundfaktura registreras".into()),
        Block::Stycke(
            "En extern kundfaktura har skapats i ett annat system och registreras med sin PDF \
             som underlag, kund, det andra systemets fakturanummer, fakturadatum, förfallodag \
             och belopp per momssats och intäktskonto. Momsen räknas per momssats på summan av \
             radernas belopp, och registreringen bokförs som ett utfärdande: 1510 i debet mot \
             intäktskontona och utgående moms, daterat på fakturadatumet med PDF:en som \
             underlag. Externa fakturor ingår inte i fakturaserien. En extern kreditfaktura \
             registreras från originalets dokument med negativa belopp och en hänvisning. Rätta \
             registrering vänder registreringens verifikat med en rättelse och låser upp \
             fakturan med betalningarna kvar; den registreras om med samma spärrar."
                .into(),
        ),
        kodtabell(crate::extern_kundfaktura::REGISTRERINGSSPÄRRAR),
    ]);
    // #155 Fakturaflödenas regler
    block.extend(fakturaflödenas_regler());
    block
}

/// Intäktskontot som en kundfakturarad får som förval, per momssats och momsläge, ur
/// [`Momsläge::intäktskonto`].
fn intäktskontona() -> Block {
    let mut rader: Vec<Tabellrad> = Momssats::ALLA
        .into_iter()
        .map(|sats| {
            Tabellrad::Rad(vec![
                sats.to_string().into(),
                Momsläge::Moms.intäktskonto(sats).get().to_string().into(),
            ])
        })
        .collect();
    rader.extend(
        [
            ("Omvänd skattskyldighet, bygg", Momsläge::Bygg),
            ("Omvänd skattskyldighet, EU-tjänst", Momsläge::EuTjänst),
            ("Företaget är inte momsregistrerat", Momsläge::UtanMoms),
        ]
        .map(|(namn, läge)| {
            Tabellrad::Rad(vec![
                namn.into(),
                läge.intäktskonto(Momssats::Noll).get().to_string().into(),
            ])
        }),
    );
    Block::Tabell(Tabell {
        kolumner: vec!["Momssats".into(), "Intäktskonto".into()],
        rader,
    })
}

/// Reglerna för fakturaflödena som inte är automatkonteringar eller spärrtabeller (S2):
/// intäktskontona, den proportionella fördelningen, registren, bokföringsmetoden, T2,
/// statusen i reskontran och spärrarna som håller reskontran i takt med bokföringen.
fn fakturaflödenas_regler() -> Vec<Block> {
    let kundfordringar = KUNDFORDRINGAR;
    let leverantörsskulder = LEVERANTÖRSSKULDER;
    vec![
        Block::Underrubrik("Intäktskonto per momssats".into()),
        Block::Stycke(
            "En rad på en kundfaktura får intäktskontot efter sin momssats och fakturans \
             omvända skattskyldighet. Det kan ändras per rad till ett annat aktivt intäktskonto \
             i klass 3, och Rätta kontering byter det på en utfärdad faktura."
                .into(),
        ),
        intäktskontona(),
        Block::Underrubrik("Proportionell fördelning".into()),
        Block::Stycke(
            "Under kontantmetoden, vid delkredit, kundförlust och T2 fördelas ett belopp på \
             fakturans konton och momssatser i proportion till deras andel av fakturans total, \
             avrundat till öre så att summan stämmer exakt. Avrundningen är största resten: \
             varje del avrundas först nedåt, och de öre som fattas går till delarna med störst \
             rest, vid lika rest till den som kommer först. Ett negativt belopp fördelas som det \
             positiva med omvänt tecken. Varje del avviker mindre än ett öre från sin exakta \
             andel. Den fiktiva momsen vid omvänd skattskyldighet ingår inte i totalen och \
             fördelas som ett par som summerar till noll."
                .into(),
        ),
        Block::Underrubrik("Leverantörens standardkonto".into()),
        Block::Stycke(
            "En leverantörs standardkonto är en automatkontering: det fyller rader utan konto när \
             en leverantörsfaktura från leverantören skrivs av, och raden bokförs på det när \
             fakturan registreras. Standardkontot är ett aktivt konto i kontoplanen. Varje \
             ändring av det loggas i behandlingshistoriken som \"standardkonto ändrat\", också \
             när leverantören läggs upp med ett. Leverantörens förvalda omvända \
             skattskyldighet fylls i på samma sätt och kan ändras på fakturan."
                .into(),
        ),
        Block::Underrubrik("Kunder och leverantörer".into()),
        Block::Stycke(
            "Kund- och leverantörsregistret håller motparterna. Kundens namn, identitet, adress \
             och momsreg-nr kopieras in på fakturan när den utfärdas, och förfallodagen föreslås \
             som fakturadatumet plus kundens betalningsvillkor; en senare ändring i registret \
             ändrar inte en utfärdad faktura. Kundens förvalda omvända skattskyldighet fylls i \
             på utkastet. En kund eller leverantör som läggs upp från ett utkast har statusen ny \
             och hör till utkastet: den blir verklig när fakturan utfärdas eller registreras och \
             raderas, med Maya som aktör och loggat, när inget utkast längre hänvisar till den. \
             En kund eller leverantör med fakturor kan arkiveras men inte raderas, och \
             arkiverade visas inte i väljarna. Varje ändring i registren loggas med från och \
             till per fält."
                .into(),
        ),
        Block::Underrubrik("Bokföringsmetoden och reskontrakontot".into()),
        Block::Stycke(
            format!(
                "Varje faktura vet om dess fordran är bokförd på {kundfordringar} eller dess \
                 skuld på {leverantörsskulder}. Under fakturametoden är den bokförd från \
                 utfärdandet eller registreringen, under kontantmetoden från T2-verifikatet. \
                 Betalning, kredit och kundförlust bokförs mot reskontrakontot när fordran eller \
                 skulden är bokförd och annars mot intäkt eller kostnad och moms, enligt \
                 automatkonteringarna. Bokföringsmetoden på fakturadatumet avgör om utfärdandet \
                 eller registreringen bokförs, så ett byte av metod kräver ingen \
                 övergångsbokning; ett byte gäller bara framåt och spärras från ett datum då \
                 det redan finns utfärdade eller registrerade fakturor (fakturor_efter). En \
                 kreditfaktura följer sitt original; en kreditfaktura från en leverantör utan \
                 hänvisning till ett original räknas som bokförd. En betalning, kredit eller kundförlust \
                 daterad efter räkenskapsårets slut på en faktura under kontantmetoden som var \
                 obetald vid årets slut bokförs mot {kundfordringar} eller \
                 {leverantörsskulder}, också innan T2-verifikatet är bokfört."
            )
            .into(),
        ),
        Block::Underrubrik("T2 under kontantmetoden".into()),
        Block::Stycke(
            "Dagen efter räkenskapsårets slut skapar Maya, som aktör, ett T2-utkast daterat på \
             årets sista dag för ett företag med kontantmetoden. Det har det obetalda beloppet \
             vid årets slut på varje kund- och leverantörsfaktura med fakturadatum i året vars \
             fordran eller skuld inte är bokförd, fördelat proportionellt enligt \
             automatkonteringarna. Utan obetalda fakturor skapas inget utkast. Utkastet räknas \
             om varje gång en ändring sparas, och Maya raderar det när inget längre är obetalt. \
             Det kan inte ändras eller raderas för hand. En användare bokför det, och då är \
             fakturornas fordringar och skulder bokförda med T2-verifikatet, som vänds aldrig. \
             Det kan inte bokföras medan en faktura från året är upplåst med Rätta registrering \
             eller Rätta avskrift; fakturan registreras om först."
                .into(),
        ),
        Block::Underrubrik("Status i reskontran".into()),
        Block::Stycke(
            "En fakturas status räknas fram ur dess total, krediteringar, betalningar och \
             kundförluster, förfallodagen och dagens datum, och lagras inte: obetald, \
             delbetald, betald, förfallen när något återstår efter förfallodagen, krediterad, \
             tillgodo när motparten har betalat mer än det som återstår efter krediteringarna, \
             och kundförlust. Att en kundfaktura har skickats är en loggad händelse och ingen \
             status. En leverantörsfaktura eller extern kundfaktura vars registrering är vänd \
             är upplåst tills den registreras om."
                .into(),
        ),
        Block::Underrubrik("Spärrar som håller reskontran i takt med bokföringen".into()),
        Block::Stycke(
            format!(
                "Utöver spärrtabellerna ovan spärrar Maya det som skulle få reskontran och \
                 {kundfordringar} eller {leverantörsskulder} att gå isär:"
            )
            .into(),
        ),
        Block::Lista(vec![
            "Rätta registrering och Rätta avskrift spärras när fakturan ingår i ett bokfört \
             T2-verifikat, eftersom T2 aldrig vänds."
                .into(),
            "T2-utkastet bokförs inte medan en faktura från året är upplåst.".into(),
            format!(
                "Rätta betalning och Återkalla kundförlust spärras när något har bokförts på \
                 {kundfordringar} eller {leverantörsskulder} utifrån betalningen eller förlusten, \
                 till exempel en senare kredit som översteg det obetalda: rättelsen får bara \
                 ändra fakturans del på kontot med det som dess eget verifikat bokförde där."
            )
            .into(),
            "En återbetalning dateras inte före den dag då tillgodohavandet uppstod \
             (aterbetalning_fore_tillgodo)."
                .into(),
            "Bokföringsmetoden byts inte från ett datum då det finns utfärdade eller \
             registrerade fakturor (fakturor_efter)."
                .into(),
        ]),
    ]
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
