//! Kundfakturor: listan och den valda fakturan sida vid sida, som Verifikat (variant D i
//! `prototype/44-kundfakturor`). Ett utkast visas som formuläret, med spärrarna vid fälten, en
//! förhandsvisning som fälls ut med Visa förhandsvisning eller P och Granska och utfärda. En
//! utfärdad faktura visas som ett dokument med den arkiverade PDF:en och verifikatet.
//!
//! Sidorna är `/foretag/<id>/kundfakturor` (`?f=<faktura>` väljer en, `&granska=1` öppnar
//! Granska och utfärda, `&radera=1` frågan, `&visa=1` förhandsvisningen) och
//! `/foretag/<id>/kundfakturor/ny`. Formuläret prövas med `maya_domain::kundfaktura` i
//! webbläsaren och i `core`.
//!
//! Handlingarna på en utfärdad faktura (betalning, kredit, skicka …) öppnas som paneler med
//! `?panel=<kod>`; varje senare ticket lägger till sin variant i [`Panel`].

use jiff::civil::Date;
use leptos::prelude::*;
use leptos_router::hooks::use_query_map;
use maya_domain::belopp::Saldo;
use maya_domain::faktura::Momssats;
use maya_domain::foretag::Momsperiod;
use maya_domain::kontoplan::Kontonummer;
use maya_domain::kreditfaktura::NyKreditering;
use maya_domain::kund::{self, NyKund, OmvändSkattskyldighet};
use maya_domain::kundfaktura::{
    Fakturakund, Fält, Förutsättningar, Momsläge, NY, NyFakturarad, NyKundfaktura, Radfält, Spärr,
    Säljare,
};
use maya_domain::rakenskapsar::Räkenskapsår;
use maya_domain::reskontra::Grupp;
use serde::{Deserialize, Serialize};

use crate::app::LoggaIn;
use crate::betalning::{Betalningar, Betalningspanel, Märke, ReskontraVy, RättaBetalningspanel};
use crate::foretag::IngenÅtkomst;
use crate::kunder::kundregistret;
use crate::sidodata::sida;
use crate::utskick::{SkickaPanel, SkickaVy, UtskickVy, Utskicken};
use crate::verifikat::{UtkastBadge, företagets_bas, inloggad};

/// Adressen till Kundfakturor för företaget med sidan `bas` (`/foretag/<id>`).
pub fn kundfakturorna(bas: &str) -> String {
    format!("{bas}/kundfakturor")
}

/// Ett belopp i öre som fakturan skriver det.
fn kr(öre: i64) -> String {
    Saldo::från_öre(öre).to_string()
}

/// En faktura i listan.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListVy {
    pub id: String,
    pub utkast: bool,
    /// Fakturanumret, tomt för ett utkast.
    pub nummer: String,
    pub kund: String,
    pub fakturadatum: String,
    pub förfallodag: String,
    pub total: String,
    /// Kunden är ny från utkastet (#137).
    pub ny_kund: bool,
    // #139 Betalningar
    /// Det som återstår; för ett utkast totalen.
    pub återstår: String,
    /// Märkets kod och namn för en utfärdad faktura.
    pub märke: Option<(String, String)>,
    /// Gruppen i listan.
    pub grupp: Option<Grupp>,
    /// "förfaller om 5 dagar" för en faktura med något kvar att betala.
    pub förfallotext: String,
    /// Fakturan har skapats i ett annat system (#143).
    pub extern_faktura: bool,
}

/// Det som formuläret prövas mot, i webbläsaren med samma regler som på servern.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FörutsättningarVy {
    pub kunder: Vec<Fakturakund>,
    pub säljare: Säljare,
    /// Kontona och om de är aktiva.
    pub konton: Vec<(u16, bool)>,
    /// Räkenskapsårens start och slut.
    pub räkenskapsår: Vec<(String, String)>,
    /// Momsperiodens gäller-från och kod.
    pub momsperioder: Vec<(String, String)>,
    pub nästa_nummer: Option<u64>,
    pub startnummer: u64,
    /// Aktiva konton i klass 3 med namn, för kontoväljaren.
    pub intäktskonton: Vec<(u16, String)>,
    /// Dagens datum, förvalt fakturadatum på en ny faktura.
    pub idag: String,
}

impl FörutsättningarVy {
    fn domän(&self) -> Förutsättningar {
        let datum = |d: &str| d.parse::<Date>().ok();
        Förutsättningar {
            kunder: self.kunder.clone(),
            säljare: self.säljare.clone(),
            konton: self
                .konton
                .iter()
                .filter_map(|(k, aktiv)| Some((Kontonummer::new(*k)?, *aktiv)))
                .collect(),
            räkenskapsår: self
                .räkenskapsår
                .iter()
                .filter_map(|(s, e)| Some(Räkenskapsår::befintligt(datum(s)?, datum(e)?)))
                .collect(),
            momsperioder: self
                .momsperioder
                .iter()
                .filter_map(|(d, kod)| Some((datum(d)?, Momsperiod::från_kod(kod)?)))
                .collect(),
            nästa_nummer: self.nästa_nummer,
            startnummer: self.startnummer,
        }
    }
}

/// Ett utkast med sitt formulär.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UtkastVy {
    pub id: String,
    pub formulär: NyKundfaktura,
    /// Tid och namn.
    pub skapad: (String, String),
}

/// En rad i konteringen: konto, kontonamn, debet och kredit.
pub type KonteringVy = (String, String, String, String);

/// Det som Granska och utfärda visar.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GranskningVy {
    /// Numret som fakturan får.
    pub nummer: String,
    pub fakturadatum: String,
    pub kontering: Vec<KonteringVy>,
    pub total: String,
    /// Kvarvarande spärrar som text; Utfärda är inaktiv när det finns några.
    pub spärrar: Vec<String>,
    /// #138: kundens e-postadress när Maya har SMTP, för kryssrutan "Skicka till …"; annars tom.
    pub skicka_till: String,
}

/// En rad på en utfärdad faktura.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FakturaradVy {
    pub beskrivning: String,
    pub antal: String,
    pub à_pris: String,
    pub rabatt: String,
    pub moms: String,
    pub konto: String,
    pub netto: String,
}

/// Den arkiverade PDF:en.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PdfVy {
    pub href: String,
    pub filnamn: String,
    pub arkiverad: String,
    pub sha256: String,
    pub storlek: String,
}

/// En utfärdad faktura som dokument.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FakturaVy {
    pub id: String,
    pub nummer: String,
    /// OCR-referensen eller numret, och om det är OCR.
    pub referens: (String, bool),
    pub kund: Fakturakund,
    pub fakturadatum: String,
    pub förfallodag: String,
    pub leveransdatum: String,
    pub er_referens: String,
    pub rader: Vec<FakturaradVy>,
    /// "Moms 25 % på 9 500,00" och momsen.
    pub momsrader: Vec<(String, String)>,
    pub netto: String,
    pub moms: String,
    pub total: String,
    /// Den omvända skattskyldighetens namn och fakturans text om den, eller `None`.
    pub omvänd: Option<(String, String)>,
    /// Företaget var inte momsregistrerat: ingen moms och inget momsreg-nr.
    pub utan_moms: bool,
    /// Verifikatets id och nummer.
    pub verifikat: (String, String),
    pub pdf: PdfVy,
    /// Tid, namn och kanal.
    pub utfärdad: (String, String, String),
    /// Tid, händelse och namn.
    pub historik: Vec<(String, String, String)>,
    /// #142: om utfärdandet har ett verifikat, så att konteringen kan rättas.
    pub kan_rätta_kontering: bool,
    /// #142: panelen Rätta kontering, när den är öppen.
    pub rätta_kontering: Option<crate::kontering::RättaKonteringVy>,
    /// #138: utskicken, panelen Skicka när den är öppen, och om Maya har SMTP.
    pub utskick: Vec<UtskickVy>,
    pub skicka: Option<SkickaVy>,
    pub epost: bool,
    /// Status, betalningar och det som återstår (#139).
    pub reskontra: Option<ReskontraVy>,
    /// Kundförlusterna och deras paneler (#141).
    pub kundförlust: Option<crate::kundforlust::KundförlustVy>,
    // #140 Kreditfakturor
    /// För en kreditfaktura: originalets id och hänvisningens text.
    pub krediterar: Option<(String, String)>,
    /// Kreditfakturorna som krediterar fakturan: id, nummer och total.
    pub krediteras_av: Vec<(String, String, String)>,
    /// Något finns kvar att kreditera.
    pub kan_kreditera: bool,
    /// Panelen Kreditfaktura… när den är öppen.
    pub kreditera: Option<KrediteraVy>,
}

/// Panelen Kreditfaktura… (#140): hela fakturan eller ett antal per rad.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KrediteraVy {
    /// Originalets rader: beskrivning, antal med enhet och netto.
    pub rader: Vec<(String, String, String)>,
    /// Det som finns kvar att kreditera.
    pub kvar: String,
    /// Valet när panelen öppnas, eller det som användaren skickade.
    pub start: NyKreditering,
    /// Spärrarna från servern, om valet avvisades.
    pub fel: Option<String>,
}

/// Det som Kundfakturor visar.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kundfakturadata {
    pub lista: Vec<ListVy>,
    pub valt: Option<String>,
    pub utkast: Option<UtkastVy>,
    pub dokument: Option<FakturaVy>,
    /// För ett nytt eller valt utkast.
    pub förutsättningar: Option<FörutsättningarVy>,
    pub granskning: Option<GranskningVy>,
    /// Kundfordringarna: det som återstår på de utfärdade fakturorna (#139).
    pub kundfordringar: String,
    // #143 Externa kundfakturor
    /// Den valda fakturan när den är extern.
    pub extern_faktura: Option<crate::externa_kundfakturor::ExternVy>,
    /// Sidan Ny → Extern faktura.
    pub extern_ny: bool,
}

/// Datan för `/foretag/<id>/kundfakturor` och `/foretag/<id>/kundfakturor/ny`.
#[server(prefix = "/api", endpoint = "kundfakturadata", input = server_fn::codec::GetUrl)]
pub async fn hämta_kundfakturadata(adress: String) -> Result<Kundfakturadata, ServerFnError> {
    match crate::sidodata::Sidbegäran::för(&adress)? {
        Some(begäran) => server::kundfakturadata(&begäran).await,
        None => Ok(Kundfakturadata::default()),
    }
}

/// Felkoderna i adressen och deras texter.
fn feltext(kod: &str) -> Option<&'static str> {
    Some(match kod {
        "utkast" => "Utkastet kunde inte sparas. Rätta det som är markerat.",
        "sparrat" => "Fakturan kan inte utfärdas än. Rätta det som är markerat.",
        "kontant" => {
            "Under kontantmetoden utfärdas kundfakturor i en senare version av Maya. Fakturan \
             är kvar som utkast."
        }
        "startnummer" => "Startnumret kan inte ändras efter första utfärdandet.",
        "finns_inte" => "Utkastet finns inte längre eller är redan utfärdat.",
        // #142 Rätta kontering
        "kontering" => "Konteringen kunde inte rättas. Rätta det som är markerat.",
        "rattelse" => {
            "Konteringen kan inte rättas i dag: rättelsen och det nya verifikatet dateras i dag, \
             och det datumet måste gå att bokföra på och vara tidigast utfärdandets datum."
        }
        "ingen_kontering" => "Fakturans utfärdande har inget verifikat att rätta.",
        // #138 Utskick
        "skickfel" => {
            "Utskicket misslyckades och loggades. Fakturan är utfärdad och kan skickas igen."
        }
        "mottagare" => "Mottagaren är ingen e-postadress. Inget skickades.",
        // #140 Kreditfakturor
        "kreditering" => "Kreditfakturan kunde inte skapas.",
        kod => {
            return crate::betalning::feltext(kod)
                .or_else(|| crate::kundforlust::feltext(kod))
                .or_else(|| crate::externa_kundfakturor::feltext(kod));
        }
    })
}

/// Formulärets fältnamn för radfälten, med radens nummer efter: `beskrivning_0`.
const RADFÄLT: [&str; 7] = [
    "beskrivning",
    "antal",
    "enhet",
    "a_pris",
    "rabatt",
    "momssats",
    "konto",
];

fn radens<'a>(r: &'a mut NyFakturarad, namn: &str) -> &'a mut String {
    match namn {
        "beskrivning" => &mut r.beskrivning,
        "antal" => &mut r.antal,
        "enhet" => &mut r.enhet,
        "a_pris" => &mut r.à_pris,
        "rabatt" => &mut r.rabatt,
        "momssats" => &mut r.momssats,
        _ => &mut r.konto,
    }
}

/// Fälten för en ny kund i det streckade kortet (#137): formulärets namn, etikett och fält.
const NYA_KUNDFÄLT: [(&str, &str, kund::Fält); 6] = [
    ("ny_namn", "Namn", kund::Fält::Namn),
    (
        "ny_identitet",
        "Orgnr eller personnr",
        kund::Fält::Identitet,
    ),
    (
        "ny_adress",
        "Adress (gata, postnr, ort)",
        kund::Fält::Adress,
    ),
    ("ny_epost", "E-post", kund::Fält::Epost),
    ("ny_momsregnr", "Momsreg-nr", kund::Fält::Momsregnr),
    (
        "ny_betalningsvillkor",
        "Betalningsvillkor, dagar",
        kund::Fält::Betalningsvillkor,
    ),
];

