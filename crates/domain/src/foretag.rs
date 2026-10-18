//! Formuläret för att lägga upp ett företag. Formuläret i webbläsaren och servern kontrollerar
//! det med samma kod, och varje fel har en kod som visas vid sitt fält.

use jiff::civil::Date;

use crate::orgnr::Organisationsnummer;
use crate::rakenskapsar::Räkenskapsår;

/// Fakturametoden eller kontantmetoden.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Bokföringsmetod {
    Faktura,
    Kontant,
}

impl Bokföringsmetod {
    pub const ALLA: [Bokföringsmetod; 2] = [Bokföringsmetod::Faktura, Bokföringsmetod::Kontant];

    pub fn kod(self) -> &'static str {
        match self {
            Bokföringsmetod::Faktura => "faktura",
            Bokföringsmetod::Kontant => "kontant",
        }
    }

    pub fn namn(self) -> &'static str {
        match self {
            Bokföringsmetod::Faktura => "Fakturametoden",
            Bokföringsmetod::Kontant => "Kontantmetoden",
        }
    }

    pub fn från_kod(kod: &str) -> Option<Self> {
        Self::ALLA.into_iter().find(|m| m.kod() == kod)
    }
}

/// Perioden som företaget redovisar moms för, eller att det inte är momsregistrerat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Momsperiod {
    Månad,
    Kvartal,
    Helår,
    EjMomsregistrerad,
}

impl Momsperiod {
    pub const ALLA: [Momsperiod; 4] = [
        Momsperiod::Månad,
        Momsperiod::Kvartal,
        Momsperiod::Helår,
        Momsperiod::EjMomsregistrerad,
    ];

    pub fn kod(self) -> &'static str {
        match self {
            Momsperiod::Månad => "manad",
            Momsperiod::Kvartal => "kvartal",
            Momsperiod::Helår => "helar",
            Momsperiod::EjMomsregistrerad => "ej_momsregistrerad",
        }
    }

    pub fn namn(self) -> &'static str {
        match self {
            Momsperiod::Månad => "Månad",
            Momsperiod::Kvartal => "Kvartal",
            Momsperiod::Helår => "Helår",
            Momsperiod::EjMomsregistrerad => "Ej momsregistrerad",
        }
    }

    pub fn från_kod(kod: &str) -> Option<Self> {
        Self::ALLA.into_iter().find(|m| m.kod() == kod)
    }
}

/// Formulärets fält, med namnen som formuläret använder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Fält {
    Orgnr,
    Namn,
    FörstaÅrStart,
    FörstaÅrSlut,
    Bokföringsmetod,
    Momsperiod,
    Gatuadress,
    Postnummer,
    Postort,
    Epost,
    Telefon,
    Säte,
    Momsregnr,
    FSkatt,
    /// Datumet som ett byte av bokföringsmetod eller momsperiod gäller från.
    GällerFrån,
}

impl Fält {
    pub fn namn(self) -> &'static str {
        match self {
            Fält::Orgnr => "orgnr",
            Fält::Namn => "namn",
            Fält::FörstaÅrStart => "start",
            Fält::FörstaÅrSlut => "slut",
            Fält::Bokföringsmetod => "bokforingsmetod",
            Fält::Momsperiod => "momsperiod",
            Fält::Gatuadress => "gatuadress",
            Fält::Postnummer => "postnummer",
            Fält::Postort => "postort",
            Fält::Epost => "epost",
            Fält::Telefon => "telefon",
            Fält::Säte => "sate",
            Fält::Momsregnr => "momsregnr",
            Fält::FSkatt => "f_skatt",
            Fält::GällerFrån => "galler_fran",
        }
    }
}

/// Ett fel vid ett fält.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fältfel {
    pub fält: Fält,
    pub kod: &'static str,
    pub text: String,
}

impl Fältfel {
    fn nytt(fält: Fält, kod: &'static str, text: impl ToString) -> Self {
        Fältfel {
            fält,
            kod,
            text: text.to_string(),
        }
    }
}

/// Formuläret som användaren fyllde i, som text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NyttFöretag {
    pub orgnr: String,
    pub namn: String,
    /// `ÅÅÅÅ-MM-DD`.
    pub första_år_start: String,
    /// `ÅÅÅÅ-MM-DD`.
    pub första_år_slut: String,
    pub bokföringsmetod: String,
    pub momsperiod: String,
}

/// Ett kontrollerat formulär.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GiltigtFöretag {
    pub orgnr: Organisationsnummer,
    pub namn: String,
    pub första_år: Räkenskapsår,
    pub bokföringsmetod: Bokföringsmetod,
    pub momsperiod: Momsperiod,
}