fn ny_kunds<'a>(k: &'a mut NyKund, namn: &str) -> &'a mut String {
    match namn {
        "ny_namn" => &mut k.namn,
        "ny_identitet" => &mut k.identitet,
        "ny_adress" => &mut k.adress,
        "ny_epost" => &mut k.epost,
        "ny_momsregnr" => &mut k.momsregnr,
        _ => &mut k.betalningsvillkor,
    }
}

/// Formuläret ur fälten i ett inskickat formulär eller i adressen.
fn ur_fält(värde: impl Fn(&str) -> Option<String>) -> NyKundfaktura {
    let fält = |namn: &str| värde(namn).unwrap_or_default();
    let antal = (0..500)
        .take_while(|i| värde(&format!("beskrivning_{i}")).is_some())
        .count();
    let mut ny_kund = NyKund::default();
    for (namn, _, _) in NYA_KUNDFÄLT {
        if let Some(v) = värde(namn) {
            *ny_kunds(&mut ny_kund, namn) = v;
        }
    }
    NyKundfaktura {
        kund: fält("kund"),
        ny_kund,
        krediterar: None,
        fakturadatum: fält("fakturadatum"),
        leveransdatum: fält("leveransdatum"),
        förfallodag: fält("forfallodag"),
        er_referens: fält("er_referens"),
        startnummer: fält("startnummer"),
        omvänd_skattskyldighet: fält("omvand_skattskyldighet"),
        rader: (0..antal)
            .map(|i| {
                let mut r = NyFakturarad::default();
                for namn in RADFÄLT {
                    *radens(&mut r, namn) = fält(&format!("{namn}_{i}"));
                }
                r
            })
            .collect(),
    }
}

/// Handlingarna på en utfärdad faktura som öppnas som paneler (`?panel=<kod>`). Tickets som
/// lägger till en handling (betalning #139, skicka #138, kredit #140, kundförlust #141, rätta
/// kontering #142) lägger till en variant, dess kod och dess panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    /// Rätta kontering (#142): `?panel=ratta`.
    RättaKontering,
    /// Skicka fakturan med e-post, eller markera den som lämnad på annat sätt (#138).
    Skicka,
    /// Registrera betalning (#139).
    Betalning,
    /// Rätta betalning, med betalningen i `&b=` (#139).
    RättaBetalning,
    /// Konstaterad kundförlust (#141).
    Kundförlust,
    /// Återkalla kundförlust (#141).
    ÅterkallaKundförlust,
    /// Kreditfaktura… (#140).
    Kreditera,
    /// Återbetala ett tillgodohavande (#140).
    Återbetala,
}

impl Panel {
    pub fn från_kod(kod: &str) -> Option<Self> {
        match kod {
            "ratta" => Some(Panel::RättaKontering),
            "skicka" => Some(Panel::Skicka),
            "betalning" => Some(Panel::Betalning),
            "ratta-betalning" => Some(Panel::RättaBetalning),
            "kundforlust" => Some(Panel::Kundförlust),
            "aterkalla-kundforlust" => Some(Panel::ÅterkallaKundförlust),
            "kreditera" => Some(Panel::Kreditera),
            "aterbetala" => Some(Panel::Återbetala),
            _ => None,
        }
    }

    pub fn kod(self) -> &'static str {
        match self {
            Panel::RättaKontering => "ratta",
            Panel::Skicka => "skicka",
            Panel::Betalning => "betalning",
            Panel::RättaBetalning => "ratta-betalning",
            Panel::Kundförlust => "kundforlust",
            Panel::ÅterkallaKundförlust => "aterkalla-kundforlust",
            Panel::Kreditera => "kreditera",
            Panel::Återbetala => "aterbetala",
        }
    }
}

#[cfg(feature = "ssr")]
pub(crate) mod server {
    use std::sync::Arc;

    use axum::extract::{Path, State};
    use axum::http::StatusCode;
    use axum::response::{IntoResponse, Redirect, Response};
    use axum::routing::{get, post};
    use axum::{Extension, Form, Router};
    use leptos::prelude::ServerFnError;
    use maya_core::atkomst::ÅtkomstFel;
    use maya_core::foretag::FöretagsId;
    use maya_core::inloggning::Inloggad;
    use maya_core::kundfaktura::{self, KundfakturaFel, KundfakturaId, Status};
    use maya_core::{Databas, Klocka, kontoplan};
    use maya_domain::faktura::Rabatt;
    use maya_domain::kundfaktura::Förutsättningar;

    use super::*;
    use crate::sidodata::{self, Sidbegäran};
    use crate::svar::{ingen_åtkomst, pdf, serverfel, text};
    use crate::verifikat::server::tid;

    #[derive(Clone)]
    pub(crate) struct KundfakturaState {
        pub db: Databas,
        pub klocka: Klocka,
        /// #138: e-postadaptern, `None` utan SMTP.
        pub epost: Option<crate::epost::SmtpEpost>,
    }

    pub(crate) fn routes(state: KundfakturaState) -> Router {
        Router::new()
            .route("/foretag/{id}/kundfakturor", post(skapa))
            .route("/foretag/{id}/kundfakturor/{f}", post(spara))
            .route("/foretag/{id}/kundfakturor/{f}/utfarda", post(utfärda))
            .route("/foretag/{id}/kundfakturor/{f}/radera", post(radera))
            .route(
                "/foretag/{id}/kundfakturor/{f}/forhandsvisning",
                get(förhandsvisning),
            )
            // #142 Rätta kontering
            .route(
                "/foretag/{id}/kundfakturor/{f}/ratta-kontering",
                post(crate::kontering::server::rätta),
            )
            // #139 Betalningar
            .route(
                "/foretag/{id}/kundfakturor/{f}/betalning",
                post(crate::betalning::server::registrera_kundbetalning),
            )
            .route(
                "/foretag/{id}/kundfakturor/{f}/betalningar/{b}/ratta",
                post(crate::betalning::server::rätta_kundbetalning),
            )
            // #141 Kundförlust
            .route(
                "/foretag/{id}/kundfakturor/{f}/kundforlust",
                post(crate::kundforlust::server::bokför),
            )
            .route(
                "/foretag/{id}/kundfakturor/{f}/kundforlust/{k}/aterkalla",
                post(crate::kundforlust::server::återkalla),
            )
            // #140 Kreditfakturor
            .route("/foretag/{id}/kundfakturor/{f}/kreditera", post(kreditera))
            .with_state(Arc::new(state.clone()))
            // #143 Externa kundfakturor
            .merge(crate::externa_kundfakturor::server::routes(
                state.db,
                state.klocka,
            ))
    }

    /// Hämtar det som Kundfakturor visar. Utan åtkomst blir datan tom.
    pub(crate) async fn kundfakturadata(b: &Sidbegäran) -> Result<Kundfakturadata, ServerFnError> {
        let (företag, ny, extern_ny) = match b.delar().as_slice() {
            ["foretag", id, "kundfakturor"] => (id.parse::<FöretagsId>().ok(), false, false),
            ["foretag", id, "kundfakturor", "ny"] => (id.parse::<FöretagsId>().ok(), true, false),
            // #143 Ny → Extern faktura
            ["foretag", id, "kundfakturor", "extern", "ny"] => {
                (id.parse::<FöretagsId>().ok(), false, true)
            }
            _ => (None, false, false),
        };
        let Some(företag) = företag else {
            return Ok(Kundfakturadata::default());
        };
        let valt = if ny || extern_ny {
            None
        } else {
            b.värde("f").and_then(|f| f.parse::<KundfakturaId>().ok())
        };
        match hämta(b, företag, ny, valt).await {
            Ok(data) => Ok(Kundfakturadata { extern_ny, ..data }),
            Err(KundfakturaFel::Åtkomst(
                ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte,
            )) => Ok(Kundfakturadata::default()),
            Err(e) => Err(sidodata::serverfel(e)),
        }
    }

    async fn hämta(
        b: &Sidbegäran,
        företag: FöretagsId,
        ny: bool,
        valt: Option<KundfakturaId>,
    ) -> Result<Kundfakturadata, KundfakturaFel> {
        let aktör = &b.inloggad.aktör;
        let idag = b.klocka.idag();
        let rader = kundfaktura::lista(&b.db, aktör, företag).await?;
        let kundfordringar: i64 = rader
            .iter()
            .filter(|f| f.status.i_reskontran())
            .map(|f| f.återstår.max(0))
            .sum();
        let lista = rader
            .into_iter()
            .map(|f| {
                // #143: en upplåst extern faktura står bland utkasten tills den registreras om.
                let utfärdad = f.förfallodag.filter(|_| f.status.i_reskontran());
                ListVy {
                    id: f.id.to_string(),
                    utkast: f.status == Status::Utkast,
                    extern_faktura: f.är_extern,
                    nummer: f.fakturanummer.clone(),
                    kund: f.kund.clone(),
                    fakturadatum: f.fakturadatum.map(|d| d.to_string()).unwrap_or_default(),
                    förfallodag: f.förfallodag.map(|d| d.to_string()).unwrap_or_default(),
                    total: kr(f.total),
                    ny_kund: f.ny_kund,
                    återstår: kr(f.återstår),
                    märke: match (f.status, utfärdad) {
                        (Status::Upplåst, _) => Some(maya_domain::reskontra::Märke::Upplåst),
                        (_, Some(d)) => Some(märke_för(&f, d, idag)),
                        _ => None,
                    }
                    .map(|m| (m.kod().to_string(), m.namn().to_string())),
                    grupp: Some(match utfärdad {
                        Some(d) => maya_domain::reskontra::grupp(f.återstår, d, idag),
                        None => Grupp::Utkast,
                    }),
                    förfallotext: utfärdad
                        .filter(|_| f.återstår > 0)
                        .map(|d| maya_domain::reskontra::förfallotext(d, idag))
                        .unwrap_or_default(),
                }
            })
            .collect();
        let mut data = Kundfakturadata {
            lista,
            kundfordringar: kr(kundfordringar),
            ..Kundfakturadata::default()
        };
        // #143: en extern faktura visas med sitt original och sina egna handlingar.
        if let Some(f) = valt
            && let Some(ext) =
                maya_core::extern_kundfaktura::fakturan(&b.db, aktör, företag, f).await?
        {
            data.valt = Some(f.to_string());
            data.extern_faktura =
                Some(crate::externa_kundfakturor::server::extern_vy(b, företag, &ext).await?);
            return Ok(data);
        }
        let faktura = match valt {
            Some(f) => match kundfaktura::kundfakturan(&b.db, aktör, företag, f).await {
                Ok(faktura) => Some(faktura),
                Err(KundfakturaFel::FinnsInte) => None,
                Err(e) => return Err(e),
            },
            None => None,
        };
        let konton = kontoplan::konton(&b.db, aktör, företag).await?;
        let kontonamn = |nr: u16| {
            konton
                .iter()
                .find(|k| k.nummer.get() == nr)
                .map(|k| k.namn.clone())
                .unwrap_or_default()
        };
        if let Some(f) = &faktura {
            data.valt = Some(f.id.to_string());
            match &f.utfärdad {
                Some(u) => {
                    let mut d = dokument(företag, f, u);
                    // #142 Rätta kontering: verifikatet och kontona som de är bokförda nu.
                    if let Some(k) =
                        crate::kontering::server::gällande(&b.db, aktör, företag, f.id).await?
                    {
                        d.verifikat = (
                            k.verifikat.id.to_string(),
                            k.verifikat
                                .nummer
                                .map(|n| n.to_string())
                                .unwrap_or_default(),
                        );
                        for (rad, r) in d.rader.iter_mut().zip(&k.rader) {
                            rad.konto = r.konto.to_string();
                        }
                        d.kan_rätta_kontering = true;
                        if b.värde("panel").and_then(|p| Panel::från_kod(&p))
                            == Some(Panel::RättaKontering)
                        {
                            d.rätta_kontering = Some(
                                crate::kontering::server::panelen(
                                    &b.db,
                                    aktör,
                                    företag,
                                    &k,
                                    b.klocka.idag(),
                                )
                                .await?,
                            );
                        }
                    }
                    // #138 Utskick
                    let panel = b.värde("panel").as_deref().and_then(Panel::från_kod);
                    let (utskick, skicka) = crate::utskick::server::utskicken(
                        b,
                        företag,
                        f.id,
                        panel == Some(Panel::Skicka),
                    )
                    .await
                    .map_err(|e| match e {
                        maya_core::utskick::UtskickFel::Åtkomst(e) => KundfakturaFel::Åtkomst(e),
                        _ => KundfakturaFel::FinnsInte,
                    })?;
                    (d.utskick, d.skicka, d.epost) = (utskick, skicka, b.config.smtp.is_some());
                    // #140: en kreditfaktura räknas av mot originalet och betalas inte.
                    let kvar = u.total + f.krediteras_av.iter().map(|k| k.total).sum::<i64>();
                    d.kan_kreditera = u.krediterar.is_none() && kvar > 0;
                    if d.kan_kreditera && panel == Some(Panel::Kreditera) {
                        d.kreditera = Some(kreditera_panelen(b, u, kvar));
                    }
                    // #139 Betalningar
                    let panel = b.värde("panel");
                    d.reskontra = if u.krediterar.is_some() {
                        None
                    } else {
                        Some(
                            crate::betalning::server::reskontra(
                                b,
                                företag,
                                maya_core::betalning::Faktura::Kund(f.id),
                                panel.as_deref(),
                            )
                            .await
                            .map_err(|e| match e {
                                maya_core::betalning::BetalningFel::Åtkomst(e) => {
                                    KundfakturaFel::Åtkomst(e)
                                }
                                maya_core::betalning::BetalningFel::FinnsInte => {
                                    KundfakturaFel::FinnsInte
                                }
                                e => KundfakturaFel::Åtkomst(ÅtkomstFel::Databas(
                                    maya_core::DatabasFel::Korrupt(e.to_string()),
                                )),
                            })?,
                        )
                    };
                    // #141 Kundförlust: en kreditfaktura skrivs inte av som kundförlust.
                    if u.krediterar.is_none() {
                        d.kundförlust = Some(
                            crate::kundforlust::server::kundförlusterna(
                                b,
                                företag,
                                f.id,
                                panel.as_deref(),
                            )
                            .await
                            .map_err(|e| match e {
                                maya_core::kundforlust::KundförlustFel::Åtkomst(e) => {
                                    KundfakturaFel::Åtkomst(e)
                                }
                                maya_core::kundforlust::KundförlustFel::FinnsInte => {
                                    KundfakturaFel::FinnsInte
                                }
                                e => KundfakturaFel::Åtkomst(ÅtkomstFel::Databas(
                                    maya_core::DatabasFel::Korrupt(e.to_string()),
                                )),
                            })?,
                        );
                    }
                    data.dokument = Some(d);
                }
                None => {
                    data.utkast = Some(UtkastVy {
                        id: f.id.to_string(),
                        formulär: f.formulär.clone(),
                        skapad: (tid(f.skapad.tid), f.skapad.namn.clone()),
                    });
                }
            }
        }
        if ny || data.utkast.is_some() {
            let förutsättningar = kundfaktura::förutsättningar(&b.db, aktör, företag).await?;
            if let (Some(u), true) = (&data.utkast, b.värde("granska").is_some()) {
                let mut g = granskning(&u.formulär, &förutsättningar, &kontonamn);
                // #138: kryssrutan bara med SMTP och en kund med e-postadress.
                if b.config.smtp.is_some() {
                    g.skicka_till = u
                        .formulär
                        .kunden(&förutsättningar)
                        .map(|k| k.epost)
                        .unwrap_or_default();
                }
                data.granskning = Some(g);
            }
            data.förutsättningar = Some(FörutsättningarVy {
                kunder: förutsättningar.kunder.clone(),
                säljare: förutsättningar.säljare.clone(),
                konton: förutsättningar
                    .konton
                    .iter()
                    .map(|(k, aktiv)| (k.get(), *aktiv))
                    .collect(),
                räkenskapsår: förutsättningar
                    .räkenskapsår
                    .iter()
                    .map(|å| (å.start().to_string(), å.slut().to_string()))
                    .collect(),
                momsperioder: förutsättningar
                    .momsperioder
                    .iter()
                    .map(|(d, p)| (d.to_string(), p.kod().to_string()))
                    .collect(),
                nästa_nummer: förutsättningar.nästa_nummer,
                startnummer: förutsättningar.startnummer,
                intäktskonton: konton
                    .iter()
                    .filter(|k| k.aktiv && k.nummer.klass() == 3)
                    .map(|k| (k.nummer.get(), k.namn.clone()))
                    .collect(),
                idag: b.klocka.idag().to_string(),
            });
        }
        Ok(data)
    }