impl NyttFöretag {
    /// Kontrollerar formuläret. Felen kommer i fältens ordning.
    pub fn kontrollera(&self) -> Result<GiltigtFöretag, Vec<Fältfel>> {
        let mut fel = Vec::new();
        let orgnr = Organisationsnummer::tolka(&self.orgnr)
            .map_err(|e| fel.push(Fältfel::nytt(Fält::Orgnr, e.kod(), e)))
            .ok();
        let namn = self.namn.trim().to_string();
        if namn.is_empty() {
            fel.push(Fältfel::nytt(
                Fält::Namn,
                "namn_saknas",
                "Företaget måste ha ett namn.",
            ));
        }
        let start = datum(&self.första_år_start, Fält::FörstaÅrStart, &mut fel);
        let slut = datum(&self.första_år_slut, Fält::FörstaÅrSlut, &mut fel);
        let första_år = match (start, slut) {
            (Some(start), Some(slut)) => Räkenskapsår::första(start, slut)
                .map_err(|e| fel.push(Fältfel::nytt(Fält::FörstaÅrSlut, e.kod(), e)))
                .ok(),
            _ => None,
        };
        let bokföringsmetod = val(
            Bokföringsmetod::från_kod(&self.bokföringsmetod),
            Fält::Bokföringsmetod,
            "Välj bokföringsmetod.",
            &mut fel,
        );
        let momsperiod = val(
            Momsperiod::från_kod(&self.momsperiod),
            Fält::Momsperiod,
            "Välj momsperiod.",
            &mut fel,
        );
        match (orgnr, första_år, bokföringsmetod, momsperiod) {
            (Some(orgnr), Some(första_år), Some(bokföringsmetod), Some(momsperiod))
                if fel.is_empty() =>
            {
                Ok(GiltigtFöretag {
                    orgnr,
                    namn,
                    första_år,
                    bokföringsmetod,
                    momsperiod,
                })
            }
            _ => Err(fel),
        }
    }
}

fn datum(text: &str, fält: Fält, fel: &mut Vec<Fältfel>) -> Option<Date> {
    text.trim()
        .parse::<Date>()
        .map_err(|_| {
            fel.push(Fältfel::nytt(
                fält,
                "datum_ogiltigt",
                "Ange ett datum som ÅÅÅÅ-MM-DD.",
            ))
        })
        .ok()
}

fn val<T>(val: Option<T>, fält: Fält, text: &str, fel: &mut Vec<Fältfel>) -> Option<T> {
    if val.is_none() {
        fel.push(Fältfel::nytt(fält, "val_saknas", text));
    }
    val
}

/// Företagets uppgifter under Inställningar → Företag: namn, adress och kontaktuppgifter, som
/// kan ändras när som helst, och säte, momsregistreringsnummer och F-skatt, som senare flöden
/// kräver. Tom text betyder att uppgiften saknas.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Uppgifter {
    pub namn: String,
    pub gatuadress: String,
    /// `NNN NN` när det är kontrollerat.
    pub postnummer: String,
    pub postort: String,
    pub epost: String,
    pub telefon: String,
    /// Orten där styrelsen har sitt säte.
    pub säte: String,
    /// `SE` och tolv siffror när det är kontrollerat.
    pub momsregnr: String,
    pub f_skatt: bool,
}

impl Uppgifter {
    /// Kontrollerar uppgifterna och skriver dem på ett enhetligt sätt. Felen kommer i fältens
    /// ordning.
    pub fn kontrollera(&self) -> Result<Uppgifter, Vec<Fältfel>> {
        let mut fel = Vec::new();
        let namn = self.namn.trim().to_string();
        if namn.is_empty() {
            fel.push(Fältfel::nytt(
                Fält::Namn,
                "namn_saknas",
                "Företaget måste ha ett namn.",
            ));
        }
        let siffror: String = self
            .postnummer
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let postnummer = if siffror.is_empty() {
            String::new()
        } else if siffror.len() == 5 && siffror.bytes().all(|b| b.is_ascii_digit()) {
            format!("{} {}", &siffror[..3], &siffror[3..])
        } else {
            fel.push(Fältfel::nytt(
                Fält::Postnummer,
                "postnummer_ogiltigt",
                "Postnumret ska ha fem siffror.",
            ));
            siffror
        };
        let epost = match self.epost.trim() {
            "" => String::new(),
            adress => crate::epost::normalisera(adress).unwrap_or_else(|_| {
                fel.push(Fältfel::nytt(
                    Fält::Epost,
                    "epost_ogiltig",
                    "Ange en e-postadress som namn@exempel.se.",
                ));
                adress.to_string()
            }),
        };
        let momsregnr = self
            .momsregnr
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '-')
            .collect::<String>()
            .to_uppercase();
        if !momsregnr.is_empty()
            && !(momsregnr.len() == 14
                && momsregnr.starts_with("SE")
                && momsregnr[2..].bytes().all(|b| b.is_ascii_digit()))
        {
            fel.push(Fältfel::nytt(
                Fält::Momsregnr,
                "momsregnr_ogiltigt",
                "Momsregistreringsnumret är SE och tolv siffror, t.ex. SE556036079301.",
            ));
        }
        if !fel.is_empty() {
            return Err(fel);
        }
        Ok(Uppgifter {
            namn,
            gatuadress: self.gatuadress.trim().to_string(),
            postnummer,
            postort: self.postort.trim().to_string(),
            epost,
            telefon: self.telefon.trim().to_string(),
            säte: self.säte.trim().to_string(),
            momsregnr,
            f_skatt: self.f_skatt,
        })
    }
}

/// Det ett byte av bokföringsmetod eller momsperiod prövas mot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ändringsläge {
    /// Företagets räkenskapsår i ordning.
    pub räkenskapsår: Vec<Räkenskapsår>,
    /// Bokföringsmetoden med gäller-från-datum, äldst först.
    pub bokföringsmetoder: Vec<(Date, Bokföringsmetod)>,
    /// Momsperioden med gäller-från-datum, äldst först.
    pub momsperioder: Vec<(Date, Momsperiod)>,
    /// Datumet på det senast daterade bokförda verifikatet.
    pub senast_bokförda: Option<Date>,
    /// Datumet på det senast daterade verifikatet, bokfört eller utkast.
    pub senast_daterade: Option<Date>,
}

impl Ändringsläge {
    /// Räkenskapsåren som ett byte kan gälla från: de som finns och året efter det sista.
    fn år(&self) -> Vec<Räkenskapsår> {
        let mut år = self.räkenskapsår.clone();
        if let Some(sista) = år.last().copied() {
            år.push(sista.nästa());
        }
        år
    }
}

/// Ett byte av bokföringsmetod eller momsperiod som användaren fyllde i, som text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Byte {
    /// `ÅÅÅÅ-MM-DD`.
    pub gäller_från: String,
    /// Koden för den nya bokföringsmetoden eller momsperioden.
    pub val: String,
}

impl Byte {
    /// Ett byte av bokföringsmetod gäller från början av ett räkenskapsår efter det första,
    /// som finns eller är nästa, och som inte har några bokförda verifikat.
    pub fn kontrollera_bokföringsmetod(
        &self,
        läge: &Ändringsläge,
    ) -> Result<(Date, Bokföringsmetod), Vec<Fältfel>> {
        let mut fel = Vec::new();
        let från = datum(&self.gäller_från, Fält::GällerFrån, &mut fel);
        let metod = val(
            Bokföringsmetod::från_kod(&self.val),
            Fält::Bokföringsmetod,
            "Välj bokföringsmetod.",
            &mut fel,
        );
        let (Some(från), Some(metod)) = (från, metod) else {
            return Err(fel);
        };
        if !läge.år().iter().any(|år| år.start() == från) {
            return Err(vec![Fältfel::nytt(
                Fält::GällerFrån,
                "inte_arets_start",
                "Bokföringsmetoden byts från början av ett räkenskapsår, som det närmast \
                 följande.",
            )]);
        }
        prova_byte(
            &läge.bokföringsmetoder,
            från,
            metod,
            läge,
            Fält::Bokföringsmetod,
        )?;
        Ok((från, metod))
    }