    fn granskning(
        formulär: &NyKundfaktura,
        f: &Förutsättningar,
        kontonamn: &dyn Fn(u16) -> String,
    ) -> GranskningVy {
        match formulär.kontrollera(f) {
            Ok(u) => GranskningVy {
                nummer: u.nummer.to_string(),
                fakturadatum: u.fakturadatum.to_string(),
                kontering: u
                    .kontering()
                    .iter()
                    .map(|r| {
                        (
                            r.konto.to_string(),
                            kontonamn(r.konto.get()),
                            r.debet.eller_tomt(),
                            r.kredit.eller_tomt(),
                        )
                    })
                    .collect(),
                total: kr(u.total),
                spärrar: Vec::new(),
                skicka_till: String::new(),
            },
            Err(spärrar) => GranskningVy {
                spärrar: spärrar.into_iter().map(|s| s.text).collect(),
                ..GranskningVy::default()
            },
        }
    }

    /// Märket för en utfärdad faktura i listorna: kreditfaktura, eller statusen efter
    /// krediteringarna och betalningarna (#140) och en gällande kundförlust (#141).
    pub(crate) fn märke_för(
        f: &kundfaktura::Kundfakturarad,
        förfallodag: Date,
        idag: Date,
    ) -> maya_domain::reskontra::Märke {
        match f.krediterar {
            Some(_) => maya_domain::reskontra::Märke::Kreditfaktura,
            None => maya_domain::reskontra::kundfakturans_märke(
                f.total,
                f.krediterat,
                f.återstår,
                f.kundförlust,
                förfallodag,
                idag,
            ),
        }
    }

    /// Panelen Kreditfaktura… med originalets rader, förvald hela fakturan.
    fn kreditera_panelen(b: &Sidbegäran, u: &kundfaktura::Utfärdning, kvar: i64) -> KrediteraVy {
        let tillbaka = b.värde("fel").as_deref() == Some("kreditering");
        KrediteraVy {
            rader: u
                .rader
                .iter()
                .map(|r| {
                    (
                        r.beskrivning.clone(),
                        format!("{} {}", r.antal, r.enhet).trim().to_string(),
                        kr(r.netto),
                    )
                })
                .collect(),
            kvar: kr(kvar),
            start: if tillbaka {
                kreditering_ur(|namn| b.värde(namn), u.rader.len())
            } else {
                NyKreditering {
                    hela: true,
                    antal: u.rader.iter().map(|r| r.antal.to_string()).collect(),
                }
            },
            fel: tillbaka.then(|| {
                b.värde("text")
                    .unwrap_or_else(|| feltext("kreditering").unwrap_or_default().into())
            }),
        }
    }

    fn dokument(
        företag: FöretagsId,
        f: &kundfaktura::Kundfaktura,
        u: &kundfaktura::Utfärdning,
    ) -> FakturaVy {
        FakturaVy {
            krediterar: u
                .krediterar
                .as_ref()
                .map(|h| (h.faktura.to_string(), h.text())),
            krediteras_av: f
                .krediteras_av
                .iter()
                .map(|k| (k.id.to_string(), k.nummer.to_string(), kr(k.total)))
                .collect(),
            id: f.id.to_string(),
            nummer: u.nummer.to_string(),
            referens: (u.referens.clone(), u.säljare.ocr),
            kund: u.kund.clone(),
            fakturadatum: u.fakturadatum.to_string(),
            förfallodag: u.förfallodag.to_string(),
            leveransdatum: u.leveransdatum.clone(),
            er_referens: u.er_referens.clone(),
            rader: u
                .rader
                .iter()
                .map(|r| FakturaradVy {
                    beskrivning: r.beskrivning.clone(),
                    antal: if r.enhet.is_empty() {
                        r.antal.to_string()
                    } else {
                        format!("{} {}", r.antal, r.enhet)
                    },
                    à_pris: kr(r.à_pris),
                    rabatt: if r.rabatt == Rabatt::INGEN {
                        String::new()
                    } else {
                        format!("{} %", r.rabatt)
                    },
                    moms: r.momssats.to_string(),
                    konto: r.konto.to_string(),
                    netto: kr(r.netto),
                })
                .collect(),
            momsrader: u
                .momsrader
                .iter()
                .map(|m| (format!("Moms {} på {}", m.sats, kr(m.underlag)), kr(m.moms)))
                .collect(),
            netto: kr(u.netto),
            moms: kr(u.moms),
            total: kr(u.total),
            omvänd: u
                .momsläge
                .fakturatext()
                .map(|text| (u.momsläge.omvänd().namn().to_string(), text.to_string())),
            utan_moms: u.momsläge == Momsläge::UtanMoms,
            verifikat: (
                u.verifikat.id.to_string(),
                u.verifikat
                    .nummer
                    .map(|n| n.to_string())
                    .unwrap_or_default(),
            ),
            pdf: PdfVy {
                href: format!("/foretag/{företag}/underlag/{}/fil", u.pdf.underlag),
                filnamn: u.pdf.filnamn.clone(),
                arkiverad: tid(u.pdf.arkiverad),
                sha256: u.pdf.sha256.clone(),
                storlek: maya_domain::underlag::storlek(u.pdf.storlek),
            },
            utfärdad: (
                tid(u.utfärdad.tid),
                u.utfärdad.namn.clone(),
                u.utfärdad.aktör.kanal.namn().to_string(),
            ),
            historik: f
                .historik
                .iter()
                .map(|h| (tid(h.post.tid), h.post.typ.clone(), h.namn.clone()))
                .collect(),
            ..FakturaVy::default()
        }
    }

    type Fält = Vec<(String, String)>;

    fn värde(fält: &Fält, namn: &str) -> Option<String> {
        fält.iter().find(|(k, _)| k == namn).map(|(_, v)| v.clone())
    }

    /// Den inloggade och företaget i sökvägen, eller svaret om något saknas.
    pub(crate) fn tolka(
        id: &str,
        inloggad: Option<Extension<Inloggad>>,
    ) -> Result<(Inloggad, FöretagsId), Box<Response>> {
        let Some(Extension(inloggad)) = inloggad else {
            return Err(Box::new(Redirect::to("/").into_response()));
        };
        let Ok(företag) = id.parse::<FöretagsId>() else {
            return Err(Box::new(ingen_åtkomst()));
        };
        Ok((inloggad, företag))
    }

    /// Tillbaka till formuläret på `sida` med det som användaren skrev och felet `kod`.
    fn tillbaka(sida: &str, fält: &Fält, kod: &str) -> Response {
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        query.append_pair("form", "kundfaktura");
        query.append_pair("fel", kod);
        for (namn, v) in fält.iter().filter(|(namn, _)| namn != "handling") {
            query.append_pair(namn, v);
        }
        let skiljetecken = if sida.contains('?') { '&' } else { '?' };
        Redirect::to(&format!("{sida}{skiljetecken}{}", query.finish())).into_response()
    }

    async fn skapa(
        State(state): State<Arc<KundfakturaState>>,
        Path(id): Path<String>,
        inloggad: Option<Extension<Inloggad>>,
        Form(fält): Form<Fält>,
    ) -> Response {
        let (inloggad, företag) = match tolka(&id, inloggad) {
            Ok(t) => t,
            Err(r) => return *r,
        };
        spara_i(&state, &inloggad, företag, None, &fält).await
    }

    async fn spara(
        State(state): State<Arc<KundfakturaState>>,
        Path((id, f)): Path<(String, String)>,
        inloggad: Option<Extension<Inloggad>>,
        Form(fält): Form<Fält>,
    ) -> Response {
        let (inloggad, företag) = match tolka(&id, inloggad) {
            Ok(t) => t,
            Err(r) => return *r,
        };
        let Ok(f) = f.parse::<KundfakturaId>() else {
            return finns_inte();
        };
        spara_i(&state, &inloggad, företag, Some(f), &fält).await
    }

    async fn spara_i(
        state: &KundfakturaState,
        inloggad: &Inloggad,
        företag: FöretagsId,
        utkast: Option<KundfakturaId>,
        fält: &Fält,
    ) -> Response {
        let formulär = ur_fält(|namn| värde(fält, namn));
        let lista = format!("/foretag/{företag}/kundfakturor");
        let formulärets_sida = match utkast {
            Some(f) => format!("{lista}?f={f}"),
            None => format!("{lista}/ny"),
        };
        match kundfaktura::spara_utkast(
            &state.db,
            &state.klocka,
            &inloggad.aktör,
            företag,
            utkast,
            &formulär,
        )
        .await
        {
            Ok(f) if värde(fält, "handling").as_deref() == Some("granska") => {
                Redirect::to(&format!("{lista}?f={f}&granska=1")).into_response()
            }
            Ok(f) => Redirect::to(&format!("{lista}?f={f}&sparat=1")).into_response(),
            Err(KundfakturaFel::Spärrat(_)) => tillbaka(&formulärets_sida, fält, "utkast"),
            Err(KundfakturaFel::StartnummerLåst) => {
                tillbaka(&formulärets_sida, fält, "startnummer")
            }
            Err(KundfakturaFel::IngetUtkast) => {
                Redirect::to(&format!("{lista}?fel=finns_inte")).into_response()
            }
            Err(e) => fakturafel(e),
        }
    }

    async fn utfärda(
        State(state): State<Arc<KundfakturaState>>,
        Path((id, f)): Path<(String, String)>,
        inloggad: Option<Extension<Inloggad>>,
        Form(fält): Form<Fält>,
    ) -> Response {
        let (inloggad, företag) = match tolka(&id, inloggad) {
            Ok(t) => t,
            Err(r) => return *r,
        };
        let Ok(f) = f.parse::<KundfakturaId>() else {
            return finns_inte();
        };
        let lista = format!("/foretag/{företag}/kundfakturor");
        // #138: skickas efter att utfärdandet har committats, när rutan är kryssad.
        let utfärdat = match (&state.epost, värde(&fält, "skicka").as_deref()) {
            (Some(epost), Some("1")) => {
                maya_core::utskick::utfärda_och_skicka(
                    &state.db,
                    &state.klocka,
                    &inloggad.aktör,
                    företag,
                    f,
                    epost,
                )
                .await
            }
            _ => kundfaktura::utfärda(&state.db, &state.klocka, &inloggad.aktör, företag, f)
                .await
                .map(|u| (u, None)),
        };
        match utfärdat {
            Ok((u, utfall)) => {
                let tillägg = match utfall {
                    Some(maya_core::utskick::Utfall::Skickat) => "&skickat=1",
                    Some(maya_core::utskick::Utfall::Misslyckat(_)) => "&fel=skickfel",
                    None => "",
                };
                Redirect::to(&format!("{lista}?f={}&utfardad=1{tillägg}", u.id)).into_response()
            }
            Err(KundfakturaFel::Spärrat(_)) => {
                Redirect::to(&format!("{lista}?f={f}&granska=1&fel=sparrat")).into_response()
            }
            Err(KundfakturaFel::Kontantmetoden) => {
                Redirect::to(&format!("{lista}?f={f}&fel=kontant")).into_response()
            }
            Err(KundfakturaFel::IngetUtkast) => {
                Redirect::to(&format!("{lista}?f={f}&fel=finns_inte")).into_response()
            }
            Err(e) => fakturafel(e),
        }
    }

    async fn radera(
        State(state): State<Arc<KundfakturaState>>,
        Path((id, f)): Path<(String, String)>,
        inloggad: Option<Extension<Inloggad>>,
    ) -> Response {
        let (inloggad, företag) = match tolka(&id, inloggad) {
            Ok(t) => t,
            Err(r) => return *r,
        };
        let Ok(f) = f.parse::<KundfakturaId>() else {
            return finns_inte();
        };
        let lista = format!("/foretag/{företag}/kundfakturor");
        match kundfaktura::radera_utkast(&state.db, &state.klocka, &inloggad.aktör, företag, f)
            .await
        {
            Ok(()) => Redirect::to(&format!("{lista}?raderad=1")).into_response(),
            Err(KundfakturaFel::IngetUtkast) => {
                Redirect::to(&format!("{lista}?fel=finns_inte")).into_response()
            }
            Err(e) => fakturafel(e),
        }
    }

    async fn förhandsvisning(
        State(state): State<Arc<KundfakturaState>>,
        Path((id, f)): Path<(String, String)>,
        inloggad: Option<Extension<Inloggad>>,
    ) -> Response {
        let (inloggad, företag) = match tolka(&id, inloggad) {
            Ok(t) => t,
            Err(r) => return *r,
        };
        let Ok(f) = f.parse::<KundfakturaId>() else {
            return finns_inte();
        };
        match kundfaktura::förhandsvisning(&state.db, &state.klocka, &inloggad.aktör, företag, f)
            .await
        {
            Ok(p) => pdf(p),
            Err(KundfakturaFel::Spärrat(_)) => text(
                StatusCode::CONFLICT,
                "Fakturan har spärrar kvar och kan inte förhandsvisas som PDF.\n",
            ),
            Err(e) => fakturafel(e),
        }
    }

    /// `POST /foretag/{id}/kundfakturor/{f}/kreditera`: skapar kreditfakturans utkast och visar
    /// det, eller panelen igen med spärrarna (#140).
    async fn kreditera(
        State(state): State<Arc<KundfakturaState>>,
        Path((id, f)): Path<(String, String)>,
        inloggad: Option<Extension<Inloggad>>,
        Form(fält): Form<Fält>,
    ) -> Response {
        let (inloggad, företag) = match tolka(&id, inloggad) {
            Ok(t) => t,
            Err(r) => return *r,
        };
        let Ok(f) = f.parse::<KundfakturaId>() else {
            return finns_inte();
        };
        let lista = format!("/foretag/{företag}/kundfakturor");
        let antal = (0..500)
            .take_while(|i| värde(&fält, &format!("antal_{i}")).is_some())
            .count();
        let kreditering = kreditering_ur(|namn| värde(&fält, namn), antal);
        match kundfaktura::kreditera(
            &state.db,
            &state.klocka,
            &inloggad.aktör,
            företag,
            f,
            &kreditering,
        )
        .await
        {
            Ok(k) => Redirect::to(&format!("{lista}?f={k}&kreditutkast=1")).into_response(),
            Err(KundfakturaFel::Kreditering(spärrar)) => {
                let mut query = url::form_urlencoded::Serializer::new(String::new());
                query.append_pair("f", &f.to_string());
                query.append_pair("panel", Panel::Kreditera.kod());
                query.append_pair("fel", "kreditering");
                let text: Vec<String> = spärrar.into_iter().map(|s| s.text).collect();
                query.append_pair("text", &text.join(" "));
                for (namn, v) in &fält {
                    query.append_pair(namn, v);
                }
                Redirect::to(&format!("{lista}?{}", query.finish())).into_response()
            }
            Err(e) => fakturafel(e),
        }
    }

    pub(crate) fn fakturafel(e: KundfakturaFel) -> Response {
        match e {
            KundfakturaFel::FinnsInte | KundfakturaFel::IngetUtkast => finns_inte(),
            KundfakturaFel::Åtkomst(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte) => {
                ingen_åtkomst()
            }
            e => serverfel(e),
        }
    }

    pub(crate) fn finns_inte() -> Response {
        text(StatusCode::NOT_FOUND, "Kundfakturan finns inte.\n")
    }
}

/// Valet i panelen Kreditfaktura… ur fälten: `hela` och `antal_<rad>` för `rader` rader.
#[cfg(feature = "ssr")]
fn kreditering_ur(värde: impl Fn(&str) -> Option<String>, rader: usize) -> NyKreditering {
    NyKreditering {
        hela: värde("hela").is_some_and(|v| !v.is_empty()),
        antal: (0..rader)
            .map(|i| värde(&format!("antal_{i}")).unwrap_or_default())
            .collect(),
    }
}

/// Kundfakturor: listan och den valda fakturan.
#[component]
pub fn KundfakturorSida() -> impl IntoView {
    sida(hämta_kundfakturadata, |data| {
        kundfakturor_sida(data, false)
    })
}

/// Kundfakturor med formuläret för en ny kundfaktura.
#[component]
pub fn NyKundfakturaSida() -> impl IntoView {
    sida(hämta_kundfakturadata, |data| kundfakturor_sida(data, true))
}

/// Kundfakturor med uppladdningen av en ny extern kundfaktura (#143).
#[component]
pub fn NyExternKundfakturaSida() -> impl IntoView {
    sida(hämta_kundfakturadata, |data| {
        kundfakturor_sida(data, false)
    })
}

fn kundfakturor_sida(data: Kundfakturadata, ny: bool) -> AnyView {
    if !inloggad() {
        return view! { <LoggaIn /> }.into_any();
    }
    let Some(bas) = företagets_bas() else {
        return view! { <IngenÅtkomst /> }.into_any();
    };
    let fråga = |namn: &str| use_query_map().with_untracked(|q| q.get(namn));
    let notis = [
        ("sparat", "Sparat."),
        (
            "utfardad",
            "Fakturan utfärdades, PDF:en arkiverades och verifikatet bokfördes.",
        ),
        ("raderad", "Utkastet raderades och raderingen loggades."),
        (
            "rattad",
            "Konteringen rättades: utfärdandets verifikat vändes och ett nytt bokfördes.",
        ),
        // #138 Utskick
        (
            "skickat",
            "Fakturan skickades med e-post, och utskicket loggades.",
        ),
        ("lamnad", "Fakturan markerades som lämnad på annat sätt."),
        // #139 Betalningar
        (
            "betald",
            "Betalningen registrerades och verifikatet bokfördes.",
        ),
        (
            "betalning_rattad",
            "Betalningen rättades. Rättelsen bokfördes och betalningen räknas inte längre.",
        ),
        // #141 Kundförlust
        (
            "kundforlust_aterkallad",
            "Kundförlusten återkallades med ett nytt verifikat, och fakturan kan betalas igen.",
        ),
        (
            "kundforlust",
            "Kundförlusten bokfördes, och utgående moms minskades.",
        ),
        // #140 Kreditfakturor
        (
            "kreditutkast",
            "Kreditfakturans utkast skapades. Granska och utfärda det.",
        ),
        (
            "aterbetald",
            "Återbetalningen registrerades och verifikatet bokfördes.",
        ),
        // #143 Externa kundfakturor
        (
            "uppladdat",
            "Ett utkast skapades med PDF:en kopplad. Skriv av fakturan.",
        ),
        (
            "extern_kreditutkast",
            "Kreditfakturans utkast skapades med PDF:en kopplad. Skriv av den med negativa belopp.",
        ),
    ]
    .into_iter()
    .find(|(namn, _)| fråga(namn).is_some())
    .map(|(_, text)| text.to_string())
    // #143: registreringen och Rätta registrering säger vilket verifikat det blev.
    .or_else(|| {
        fråga("registrerad").map(|v| format!("Fakturan registrerades och bokfördes som {v}."))
    })
    .or_else(|| {
        fråga("registrering_rattad").map(|v| {
            if v == "1" {
                "Registreringen rättades. Fakturan är upplåst; skriv av den och registrera om."
                    .to_string()
            } else {
                format!(
                    "Registreringen rättades med {v}, och fakturan är upplåst; skriv av den och \
                     registrera om."
                )
            }
        })
    })
    .map(|text| view! { <p class="notis" role="status">{text}</p> });
    let fel = fråga("fel")
        .and_then(|k| feltext(&k))
        .map(|text| view! { <p class="fel" role="alert">{text}</p> });
    let lista = kundfakturorna(&bas);
    let tillbakaskickat = fråga("form").as_deref() == Some("kundfaktura");
    let från_adressen = || ur_fält(|namn| fråga(namn));
    let detalj = match (&data.utkast, &data.dokument, &data.förutsättningar) {
        // #143 Externa kundfakturor
        _ if data.extern_ny => {
            view! { <crate::externa_kundfakturor::NyExternFakturaFormulär bas=bas.clone() /> }
                .into_any()
        }
        _ if data.extern_faktura.is_some() => {
            let d = data.extern_faktura.clone().unwrap_or_default();
            view! { <crate::externa_kundfakturor::ExternFaktura bas=bas.clone() d=d /> }.into_any()
        }
        (_, _, Some(f)) if ny => {
            let start = if tillbakaskickat {
                från_adressen()
            } else {
                NyKundfaktura {
                    fakturadatum: f.idag.clone(),
                    ..NyKundfaktura::default()
                }
            };
            view! { <FakturaFormulär bas=bas.clone() f=f.clone() start=start utkast=None /> }
                .into_any()
        }
        (Some(u), _, Some(_)) if fråga("radera").is_some() => {
            view! { <RaderaUtkast lista=lista.clone() u=u.clone() /> }.into_any()
        }
        (Some(u), _, Some(f)) => {
            let start = if tillbakaskickat {
                // Hänvisningen skickas inte med formuläret (#140).
                NyKundfaktura {
                    krediterar: u.formulär.krediterar.clone(),
                    ..från_adressen()
                }
            } else {
                u.formulär.clone()
            };
            let granska = data.granskning.clone().map(|g| {
                view! { <Granska lista=lista.clone() id=u.id.clone() g=g /> }
            });
            view! {
                {granska}
                <FakturaFormulär bas=bas.clone() f=f.clone() start=start utkast=Some(u.clone()) />
            }
            .into_any()
        }
        (_, Some(d), _) => view! { <Fakturadokument bas=bas.clone() d=d.clone() /> }.into_any(),
        _ => view! {
            <div class="tom">
                <p>"Välj en faktura i listan."</p>
                <a class="knapp" href=format!("{lista}/ny")>
                    "Ny kundfaktura"
                </a>
            </div>
        }
        .into_any(),
    };
    let valt = data.valt.clone();
    let rad = |f: &ListVy| {
        let aktuellt = (Some(&f.id) == valt.as_ref()).then_some("page");
        let märke = match (&f.märke, f.utkast) {
            (_, true) => view! { <UtkastBadge /> }.into_any(),
            (Some((kod, namn)), _) => {
                view! { <Märke kod=kod.clone() namn=namn.clone() /> }.into_any()
            }
            (None, _) => ().into_any(),
        };
        let nummer = (!f.nummer.is_empty())
            .then(|| view! { <span class="diskret">{f.nummer.clone()}</span> });
        let kund = if f.kund.is_empty() {
            "Utan kund".to_string()
        } else {
            f.kund.clone()
        };
        // #143
        let extern_märke = f.extern_faktura.then(|| {
            view! {
                " "
                <Märke kod="extern".to_string() namn="Extern".to_string() />
            }
        });
        // #137
        let ny = f.ny_kund.then(|| {
            view! {
                " "
                <span class="badge badge-ny" title="Läggs i kundregistret när fakturan utfärdas">
                    "Ny kund"
                </span>
            }
        });
        let datum = match (f.förfallotext.is_empty(), f.förfallodag.is_empty()) {
            (false, _) => format!("{} · {}", f.fakturadatum, f.förfallotext),
            (true, false) => format!("{} · förfaller {}", f.fakturadatum, f.förfallodag),
            (true, true) => f.fakturadatum.clone(),
        };
        // Det som återstår för en öppen faktura, annars totalen.
        let belopp = if f.förfallotext.is_empty() {
            f.total.clone()
        } else {
            f.återstår.clone()
        };
        view! {
            <li>
                <a href=format!("{lista}?f={}", f.id) aria-current=aktuellt>
                    <span class="verifikatlista-topp">
                        <span>{nummer} " " {kund} {ny}</span>
                        <span class="belopp">{belopp}</span>
                    </span>
                    <span class="diskret">{märke} {extern_märke} " " {datum}</span>
                </a>
            </li>
        }
    };
    // #139: grupperad som Förfallna, Förfaller inom 7 dagar, Senare, Utkast och Avslutade.
    let rader = Grupp::ALLA
        .iter()
        .filter_map(|g| {
            let mut i_gruppen: Vec<&ListVy> =
                data.lista.iter().filter(|f| f.grupp == Some(*g)).collect();
            if i_gruppen.is_empty() {
                return None;
            }
            if matches!(g, Grupp::Förfallna | Grupp::InomSjuDagar | Grupp::Senare) {
                i_gruppen.sort_by(|a, b| a.förfallodag.cmp(&b.förfallodag));
            }
            let antal = i_gruppen.len();
            let rader = i_gruppen.into_iter().map(rad).collect_view();
            Some(view! {
                <li class="lista-grupp">
                    <h2 class="per">{g.rubrik()} <span class="diskret">{format!(" · {antal}")}</span></h2>
                    <ul class="verifikatlista">{rader}</ul>
                </li>
            })
        })
        .collect_view();
    let tom = data
        .lista
        .is_empty()
        .then(|| view! { <p class="diskret">"Inga kundfakturor än."</p> });
    view! {
        <div class="verifikat-delad kundfakturor">
            <div class="verifikat-spalt">
                <div class="verifikat-spalt-rubrik">
                    <div>
                        <h1>"Kundfakturor"</h1>
                        <p class="diskret kundfordringar">
                            {format!("Kundfordringar {} kr", data.kundfordringar)}
                        </p>
                    </div>
                    <a class="knapp" href=format!("{lista}/ny")>
                        "Ny kundfaktura"
                    </a>
                </div>
                <nav class="filter" aria-label="Kundfakturor">
                    <a href=kundregistret(&bas)>"Kunder"</a>
                </nav>
                {tom}
                <ul class="listgrupper">{rader}</ul>
            </div>
            <div class="verifikat-detalj">{notis} {fel} {detalj}</div>
        </div>
    }
    .into_any()
}