    /// Ett byte av momsperiod gäller från början av en momsperiod, både med den period som
    /// gäller och med den nya: en kalendermånad, ett kalenderkvartal eller ett räkenskapsår.
    pub fn kontrollera_momsperiod(
        &self,
        läge: &Ändringsläge,
    ) -> Result<(Date, Momsperiod), Vec<Fältfel>> {
        let mut fel = Vec::new();
        let från = datum(&self.gäller_från, Fält::GällerFrån, &mut fel);
        let ny = val(
            Momsperiod::från_kod(&self.val),
            Fält::Momsperiod,
            "Välj momsperiod.",
            &mut fel,
        );
        let (Some(från), Some(ny)) = (från, ny) else {
            return Err(fel);
        };
        let år = läge.år();
        let börjar = |period: Momsperiod| match period {
            Momsperiod::Månad | Momsperiod::EjMomsregistrerad => från.day() == 1,
            Momsperiod::Kvartal => från.day() == 1 && matches!(från.month(), 1 | 4 | 7 | 10),
            Momsperiod::Helår => år.iter().any(|år| år.start() == från),
        };
        let nu = gäller(&läge.momsperioder, från);
        if !börjar(ny)
            || nu.is_some_and(|nu| !börjar(nu))
            || från > år.last().map_or(från, |å| å.slut())
        {
            return Err(vec![Fältfel::nytt(
                Fält::GällerFrån,
                "inte_momsperiodens_start",
                "Momsperioden byts från början av en momsperiod, både med den period som gäller \
                 och med den nya.",
            )]);
        }
        prova_byte(&läge.momsperioder, från, ny, läge, Fält::Momsperiod)?;
        Ok((från, ny))
    }
}

/// Ett nytt slutdatum för företagets första räkenskapsår: samma regler som vid uppläggningen
/// (Å1), så länge inget senare år finns, inget verifikat hamnar efter slutet och inget byte av
/// bokföringsmetod eller momsperiod är lagt efter året. Ingen period är låst i etapp 1.
pub fn kontrollera_första_årets_slut(
    slut: &str,
    läge: &Ändringsläge,
) -> Result<Räkenskapsår, Vec<Fältfel>> {
    let mut fel = Vec::new();
    let Some(slut) = datum(slut, Fält::FörstaÅrSlut, &mut fel) else {
        return Err(fel);
    };
    let ett = |kod, text: &str| Err(vec![Fältfel::nytt(Fält::FörstaÅrSlut, kod, text)]);
    let Some(första) = läge.räkenskapsår.first() else {
        return ett("ar_saknas", "Företaget har inget räkenskapsår.");
    };
    let år = Räkenskapsår::första(första.start(), slut)
        .map_err(|e| vec![Fältfel::nytt(Fält::FörstaÅrSlut, e.kod(), e)])?;
    if läge.räkenskapsår.len() > 1 {
        return ett(
            "senare_ar_finns",
            "Det finns redan ett senare räkenskapsår, så det första årets slut ligger fast.",
        );
    }
    if läge.senast_daterade.is_some_and(|d| d > slut) {
        return ett(
            "verifikat_efter_slut",
            "Det finns verifikat efter det slutdatumet.",
        );
    }
    let byte_efter = |från: &Date| *från > första.start();
    if läge.bokföringsmetoder.iter().any(|(d, _)| byte_efter(d))
        || läge.momsperioder.iter().any(|(d, _)| byte_efter(d))
    {
        return ett(
            "byte_efter_aret",
            "Ett byte av bokföringsmetod eller momsperiod är lagt efter året.",
        );
    }
    Ok(år)
}

/// Värdet som gäller på `datum` enligt en historik med gäller-från-datum, äldst först.
pub fn gäller<T: Copy>(historik: &[(Date, T)], datum: Date) -> Option<T> {
    historik
        .iter()
        .rev()
        .find(|(från, _)| *från <= datum)
        .map(|(_, v)| *v)
}

/// Det som gäller för båda bytena: efter det senaste bytet, inga bokförda verifikat från
/// datumet och ett annat val än det som gäller.
fn prova_byte<T: Copy + PartialEq>(
    historik: &[(Date, T)],
    från: Date,
    ny: T,
    läge: &Ändringsläge,
    fält: Fält,
) -> Result<(), Vec<Fältfel>> {
    if historik.last().is_some_and(|(senaste, _)| från <= *senaste) {
        return Err(vec![Fältfel::nytt(
            Fält::GällerFrån,
            "fore_senaste_bytet",
            "Ett byte gäller framåt, efter det som gäller nu.",
        )]);
    }
    if läge.senast_bokförda.is_some_and(|d| d >= från) {
        return Err(vec![Fältfel::nytt(
            Fält::GällerFrån,
            "bokfort_efter",
            "Det finns bokförda verifikat från det datumet. Ett byte gäller bara framåt.",
        )]);
    }
    if gäller(historik, från) == Some(ny) {
        return Err(vec![Fältfel::nytt(
            fält,
            "samma_som_nu",
            "Det är det som redan gäller.",
        )]);
    }
    Ok(())
}

/// Förslaget till momsregistreringsnummer för ett aktiebolag: `SE`, organisationsnumret och
/// `01`.
pub fn föreslaget_momsregnr(orgnr: &Organisationsnummer) -> String {
    format!("SE{}01", orgnr.siffror())
}