/// Frågan innan ett utkast raderas.
#[component]
fn RaderaUtkast(lista: String, u: UtkastVy) -> impl IntoView {
    view! {
        <section class="dialog" role="alertdialog" aria-labelledby="radera-rubrik">
            <h2 id="radera-rubrik">"Radera utkastet?"</h2>
            <p class="diskret">
                "Utkastet raderas, och raderingen loggas med vem, när och en sammanfattning."
            </p>
            <form method="post" action=format!("{lista}/{}/radera", u.id)>
                <a class="knapp" href=format!("{lista}?f={}", u.id)>
                    "Avbryt"
                </a>
                " "
                <button class="knapp knapp-fara" type="submit">
                    "Radera utkastet"
                </button>
            </form>
        </section>
    }
}

/// Granska och utfärda: hela PDF:en, konteringen och numret, och Utfärda.
#[component]
fn Granska(lista: String, id: String, g: GranskningVy) -> impl IntoView {
    let här = format!("{lista}?f={id}");
    let spärrat = !g.spärrar.is_empty();
    let innehåll = if spärrat {
        let spärrar = g
            .spärrar
            .iter()
            .map(|s| view! { <li>{s.clone()}</li> })
            .collect_view();
        view! {
            <p>"Fakturan kan inte utfärdas än. Det här återstår:"</p>
            <ul class="fel">{spärrar}</ul>
        }
        .into_any()
    } else {
        let rader = g
            .kontering
            .iter()
            .map(|(konto, namn, debet, kredit)| {
                view! {
                    <tr>
                        <td>{konto.clone()}</td>
                        <td>{namn.clone()}</td>
                        <td class="belopp">{debet.clone()}</td>
                        <td class="belopp">{kredit.clone()}</td>
                    </tr>
                }
            })
            .collect_view();
        view! {
            <p>
                "Så här ser PDF:en ut. När du utfärdar får fakturan nummer "
                <strong>{g.nummer.clone()}</strong>
                ", låses och arkiveras. Därefter ändras den bara med kreditfaktura."
            </p>
            <iframe
                class="granska-pdf"
                title="Fakturans PDF"
                src=format!("{lista}/{id}/forhandsvisning")
            ></iframe>
            <p class="diskret">
                <a href=format!("{lista}/{id}/forhandsvisning") target="_blank" rel="external noopener">
                    "Öppna PDF:en"
                </a>
            </p>
            <div class="kont">
                <h3>"Bokförs som verifikat · " {g.fakturadatum.clone()}</h3>
                <table class="rader">
                    <thead>
                        <tr>
                            <th>"Konto"</th>
                            <th>"Kontonamn"</th>
                            <th class="belopp">"Debet"</th>
                            <th class="belopp">"Kredit"</th>
                        </tr>
                    </thead>
                    <tbody>{rader}</tbody>
                </table>
            </div>
            <p>"Att betala " <strong>{g.total.clone()} " kr"</strong></p>
        }
        .into_any()
    };
    view! {
        <section class="dialog granska" role="dialog" aria-labelledby="granska-rubrik">
            <h2 id="granska-rubrik">"Granska och utfärda"</h2>
            {innehåll}
            <form method="post" action=format!("{lista}/{id}/utfarda")>
                {(!spärrat && !g.skicka_till.is_empty()).then(|| view! {
                    <p>
                        <label>
                            <input type="checkbox" name="skicka" value="1" />
                            " Skicka till " {g.skicka_till.clone()} " när den utfärdats"
                        </label>
                    </p>
                })}
                <a class="knapp" href=här>
                    "Tillbaka och ändra"
                </a>
                " "
                <button class="knapp knapp-primar" type="submit" disabled=spärrat>
                    "Utfärda"
                </button>
            </form>
        </section>
    }
}

/// Förvald förfallodag: fakturadatumet plus kundens betalningsvillkor.
fn förvald_förfallodag(f: &NyKundfaktura, förutsättningar: &Förutsättningar) -> String {
    let kund = f.kunden(förutsättningar);
    match (f.fakturadatum.trim().parse::<Date>(), kund) {
        (Ok(d), Some(k)) => d
            .checked_add(jiff::Span::new().days(i64::from(k.betalningsvillkor)))
            .map(|d| d.to_string())
            .unwrap_or_default(),
        _ => String::new(),
    }
}

/// Formuläret för ett nytt eller sparat utkast: kund, datum, rader, spärrarna vid fälten,
/// summan, "Bokförs som", förhandsvisningen och knapparna.
#[component]
fn FakturaFormulär(
    bas: String,
    f: FörutsättningarVy,
    start: NyKundfaktura,
    utkast: Option<UtkastVy>,
) -> impl IntoView {
    let lista = kundfakturorna(&bas);
    let mut start = start;
    // En tom rad sist, så att det går att lägga till en rad också utan JavaScript.
    let förutsättningar = f.domän();
    if start.rader.last().is_none_or(|r| !r.tom()) {
        start.rader.push(NyFakturarad::default());
    }
    // Tomma rader får momslägets förval.
    let läge_från_start = start.momsläge(&förutsättningar);
    for r in start.rader.iter_mut().filter(|r| r.tom()) {
        *r = NyFakturarad::för(läge_från_start);
    }
    let faktura = RwSignal::new(start.clone());
    let spärrar = {
        let förutsättningar = förutsättningar.clone();
        Memo::new(move |_| {
            faktura.with(|f| f.kontrollera(&förutsättningar).err().unwrap_or_default())
        })
    };
    let utfärdbar = {
        let förutsättningar = förutsättningar.clone();
        Memo::new(move |_| faktura.with(|f| f.kontrollera(&förutsättningar).ok()))
    };
    let läge = {
        let förutsättningar = förutsättningar.clone();
        Memo::new(move |_| faktura.with(|f| f.momsläge(&förutsättningar)))
    };
    let beräkning = Memo::new(move |_| faktura.with(|f| f.beräkna_med(läge.get())));
    // Ändringar som kan byta momsläge (fakturadatumet, kunden, den omvända
    // skattskyldigheten) anpassar raderna till det nya läget.
    let förutsättningar_kvar = StoredValue::new(förutsättningar.clone());
    let ändra = move |ändring: &dyn Fn(&mut NyKundfaktura)| {
        förutsättningar_kvar.with_value(|ff| {
            faktura.update(|f| {
                let före = f.momsläge(ff);
                ändring(f);
                let efter = f.momsläge(ff);
                if före != efter {
                    f.byt_momsläge(före, efter);
                }
            })
        })
    };
    let hydrerad = RwSignal::new(false);
    Effect::new(move |_| hydrerad.set(true));
    let visa = RwSignal::new(use_query_map().with_untracked(|q| q.get("visa").is_some()));
    #[cfg(feature = "hydrate")]
    {
        let lyssnare = window_event_listener(leptos::ev::keydown, move |ev| {
            use wasm_bindgen::JsCast;
            let i_fält = ev
                .target()
                .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                .is_some_and(|e| matches!(e.tag_name().as_str(), "INPUT" | "TEXTAREA" | "SELECT"));
            if !i_fält && !ev.meta_key() && !ev.ctrl_key() && ev.key().eq_ignore_ascii_case("p") {
                ev.prevent_default();
                visa.update(|v| *v = !*v);
            }
        });
        on_cleanup(move || lyssnare.remove());
    }

    let spärr_vid = {
        let bas = bas.clone();
        move |fält: Fält| {
            let bas = bas.clone();
            move || {
                spärrar.with(|s| {
                    s.iter().find(|s| s.fält == fält).map(|s: &Spärr| {
                        let aktivera = (s.kod == "konto_inaktivt").then(|| {
                            let Fält::Rad(i, _) = fält else {
                                unreachable!()
                            };
                            let konto = faktura.with_untracked(|f| f.rader[i].konto.clone());
                            view! {
                                " "
                                <a href=format!(
                                    "{bas}/installningar/kontoplan?sok={}",
                                    konto.trim(),
                                )>"Aktivera"</a>
                            }
                        });
                        view! {
                            <p class="fel" role="alert" data-kod=s.kod>
                                {s.text.clone()}
                                {aktivera}
                            </p>
                        }
                    })
                })
            }
        }
    };
    let företagets = {
        let bas = bas.clone();
        move || {
            let fel = spärrar.with(|s| {
                s.iter()
                    .filter(|s| s.fält == Fält::Företag)
                    .map(|s| {
                        view! {
                            <p class="fel" role="alert" data-kod=s.kod>
                                {s.text.clone()}
                            </p>
                        }
                    })
                    .collect::<Vec<_>>()
            });
            (!fel.is_empty()).then(|| {
                view! {
                    <div class="foretagets-fel">
                        {fel}
                        <p>
                            <a href=format!("{bas}/installningar")>"Öppna Inställningar → Företag"</a>
                        </p>
                    </div>
                }
            })
        }
    };
    let granska_text = move || match spärrar.with(Vec::len) {
        0 => "Granska och utfärda".to_string(),
        1 => "Granska och utfärda · 1 problem kvar".to_string(),
        n => format!("Granska och utfärda · {n} problem kvar"),
    };
    let granska = move |ev: leptos::ev::MouseEvent| {
        if !spärrar.with_untracked(Vec::is_empty) {
            ev.prevent_default();
        }
    };
    let värde = move |f: fn(&NyKundfaktura) -> String| faktura.with_untracked(f);

    let kunder = f.kunder.clone();
    let vald_kund = start.kund.clone();
    let ny_vald = Memo::new(move |_| faktura.with(|f| f.kund.trim() == NY));
    let nya_fält = NYA_KUNDFÄLT
        .into_iter()
        .map(|(namn, etikett, fält)| {
            let fel = spärr_vid(Fält::NyKund(fält));
            let värde = faktura.with_untracked(|f| ny_kunds(&mut f.ny_kund.clone(), namn).clone());
            let sätt = move |ev: leptos::ev::Event| {
                faktura.update(|f| *ny_kunds(&mut f.ny_kund, namn) = event_target_value(&ev));
            };
            let fältet = if namn == "ny_adress" {
                view! { <textarea id=namn name=namn rows="3" on:input=sätt>{värde}</textarea> }
                    .into_any()
            } else {
                view! { <input id=namn name=namn type="text" value=värde on:input=sätt /> }
                    .into_any()
            };
            view! {
                <div>
                    <label for=namn>{etikett}</label>
                    {fältet}
                    {fel}
                </div>
            }
        })
        .collect_view();
    let kundval = kunder
        .iter()
        .map(|k| {
            let id = k.id.to_string();
            view! {
                <option value=id.clone() selected=id == vald_kund>
                    {k.namn.clone()}
                </option>
            }
        })
        .collect_view();
    let kundinfo = {
        let förutsättningar = förutsättningar.clone();
        move || {
            let kund = faktura.with(|f| {
                (f.kund.trim() != NY)
                    .then(|| f.kunden(&förutsättningar))
                    .flatten()
            });
            kund.map(|k| {
                let delar: Vec<String> = [k.identitet.clone(), k.epost.clone()]
                    .into_iter()
                    .filter(|d| !d.is_empty())
                    .chain([format!("{} dagar", k.betalningsvillkor)])
                    .collect();
                view! { <p class="diskret">{delar.join(" · ")}</p> }
            })
        }
    };
    let förfallo_platshållare = {
        let förutsättningar = förutsättningar.clone();
        move || faktura.with(|f| förvald_förfallodag(f, &förutsättningar))
    };
    let nummerfält = match f.nästa_nummer {
        None => view! {
            <div>
                <label for="startnummer">"Första fakturanummer"</label>
                <input
                    id="startnummer"
                    name="startnummer"
                    type="text"
                    inputmode="numeric"
                    placeholder=f.startnummer.to_string()
                    value=värde(|f| f.startnummer.clone())
                    on:input=move |ev| faktura.update(|f| f.startnummer = event_target_value(&ev))
                />
                <p class="diskret">
                    "Väljs en gång, innan företagets första faktura utfärdas. Standard är 1."
                </p>
                {spärr_vid(Fält::Startnummer)}
            </div>
        }
        .into_any(),
        Some(n) => view! {
            <div>
                <span class="etikett">"Fakturanummer"</span>
                <p>{format!("{n} vid utfärdande")}</p>
            </div>
        }
        .into_any(),
    };

    let antal_rader = Memo::new(move |_| faktura.with(|f| f.rader.len()));
    let lägg_till_rad =
        move |_| faktura.update(|f| f.rader.push(NyFakturarad::för(läge.get_untracked())));
    let rader = {
        let spärr_vid = spärr_vid.clone();
        move || {
            (0..antal_rader.get())
                .map(|i| {
                    let fält = move |namn: &'static str| {
                        faktura.with_untracked(|f| {
                            let mut r = f.rader[i].clone();
                            radens(&mut r, namn).clone()
                        })
                    };
                    let input = move |namn: &'static str, etikett: &'static str, klass: &'static str| {
                        view! {
                            <input
                                name=format!("{namn}_{i}")
                                type="text"
                                class=klass
                                aria-label=etikett
                                value=fält(namn)
                                on:input=move |ev| {
                                    faktura.update(|f| *radens(&mut f.rader[i], namn) = event_target_value(&ev))
                                }
                            />
                        }
                    };
                    // Utan fri momssats (omvänd skattskyldighet eller inte momsregistrerat)
                    // finns bara 0 %.
                    let satser = move || {
                        let fri = läge.get().fri_momssats();
                        let sats = faktura.with_untracked(|f| f.rader[i].momssats.clone());
                        Momssats::ALLA
                            .into_iter()
                            .filter(|s| fri || *s == Momssats::Noll)
                            .map(|s| {
                                let p = s.procent().to_string();
                                view! {
                                    <option value=p.clone() selected=p == sats>
                                        {s.to_string()}
                                    </option>
                                }
                            })
                            .collect_view()
                    };
                    let byt_sats = move |ev: leptos::ev::Event| {
                        let ny = event_target_value(&ev);
                        let läge = läge.get_untracked();
                        faktura.update(|f| {
                            let r = &mut f.rader[i];
                            // Kontot följer momssatsen så länge det är satsens konto.
                            let förra = Momssats::tolka(&r.momssats).map(|s| läge.intäktskonto(s).to_string());
                            if (förra.as_deref() == Some(r.konto.trim()) || r.konto.trim().is_empty())
                                && let Some(s) = Momssats::tolka(&ny)
                            {
                                r.konto = läge.intäktskonto(s).to_string();
                            }
                            r.momssats = ny;
                        });
                    };
                    let sats_värde = move || faktura.with(|f| f.rader[i].momssats.clone());
                    let konto = move || faktura.with(|f| f.rader[i].konto.clone());
                    let belopp = move || {
                        beräkning.with(|b| b.rader.get(i).copied().flatten().map(kr).unwrap_or_default())
                    };
                    view! {
                        <tr>
                            <td>
                                {input("beskrivning", "Beskrivning", "")}
                                {spärr_vid(Fält::Rad(i, Radfält::Beskrivning))}
                            </td>
                            <td>
                                {input("antal", "Antal", "belopp")}
                                {spärr_vid(Fält::Rad(i, Radfält::Antal))}
                            </td>
                            <td>{input("enhet", "Enhet", "")}</td>
                            <td>
                                {input("a_pris", "À-pris", "belopp")}
                                {spärr_vid(Fält::Rad(i, Radfält::Pris))}
                            </td>
                            <td>
                                {input("rabatt", "Rabatt i procent", "belopp")}
                                {spärr_vid(Fält::Rad(i, Radfält::Rabatt))}
                            </td>
                            <td>
                                <select
                                    name=format!("momssats_{i}")
                                    aria-label="Moms"
                                    prop:value=sats_värde
                                    on:change=byt_sats
                                >
                                    {satser}
                                </select>
                                {spärr_vid(Fält::Rad(i, Radfält::Momssats))}
                            </td>
                            <td>
                                <input
                                    name=format!("konto_{i}")
                                    type="text"
                                    inputmode="numeric"
                                    list="intaktskonton"
                                    aria-label="Konto"
                                    value=fält("konto")
                                    prop:value=konto
                                    on:input=move |ev| {
                                        faktura.update(|f| f.rader[i].konto = event_target_value(&ev))
                                    }
                                />
                                {spärr_vid(Fält::Rad(i, Radfält::Konto))}
                            </td>
                            <td class="belopp">{belopp}</td>
                        </tr>
                    }
                })
                .collect_view()
        }
    };
    let att_betala = if start.krediterar.is_some() {
        "Att få tillbaka"
    } else {
        "Att betala"
    };
    let summor = move || {
        let fri = läge.get().fri_momssats();
        beräkning.with(|b| {
            let moms = b
                .momsrader
                .iter()
                .filter(|_| fri)
                .map(|m| {
                    view! {
                        <tr>
                            <td colspan="7">{format!("Moms {} på {}", m.sats, kr(m.underlag))}</td>
                            <td class="belopp">{kr(m.moms)}</td>
                        </tr>
                    }
                })
                .collect_view();
            view! {
                <tr>
                    <td colspan="7">"Netto"</td>
                    <td class="belopp">{kr(b.netto)}</td>
                </tr>
                {moms}
                <tr class="summa">
                    <td colspan="7">{att_betala}</td>
                    <td class="belopp">{kr(b.total)}</td>
                </tr>
            }
        })
    };
    let bokförs_som = move || {
        utfärdbar.with(|u| {
            u.as_ref().map(|u| {
                let rader = u
                    .kontering()
                    .iter()
                    .map(|r| {
                        view! {
                            <tr>
                                <td>{r.konto.to_string()}</td>
                                <td class="belopp">{r.debet.eller_tomt()}</td>
                                <td class="belopp">{r.kredit.eller_tomt()}</td>
                            </tr>
                        }
                    })
                    .collect_view();
                let not =
                    momslägets_text(u.momsläge).map(|text| view! { <p class="diskret">{text}</p> });
                view! {
                    <div class="kont">
                        <h3>{format!("Bokförs som verifikat · {}", u.fakturadatum)}</h3>
                        <table class="rader">
                            <thead>
                                <tr>
                                    <th>"Konto"</th>
                                    <th class="belopp">"Debet"</th>
                                    <th class="belopp">"Kredit"</th>
                                </tr>
                            </thead>
                            <tbody>{rader}</tbody>
                        </table>
                        {not}
                    </div>
                }
            })
        })
    };
    let vald_omvänd = start.omvänd_skattskyldighet.clone();
    let omvända = OmvändSkattskyldighet::ALLA
        .into_iter()
        .map(|o| {
            view! {
                <option value=o.kod() selected=o.kod() == vald_omvänd.trim()>
                    {o.namn()}
                </option>
            }
        })
        .collect_view();
    let utan_moms = move || läge.get() == Momsläge::UtanMoms;
    let fast_momsläge = start.krediterar.is_some();
    let omvänd_fält = view! {
        <div>
            <label for="omvand_skattskyldighet">"Omvänd skattskyldighet"</label>
            <select
                id="omvand_skattskyldighet"
                name="omvand_skattskyldighet"
                disabled=move || utan_moms() || fast_momsläge
                prop:value=move || faktura.with(|f| f.omvänd_skattskyldighet.clone())
                on:change=move |ev| {
                    let ny = event_target_value(&ev);
                    ändra(&|f| f.omvänd_skattskyldighet = ny.clone());
                }
            >
                {omvända}
            </select>
            {spärr_vid(Fält::OmvändSkattskyldighet)}
            {move || {
                utan_moms()
                    .then(|| {
                        view! {
                            <p class="diskret">
                                "Företaget är inte momsregistrerat på fakturadatumet: fakturan får \
                                 ingen moms och inget momsreg-nr."
                            </p>
                        }
                    })
            }}
        </div>
    };
    let intäktskonton = f
        .intäktskonton
        .iter()
        .map(|(nr, namn)| view! { <option value=nr.to_string() label=namn.clone()></option> })
        .collect_view();
    // #140: ett kreditutkast har originalets kund och momsläge och en hänvisning.
    let kredit = start.krediterar.clone();
    let (action, rubrik, märke, radera) = match &utkast {
        Some(u) => (
            format!("{lista}/{}", u.id),
            if kredit.is_some() {
                "Kreditfaktura"
            } else {
                "Kundfaktura"
            },
            Some(view! {
                <p>
                    <UtkastBadge />
                    " "
                    <span class="diskret">"Skapat av " {u.skapad.1.clone()} " · " {u.skapad.0.clone()}</span>
                </p>
            }),
            Some(view! {
                <a class="knapp knapp-fara-diskret" href=format!("{lista}?f={}&radera=1", u.id)>
                    "Radera utkast"
                </a>
            }),
        ),
        None => (lista.clone(), "Ny kundfaktura", None, None),
    };
    let rubrik_namn = {
        let förutsättningar = förutsättningar.clone();
        move || {
            faktura.with(|f| match f.kund.trim() {
                NY if !f.ny_kund.namn.trim().is_empty() => f.ny_kund.namn.trim().to_string(),
                _ => f
                    .kunden(&förutsättningar)
                    .map_or_else(|| rubrik.to_string(), |k| k.namn),
            })
        }
    };
    let visa_länk = match &utkast {
        Some(u) => format!("{lista}?f={}&visa=1", u.id),
        None => format!("{lista}/ny?visa=1"),
    };
    let växla = move |ev: leptos::ev::MouseEvent| {
        ev.prevent_default();
        visa.update(|v| *v = !*v);
    };
    let förhandsvisning = {
        let säljare = f.säljare.clone();
        let förutsättningar = förutsättningar.clone();
        move || {
            visa.get().then(|| {
                view! {
                    <Papper
                        faktura=faktura
                        beräkning=beräkning
                        läge=läge
                        säljare=säljare.clone()
                        förutsättningar=förutsättningar.clone()
                    />
                }
            })
        }
    };
    view! {
        <div class="fakturaformular-ram" class:med-forhandsvisning=move || visa.get()>
            {förhandsvisning}
            <form
                id="kundfakturaformular"
                class="verifikatformular kundfakturaformular"
                method="post"
                action=action
                novalidate
            >
                <div class="formular-huvud">
                    <div>
                        <p class="dokument-nummer">{rubrik}</p>
                        <h2>{rubrik_namn}</h2>
                        {märke}
                    </div>
                    <a class="knapp knapp-diskret" href=visa_länk on:click=växla>
                        {move || if visa.get() { "Dölj förhandsvisning " } else { "Visa förhandsvisning " }}
                        <kbd>"P"</kbd>
                    </a>
                </div>
                {match kredit.clone() {
                    Some(h) => view! {
                        <div class="falt-rad">
                            <div class="bred">
                                <span class="etikett">"Kund"</span>
                                <input type="hidden" name="kund" value=h.kund.id.to_string() />
                                <p>{h.kund.namn.clone()}</p>
                                <p class="hanvisning">
                                    <a href=format!("{lista}?f={}", h.faktura)>{h.text()}</a>
                                </p>
                                {spärr_vid(Fält::Kund)}
                            </div>
                        </div>
                    }
                    .into_any(),
                    None => view! {
                        <div class="falt-rad">
                            <div class="bred">
                                <label for="kund">"Kund"</label>
                                <select
                                    id="kund"
                                    name="kund"
                                    on:change=move |ev| {
                                        let kund = event_target_value(&ev);
                                        förutsättningar_kvar
                                            .with_value(|ff| faktura.update(|f| f.välj_kund(&kund, ff)));
                                    }
                                >
                                    <option value="">"Välj kund…"</option>
                                    {kundval}
                                    <option value=NY selected=vald_kund.trim() == NY>
                                        "+ Ny kund…"
                                    </option>
                                </select>
                                {kundinfo}
                                {spärr_vid(Fält::Kund)}
                                <p class="diskret">
                                    <a href=format!("{}/ny", kundregistret(&bas))>"Lägg upp en kund i kundregistret"</a>
                                </p>
                            </div>
                        </div>
                    }
                    .into_any(),
                }}
                <div class="kort-streckat ny-kund" class:dold=move || !ny_vald.get()>
                    <b>"Ny kund"</b>
                    <div class="lf-rutnat">{nya_fält}</div>
                    <p class="diskret">
                        "Läggs i kundregistret när fakturan utfärdas. Raderas utkastet försvinner förslaget."
                    </p>
                </div>
                <div class="falt-rad">
                    <div>
                        <label for="fakturadatum">"Fakturadatum"</label>
                        <input
                            id="fakturadatum"
                            name="fakturadatum"
                            type="date"
                            value=värde(|f| f.fakturadatum.clone())
                            on:input=move |ev| {
                                let datum = event_target_value(&ev);
                                ändra(&|f| f.fakturadatum = datum.clone());
                            }
                        />
                        {spärr_vid(Fält::Fakturadatum)}
                    </div>
                    <div>
                        <label for="leveransdatum">"Leveransdatum eller period"</label>
                        <input
                            id="leveransdatum"
                            name="leveransdatum"
                            type="text"
                            placeholder="Fakturadatumet"
                            value=värde(|f| f.leveransdatum.clone())
                            on:input=move |ev| faktura.update(|f| f.leveransdatum = event_target_value(&ev))
                        />
                    </div>
                    <div>
                        <label for="forfallodag">"Förfallodag"</label>
                        <input
                            id="forfallodag"
                            name="forfallodag"
                            type="date"
                            value=värde(|f| f.förfallodag.clone())
                            on:input=move |ev| faktura.update(|f| f.förfallodag = event_target_value(&ev))
                        />
                        <p class="diskret">
                            {move || {
                                let förvald = förfallo_platshållare();
                                if förvald.is_empty() {
                                    "Fakturadatumet plus kundens villkor".to_string()
                                } else {
                                    format!("Tom: {förvald} enligt kunden")
                                }
                            }}
                        </p>
                        {spärr_vid(Fält::Förfallodag)}
                    </div>
                    <div>
                        <label for="er_referens">"Er referens"</label>
                        <input
                            id="er_referens"
                            name="er_referens"
                            type="text"
                            value=värde(|f| f.er_referens.clone())
                            on:input=move |ev| faktura.update(|f| f.er_referens = event_target_value(&ev))
                        />
                    </div>
                    {omvänd_fält}
                    {nummerfält}
                </div>
                <datalist id="intaktskonton">{intäktskonton}</datalist>
                <table class="rader fakturarader">
                    <thead>
                        <tr>
                            <th>"Beskrivning"</th>
                            <th class="belopp">"Antal"</th>
                            <th>"Enhet"</th>
                            <th class="belopp">"À-pris"</th>
                            <th class="belopp">"Rabatt %"</th>
                            <th>"Moms"</th>
                            <th>"Konto"</th>
                            <th class="belopp">"Belopp"</th>
                        </tr>
                    </thead>
                    <tbody>{rader}</tbody>
                    <tfoot>{summor}</tfoot>
                </table>
                {spärr_vid(Fält::Rader)}
                {spärr_vid(Fält::Summa)}
                <p>
                    <button class="knapp knapp-diskret" type="button" on:click=lägg_till_rad>
                        "+ Rad"
                    </button>
                    " "
                    <span class="diskret">"Kontot följer momssatsen och kan ändras per rad."</span>
                </p>
                {företagets}
                {bokförs_som}
                <p class="knappar">
                    <button
                        class="knapp knapp-primar"
                        type="submit"
                        name="handling"
                        value="granska"
                        on:click=granska
                        disabled=move || hydrerad.get() && !spärrar.with(Vec::is_empty)
                    >
                        {granska_text}
                    </button>
                    <button class="knapp" type="submit" name="handling" value="spara">
                        "Spara utkast"
                    </button>
                    {radera}
                </p>
                <p class="diskret">
                    "Utfärda låser fakturan, ger den nästa nummer i serien och skapar PDF:en, som \
                     arkiveras som underlag. Därefter ändras den bara med kreditfaktura."
                </p>
            </form>
        </div>
    }
}

/// Vad momsläget betyder för konteringen, under "Bokförs som".
fn momslägets_text(läge: Momsläge) -> Option<String> {
    match läge {
        Momsläge::Moms => None,
        Momsläge::Bygg | Momsläge::EuTjänst => Some(format!(
            "Omvänd skattskyldighet ({}): ingen moms på fakturan, intäkten på {}.",
            läge.omvänd().namn(),
            läge.intäktskonto(Momssats::Noll)
        )),
        Momsläge::UtanMoms => Some(
            "Företaget är inte momsregistrerat: ingen moms och inget momsreg-nr på fakturan."
                .to_string(),
        ),
    }
}

/// Förhandsvisningen av fakturan som den blir, ritad ur formuläret medan användaren skriver.
#[component]
fn Papper(
    faktura: RwSignal<NyKundfaktura>,
    beräkning: Memo<maya_domain::kundfaktura::Beräkning>,
    läge: Memo<Momsläge>,
    säljare: Säljare,
    förutsättningar: Förutsättningar,
) -> impl IntoView {
    let kund = move || faktura.with(|f| f.kunden(&förutsättningar));
    let köpare = move || {
        kund().map(|k| {
            let adress = k
                .adress
                .lines()
                .map(|r| view! { <span class="adressrad">{r.to_string()}</span> })
                .collect_view();
            // Vid omvänd skattskyldighet har fakturan köparens momsreg-nr.
            let momsregnr = (läge.get().fakturatext().is_some() && !k.momsregnr.is_empty()).then(
                || view! { <span class="adressrad">{format!("Momsreg.nr {}", k.momsregnr)}</span> },
            );
            view! {
                <p>
                    <strong>{k.namn.clone()}</strong>
                    {adress}
                    {momsregnr}
                </p>
            }
        })
    };
    let omvänd_text = move || {
        läge
            .get()
            .fakturatext()
            .map(|t| view! { <p class="papper-omvand">{t}</p> })
    };
    let rader = move || {
        faktura.with(|f| {
            beräkning.with(|b| {
                f.rader
                    .iter()
                    .zip(&b.rader)
                    .filter(|(r, _)| !r.tom())
                    .map(|(r, netto)| {
                        view! {
                            <tr>
                                <td>{r.beskrivning.clone()}</td>
                                <td class="belopp">{format!("{} {}", r.antal, r.enhet)}</td>
                                <td class="belopp">{r.à_pris.clone()}</td>
                                <td class="belopp">{format!("{} %", r.momssats)}</td>
                                <td class="belopp">{netto.map(kr).unwrap_or_default()}</td>
                            </tr>
                        }
                    })
                    .collect_view()
            })
        })
    };
    let datum = move || faktura.with(|f| f.fakturadatum.clone());
    let total = move || beräkning.with(|b| kr(b.total));
    let betalsätt = [
        ("Bankgiro", säljare.bankgiro.clone()),
        ("Plusgiro", säljare.plusgiro.clone()),
        ("IBAN", säljare.iban.clone()),
    ]
    .into_iter()
    .filter(|(_, v)| !v.is_empty())
    .map(|(n, v)| format!("{n} {v}"))
    .collect::<Vec<_>>()
    .join(" · ");
    view! {
        <aside class="papper" aria-label="Förhandsvisning">
            <p class="papper-utkast">"UTKAST"</p>
            <header>
                <p>
                    <strong>{säljare.namn.clone()}</strong>
                    <span class="adressrad">{säljare.gatuadress.clone()}</span>
                    <span class="adressrad">{format!("{} {}", säljare.postnummer, säljare.postort)}</span>
                </p>
                <h3>"Faktura"</h3>
            </header>
            {köpare}
            <p class="diskret">"Fakturadatum " {datum}</p>
            <table class="rader">
                <tbody>{rader}</tbody>
            </table>
            <p class="papper-summa">"Att betala " {total} " kr"</p>
            {omvänd_text}
            <p class="diskret">{betalsätt}</p>
            <p class="diskret">"Uppdateras medan du skriver."</p>
        </aside>
    }
}

/// Panelen Kreditfaktura… (#140): hela fakturan eller ett antal per rad. Utkastet som skapas
/// har negativa rader och en hänvisning till originalet och utfärdas som en faktura.
#[component]
fn KrediteraPanel(action: String, stäng: String, k: KrediteraVy) -> impl IntoView {
    let hela = RwSignal::new(k.start.hela);
    let fel = k
        .fel
        .clone()
        .map(|t| view! { <p class="fel" role="alert">{t}</p> });
    let rader = k
        .rader
        .iter()
        .enumerate()
        .map(|(i, (beskrivning, antal, netto))| {
            let värde = k.start.antal.get(i).cloned().unwrap_or_default();
            view! {
                <tr>
                    <td>{beskrivning.clone()}</td>
                    <td class="belopp">{antal.clone()}</td>
                    <td class="belopp">{netto.clone()}</td>
                    <td class="belopp">
                        <input
                            name=format!("antal_{i}")
                            type="text"
                            inputmode="decimal"
                            class="belopp"
                            aria-label=format!("Antal att kreditera på {beskrivning}")
                            value=värde
                            disabled=move || hela.get()
                        />
                    </td>
                </tr>
            }
        })
        .collect_view();
    view! {
        <section class="panel kreditera" aria-labelledby="kreditera-rubrik">
            <header class="panelhuvud">
                <h3 id="kreditera-rubrik">"Kreditfaktura"</h3>
                <a class="knapp knapp-diskret" href=stäng>
                    "Avbryt"
                </a>
            </header>
            <p class="diskret">
                {format!(
                    "Kvar att kreditera {} kr. Utkastet får originalets rader med negativt antal \
                     och en hänvisning till fakturan, och utfärdas med nästa nummer i serien.",
                    k.kvar,
                )}
            </p>
            {fel}
            <form method="post" action=action novalidate>
                <p>
                    <label class="kryss">
                        <input
                            type="radio"
                            name="hela"
                            value="1"
                            checked=k.start.hela
                            on:change=move |_| hela.set(true)
                        />
                        " Hela fakturan"
                    </label>
                    " "
                    <label class="kryss">
                        <input
                            type="radio"
                            name="hela"
                            value=""
                            checked=!k.start.hela
                            on:change=move |_| hela.set(false)
                        />
                        " Ett antal per rad"
                    </label>
                </p>
                <table class="rader">
                    <thead>
                        <tr>
                            <th>"Rad"</th>
                            <th class="belopp">"Antal"</th>
                            <th class="belopp">"Netto"</th>
                            <th class="belopp">"Kreditera"</th>
                        </tr>
                    </thead>
                    <tbody>{rader}</tbody>
                </table>
                <p class="knappar">
                    <button class="knapp knapp-primar" type="submit">
                        "Skapa kreditfaktura"
                    </button>
                </p>
            </form>
        </section>
    }
}

/// En utfärdad faktura som dokument, med den arkiverade PDF:en och verifikatet.
#[component]
fn Fakturadokument(bas: String, d: FakturaVy) -> impl IntoView {
    let visa = use_query_map().with_untracked(|q| q.get("visa").is_some());
    let lista = kundfakturorna(&bas);
    let pdf = d.pdf.clone();
    let förhandsvisning = visa.then(|| {
        view! {
            <iframe class="granska-pdf" title="Den arkiverade PDF:en" src=pdf.href.clone()></iframe>
        }
    });
    let visa_länk = if visa {
        view! { <a class="knapp knapp-diskret" href=format!("{lista}?f={}", d.id)>"Dölj PDF:en"</a> }
    } else {
        view! { <a class="knapp knapp-diskret" href=format!("{lista}?f={}&visa=1", d.id)>"Visa PDF:en"</a> }
    };
    let adress = d
        .kund
        .adress
        .lines()
        .map(|r| view! { <span class="adressrad">{r.to_string()}</span> })
        .collect_view();
    let (referens, ocr) = d.referens.clone();
    let er_referens = (!d.er_referens.is_empty()).then(|| {
        view! {
            <dt>"Er referens"</dt>
            <dd>{d.er_referens.clone()}</dd>
        }
    });
    let rabatt = d.rader.iter().any(|r| !r.rabatt.is_empty());
    let rader = d
        .rader
        .iter()
        .map(|r| {
            let rabatt = rabatt.then(|| view! { <td class="belopp">{r.rabatt.clone()}</td> });
            view! {
                <tr>
                    <td>{r.beskrivning.clone()}</td>
                    <td class="belopp">{r.antal.clone()}</td>
                    <td class="belopp">{r.à_pris.clone()}</td>
                    {rabatt}
                    <td class="belopp">{r.moms.clone()}</td>
                    <td>{r.konto.clone()}</td>
                    <td class="belopp">{r.netto.clone()}</td>
                </tr>
            }
        })
        .collect_view();
    let kolumner = if rabatt { "6" } else { "5" };
    let moms = d
        .momsrader
        .iter()
        .map(|(text, belopp)| {
            view! {
                <tr>
                    <td colspan=kolumner>{text.clone()}</td>
                    <td class="belopp">{belopp.clone()}</td>
                </tr>
            }
        })
        .collect_view();
    let historik = d
        .historik
        .iter()
        .map(|(tid, typ, namn)| {
            view! {
                <li>
                    <span class="diskret">{tid.clone()}</span>
                    " "
                    {typ.clone()}
                    " "
                    <span class="diskret">"· " {namn.clone()}</span>
                </li>
            }
        })
        .collect_view();
    let (verifikat_id, verifikat_nummer) = d.verifikat.clone();
    // #139 Betalningar: märket, det som återstår, handlingen, panelerna och betalningarna.
    let här = format!("{lista}?f={}", d.id);
    let vald_panel = use_query_map()
        .with_untracked(|q| q.get("panel"))
        .and_then(|k| Panel::från_kod(&k));
    let (status, kvar, betala, betalpanel, betalningar) = match d.reskontra.clone() {
        Some(r) => {
            let status =
                view! { <Märke kod=r.märke.0.clone() namn=r.märke.1.clone() /> }.into_any();
            // #140: ett tillgodohavande efter en kreditfaktura återbetalas.
            let kvar = match r.återstår_öre {
                ..0 => Some(
                    view! {
                        <span class="diskret tillgodo">
                            {format!(
                                " · Kunden har tillgodo {} kr",
                                maya_domain::belopp::Saldo::från_öre(-r.återstår_öre),
                            )}
                        </span>
                    }
                    .into_any(),
                ),
                1.. => Some(
                    view! { <span class="diskret">{format!(" · {} kvar", r.återstår)}</span> }
                        .into_any(),
                ),
                0 => None,
            };
            let betala = match r.återstår_öre {
                1.. => Some(view! {
                    <a
                        class="knapp knapp-primar"
                        href=format!("{här}&panel={}", Panel::Betalning.kod())
                    >
                        "Registrera betalning"
                    </a>
                }),
                ..0 => Some(view! {
                    <a
                        class="knapp knapp-primar"
                        href=format!("{här}&panel={}", Panel::Återbetala.kod())
                    >
                        "Återbetala…"
                    </a>
                }),
                0 => None,
            };
            let betalpanel = match (vald_panel, r.betalning.clone(), r.rätta.clone()) {
                (Some(Panel::Betalning | Panel::Återbetala), Some(v), _) => Some(
                    view! {
                        <Betalningspanel
                            action=format!("{lista}/{}/betalning", d.id)
                            stäng=här.clone()
                            v=v
                        />
                    }
                    .into_any(),
                ),
                (Some(Panel::RättaBetalning), _, Some(rv)) => Some(
                    view! {
                        <RättaBetalningspanel
                            action=format!("{lista}/{}/betalningar/{}/ratta", d.id, rv.id)
                            stäng=här.clone()
                            r=rv
                        />
                    }
                    .into_any(),
                ),
                _ => None,
            };
            let betalningar =
                view! { <Betalningar bas=bas.clone() här=här.clone() r=r /> }.into_any();
            (status, kvar.into_any(), betala, betalpanel, betalningar)
        }
        // #140: en kreditfaktura har ingen egen reskontrapost.
        None if d.krediterar.is_some() => (
            view! { <Märke kod="kreditfaktura".to_string() namn="Kreditfaktura".to_string() /> }
                .into_any(),
            ().into_any(),
            None,
            None,
            ().into_any(),
        ),
        None => (
            view! { <span class="markering">"Utfärdad"</span> }.into_any(),
            ().into_any(),
            None,
            None,
            ().into_any(),
        ),
    };
    // #140 Kreditfakturor: panelen, länken till originalet och kreditfakturorna.
    let betalpanel = match d.kreditera.clone() {
        Some(k) => Some(
            view! {
                <KrediteraPanel
                    action=format!("{lista}/{}/kreditera", d.id)
                    stäng=här.clone()
                    k=k
                />
            }
            .into_any(),
        ),
        None => betalpanel,
    };
    let krediterar = d.krediterar.clone().map(|(id, text)| {
        view! {
            <dt>"Krediterar"</dt>
            <dd>
                <a href=format!("{lista}?f={id}")>{text}</a>
            </dd>
        }
    });
    let krediteras_av = (!d.krediteras_av.is_empty()).then(|| {
        let länkar = d
            .krediteras_av
            .iter()
            .map(|(id, nummer, total)| {
                view! {
                    <span class="adressrad">
                        <a href=format!("{lista}?f={id}")>{format!("Kreditfaktura {nummer}")}</a>
                        <span class="diskret">{format!(" · {total} kr")}</span>
                    </span>
                }
            })
            .collect_view();
        view! {
            <dt>"Krediteras av"</dt>
            <dd>{länkar}</dd>
        }
    });
    let kreditera = d.kan_kreditera.then(|| {
        view! {
            <a class="knapp knapp-diskret" href=format!("{här}&panel={}", Panel::Kreditera.kod())>
                "Kreditfaktura…"
            </a>
        }
    });
    // Handlingarna som paneler (`?panel=<kod>`); raden döljs medan en panel är öppen.
    // #138 Utskick: handlingen, panelen och avsnittet.
    let skicka_text = match (d.epost, d.utskick.iter().any(|u| u.epost)) {
        (false, _) => "Markera som skickad…",
        (true, false) => "Skicka…",
        (true, true) => "Skicka igen…",
    };
    // #141 Kundförlust: panelerna och förlusterna i uppgifterna.
    let förlustpanel = d.kundförlust.as_ref().and_then(|k| {
        match (vald_panel, k.panel.clone(), k.återkalla.clone()) {
            (Some(Panel::Kundförlust), Some(p), _) => Some(
                view! {
                    <crate::kundforlust::Kundförlustpanel
                        action=format!("{lista}/{}/kundforlust", d.id)
                        stäng=här.clone()
                        p=p
                    />
                }
                .into_any(),
            ),
            (Some(Panel::ÅterkallaKundförlust), _, Some(å)) => Some(
                view! {
                    <crate::kundforlust::KundförlustÅterkallelsepanel
                        action=format!("{lista}/{}/kundforlust/{}/aterkalla", d.id, å.id)
                        stäng=här.clone()
                        vy=å
                    />
                }
                .into_any(),
            ),
            _ => None,
        }
    });
    let kundförlusterna = d.kundförlust.as_ref().map(|k| {
        view! { <crate::kundforlust::Kundförlusterna bas=bas.clone() poster=k.poster.clone() /> }
    });
    let panel = match (d.rätta_kontering.clone(), d.skicka.clone()) {
        // #142 Rätta kontering
        (Some(p), _) => Some(
            view! { <crate::kontering::RättaKonteringPanel lista=lista.clone() id=d.id.clone() p=p /> }
                .into_any(),
        ),
        // #138 Utskick
        (None, Some(s)) => {
            Some(view! { <SkickaPanel lista=lista.clone() id=d.id.clone() s=s /> }.into_any())
        }
        (None, None) => betalpanel.or(förlustpanel),
    };
    let handlingar = panel.is_none().then(|| {
        // #141 Kundförlust
        let kundförlust = d.kundförlust.as_ref().and_then(|k| {
            if k.gällande.is_some() {
                Some(
                    view! {
                        <a
                            class="knapp"
                            href=format!("{här}&panel={}", Panel::ÅterkallaKundförlust.kod())
                        >
                            "Återkalla kundförlust"
                        </a>
                    }
                    .into_any(),
                )
            } else if k.kan_bokföras {
                Some(
                    view! {
                        <a
                            class="knapp knapp-fara-diskret"
                            href=format!("{här}&panel={}", Panel::Kundförlust.kod())
                        >
                            "Kundförlust…"
                        </a>
                    }
                    .into_any(),
                )
            } else {
                None
            }
        });
        let rätta_kontering = d.kan_rätta_kontering.then(|| {
            view! {
                <a class="knapp knapp-diskret" href=format!("{lista}?f={}&panel=ratta", d.id)>
                    "Rätta kontering…"
                </a>
            }
        });
        view! {
            <p class="dokument-handlingar">
                {betala}
                <a class="knapp" href=format!("{lista}?f={}&panel=skicka", d.id)>{skicka_text}</a>
                {rätta_kontering}
                {kundförlust}
                {kreditera}
            </p>
        }
    });
    let köparens_momsregnr = (d.omvänd.is_some() && !d.kund.momsregnr.is_empty()).then(
        || view! { <span class="adressrad">{format!("Momsreg.nr {}", d.kund.momsregnr)}</span> },
    );
    let momsläget = match (&d.omvänd, d.utan_moms) {
        (Some((namn, _)), _) => Some(
            view! {
                <dt>"Omvänd skattskyldighet"</dt>
                <dd>{namn.clone()}</dd>
            }
            .into_any(),
        ),
        (None, true) => Some(
            view! {
                <dt>"Moms"</dt>
                <dd>"Ingen, företaget är inte momsregistrerat"</dd>
            }
            .into_any(),
        ),
        (None, false) => None,
    };
    let omvänd_text = d
        .omvänd
        .as_ref()
        .map(|(_, text)| view! { <p class="papper-omvand">{text.clone()}</p> });
    view! {
        <div class="fakturaformular-ram" class:med-forhandsvisning=visa>
            {förhandsvisning}
            <article class="dokument kundfaktura">
                <header class="formular-huvud">
                    <div>
                        <p class="dokument-nummer">
                            {format!(
                                "{} {}",
                                if d.krediterar.is_some() { "Kreditfaktura" } else { "Kundfaktura" },
                                d.nummer,
                            )}
                        </p>
                        <h2>{d.kund.namn.clone()}</h2>
                        <p>
                            {status}
                            " "
                            <span class="belopp">{d.total.clone()} " kr"</span>
                            {kvar}
                        </p>
                    </div>
                    {visa_länk}
                </header>
                {handlingar}
                {panel}
                <dl class="metadata">
                    <dt>"Kund"</dt>
                    <dd>{adress} {köparens_momsregnr}</dd>
                    {krediterar}
                    {krediteras_av}
                    {momsläget}
                    <dt>"Fakturadatum"</dt>
                    <dd>{d.fakturadatum.clone()}</dd>
                    <dt>"Förfallodag"</dt>
                    <dd>{d.förfallodag.clone()}</dd>
                    <dt>"Leveransdatum"</dt>
                    <dd>{d.leveransdatum.clone()}</dd>
                    {er_referens}
                    <dt>{if ocr { "OCR" } else { "Referens" }}</dt>
                    <dd>{referens}</dd>
                    <dt>"Verifikat"</dt>
                    <dd>
                        <a href=format!("{bas}/verifikat?v={verifikat_id}")>{verifikat_nummer}</a>
                        <span class="diskret">" utfärdande"</span>
                    </dd>
                    <dt>"Utfärdad"</dt>
                    <dd>{d.utfärdad.0.clone()} " av " {d.utfärdad.1.clone()} " via " {d.utfärdad.2.clone()}</dd>
                    {kundförlusterna}
                </dl>
                <table class="rader">
                    <thead>
                        <tr>
                            <th>"Beskrivning"</th>
                            <th class="belopp">"Antal"</th>
                            <th class="belopp">"À-pris"</th>
                            {rabatt.then(|| view! { <th class="belopp">"Rabatt"</th> })}
                            <th class="belopp">"Moms"</th>
                            <th>"Konto"</th>
                            <th class="belopp">"Netto"</th>
                        </tr>
                    </thead>
                    <tbody>{rader}</tbody>
                    <tfoot>
                        <tr>
                            <td colspan=kolumner>"Netto"</td>
                            <td class="belopp">{d.netto.clone()}</td>
                        </tr>
                        {moms}
                        <tr class="summa">
                            <td colspan=kolumner>"Totalt med moms"</td>
                            <td class="belopp">{d.total.clone()}</td>
                        </tr>
                    </tfoot>
                </table>
                {omvänd_text}
                {betalningar}
                <section class="arkiverad-pdf">
                    <h3>"PDF"</h3>
                    <dl class="metadata">
                        <dt>"Fil"</dt>
                        <dd>
                            <a href=pdf.href.clone() target="_blank" rel="external noopener">{pdf.filnamn.clone()}</a>
                            <span class="diskret">" · " {pdf.storlek.clone()}</span>
                        </dd>
                        <dt>"Arkiverad"</dt>
                        <dd>{pdf.arkiverad.clone()}</dd>
                        <dt>"SHA-256"</dt>
                        <dd class="hash">{pdf.sha256.clone()}</dd>
                    </dl>
                    <p class="diskret">"Den arkiverade PDF:en är kundens exemplar och genereras aldrig om."</p>
                </section>
                <Utskicken utskick=d.utskick.clone() />
                <section>
                    <h3>"Historik"</h3>
                    <ul class="historik">{historik}</ul>
                </section>
            </article>
        </div>
    }
}
