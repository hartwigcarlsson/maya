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
use maya_domain::kund::{self, NyKund};
use maya_domain::kundfaktura::{
    Fakturakund, Fält, Förutsättningar, NY, NyFakturarad, NyKundfaktura, Radfält, Spärr, Säljare,
    intäktskonto,
};
use maya_domain::rakenskapsar::Räkenskapsår;
use maya_domain::reskontra::Grupp;
use serde::{Deserialize, Serialize};

use crate::app::LoggaIn;
use crate::betalning::{Betalningar, Betalningspanel, Märke, ReskontraVy, RättaBetalningspanel};
use crate::foretag::IngenÅtkomst;
use crate::kunder::kundregistret;
use crate::sidodata::sida;
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
    /// Verifikatets id och nummer.
    pub verifikat: (String, String),
    pub pdf: PdfVy,
    /// Tid, namn och kanal.
    pub utfärdad: (String, String, String),
    /// Tid, händelse och namn.
    pub historik: Vec<(String, String, String)>,
    /// Status, betalningar och det som återstår (#139).
    pub reskontra: Option<ReskontraVy>,
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
        kod => return crate::betalning::feltext(kod),
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
        fakturadatum: fält("fakturadatum"),
        leveransdatum: fält("leveransdatum"),
        förfallodag: fält("forfallodag"),
        er_referens: fält("er_referens"),
        startnummer: fält("startnummer"),
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
    /// Registrera betalning (#139).
    Betalning,
    /// Rätta betalning, med betalningen i `&b=` (#139).
    RättaBetalning,
}

impl Panel {
    pub fn från_kod(kod: &str) -> Option<Self> {
        match kod {
            "betalning" => Some(Panel::Betalning),
            "ratta-betalning" => Some(Panel::RättaBetalning),
            _ => None,
        }
    }

    pub fn kod(self) -> &'static str {
        match self {
            Panel::Betalning => "betalning",
            Panel::RättaBetalning => "ratta-betalning",
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
            // #139 Betalningar
            .route(
                "/foretag/{id}/kundfakturor/{f}/betalning",
                post(crate::betalning::server::registrera_kundbetalning),
            )
            .route(
                "/foretag/{id}/kundfakturor/{f}/betalningar/{b}/ratta",
                post(crate::betalning::server::rätta_kundbetalning),
            )
            .with_state(Arc::new(state))
    }

    /// Hämtar det som Kundfakturor visar. Utan åtkomst blir datan tom.
    pub(crate) async fn kundfakturadata(b: &Sidbegäran) -> Result<Kundfakturadata, ServerFnError> {
        let (företag, ny) = match b.delar().as_slice() {
            ["foretag", id, "kundfakturor"] => (id.parse::<FöretagsId>().ok(), false),
            ["foretag", id, "kundfakturor", "ny"] => (id.parse::<FöretagsId>().ok(), true),
            _ => (None, false),
        };
        let Some(företag) = företag else {
            return Ok(Kundfakturadata::default());
        };
        let valt = if ny {
            None
        } else {
            b.värde("f").and_then(|f| f.parse::<KundfakturaId>().ok())
        };
        match hämta(b, företag, ny, valt).await {
            Ok(data) => Ok(data),
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
            .filter(|f| f.status != Status::Utkast)
            .map(|f| f.återstår.max(0))
            .sum();
        let lista = rader
            .into_iter()
            .map(|f| {
                let utfärdad = f.förfallodag.filter(|_| f.status != Status::Utkast);
                ListVy {
                    id: f.id.to_string(),
                    utkast: f.status == Status::Utkast,
                    nummer: f.nummer.map(|n| n.to_string()).unwrap_or_default(),
                    kund: f.kund,
                    fakturadatum: f.fakturadatum.map(|d| d.to_string()).unwrap_or_default(),
                    förfallodag: f.förfallodag.map(|d| d.to_string()).unwrap_or_default(),
                    total: kr(f.total),
                    ny_kund: f.ny_kund,
                    återstår: kr(f.återstår),
                    märke: utfärdad.map(|d| {
                        let m = maya_domain::reskontra::märke(f.total, f.återstår, d, idag);
                        (m.kod().to_string(), m.namn().to_string())
                    }),
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
                    // #139 Betalningar
                    let panel = b.värde("panel");
                    d.reskontra = Some(
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
                    );
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
                data.granskning = Some(granskning(&u.formulär, &förutsättningar, &kontonamn));
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
            },
            Err(spärrar) => GranskningVy {
                spärrar: spärrar.into_iter().map(|s| s.text).collect(),
                ..GranskningVy::default()
            },
        }
    }

    fn dokument(
        företag: FöretagsId,
        f: &kundfaktura::Kundfaktura,
        u: &kundfaktura::Utfärdning,
    ) -> FakturaVy {
        FakturaVy {
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
            reskontra: None,
        }
    }

    type Fält = Vec<(String, String)>;

    fn värde(fält: &Fält, namn: &str) -> Option<String> {
        fält.iter().find(|(k, _)| k == namn).map(|(_, v)| v.clone())
    }

    /// Den inloggade och företaget i sökvägen, eller svaret om något saknas.
    fn tolka(
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
    ) -> Response {
        let (inloggad, företag) = match tolka(&id, inloggad) {
            Ok(t) => t,
            Err(r) => return *r,
        };
        let Ok(f) = f.parse::<KundfakturaId>() else {
            return finns_inte();
        };
        let lista = format!("/foretag/{företag}/kundfakturor");
        match kundfaktura::utfärda(&state.db, &state.klocka, &inloggad.aktör, företag, f).await {
            Ok(u) => Redirect::to(&format!("{lista}?f={}&utfardad=1", u.id)).into_response(),
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

    fn fakturafel(e: KundfakturaFel) -> Response {
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
        // #139 Betalningar
        (
            "betald",
            "Betalningen registrerades och verifikatet bokfördes.",
        ),
        (
            "rattad",
            "Betalningen rättades. Rättelsen bokfördes och betalningen räknas inte längre.",
        ),
    ]
    .into_iter()
    .find(|(namn, _)| fråga(namn).is_some())
    .map(|(_, text)| view! { <p class="notis" role="status">{text}</p> });
    let fel = fråga("fel")
        .and_then(|k| feltext(&k))
        .map(|text| view! { <p class="fel" role="alert">{text}</p> });
    let lista = kundfakturorna(&bas);
    let tillbakaskickat = fråga("form").as_deref() == Some("kundfaktura");
    let från_adressen = || ur_fält(|namn| fråga(namn));
    let detalj = match (&data.utkast, &data.dokument, &data.förutsättningar) {
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
                från_adressen()
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
                    <span class="diskret">{märke} " " {datum}</span>
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
    if start.rader.last().is_none_or(|r| !r.tom()) {
        start.rader.push(NyFakturarad::default());
    }
    let faktura = RwSignal::new(start.clone());
    let förutsättningar = f.domän();
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
    let beräkning = Memo::new(move |_| faktura.with(NyKundfaktura::beräkna));
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
    let lägg_till_rad = move |_| faktura.update(|f| f.rader.push(NyFakturarad::default()));
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
                    let sats = fält("momssats");
                    let satser = Momssats::ALLA
                        .into_iter()
                        .map(|s| {
                            let p = s.procent().to_string();
                            view! {
                                <option value=p.clone() selected=p == sats>
                                    {s.to_string()}
                                </option>
                            }
                        })
                        .collect_view();
                    let byt_sats = move |ev: leptos::ev::Event| {
                        let ny = event_target_value(&ev);
                        faktura.update(|f| {
                            let r = &mut f.rader[i];
                            // Kontot följer momssatsen så länge det är satsens konto.
                            let förra = Momssats::tolka(&r.momssats).map(|s| intäktskonto(s).to_string());
                            if (förra.as_deref() == Some(r.konto.trim()) || r.konto.trim().is_empty())
                                && let Some(s) = Momssats::tolka(&ny)
                            {
                                r.konto = intäktskonto(s).to_string();
                            }
                            r.momssats = ny;
                        });
                    };
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
                                <select name=format!("momssats_{i}") aria-label="Moms" on:change=byt_sats>
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
    let summor = move || {
        beräkning.with(|b| {
            let moms = b
                .momsrader
                .iter()
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
                    <td colspan="7">"Att betala"</td>
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
                    </div>
                }
            })
        })
    };
    let intäktskonton = f
        .intäktskonton
        .iter()
        .map(|(nr, namn)| view! { <option value=nr.to_string() label=namn.clone()></option> })
        .collect_view();
    let (action, rubrik, märke, radera) = match &utkast {
        Some(u) => (
            format!("{lista}/{}", u.id),
            "Kundfaktura",
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
                <div class="falt-rad">
                    <div class="bred">
                        <label for="kund">"Kund"</label>
                        <select
                            id="kund"
                            name="kund"
                            on:change=move |ev| faktura.update(|f| f.kund = event_target_value(&ev))
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
                            on:input=move |ev| faktura.update(|f| f.fakturadatum = event_target_value(&ev))
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

/// Förhandsvisningen av fakturan som den blir, ritad ur formuläret medan användaren skriver.
#[component]
fn Papper(
    faktura: RwSignal<NyKundfaktura>,
    beräkning: Memo<maya_domain::kundfaktura::Beräkning>,
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
            view! {
                <p>
                    <strong>{k.namn.clone()}</strong>
                    {adress}
                </p>
            }
        })
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
            <p class="diskret">{betalsätt}</p>
            <p class="diskret">"Uppdateras medan du skriver."</p>
        </aside>
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
    // #139 Betalningar: märket, det som återstår, handlingen och panelerna.
    let här = format!("{lista}?f={}", d.id);
    let panel = use_query_map()
        .with_untracked(|q| q.get("panel"))
        .and_then(|k| Panel::från_kod(&k));
    let (status, kvar, handlingar, paneler, betalningar) = match d.reskontra.clone() {
        Some(r) => {
            let status =
                view! { <Märke kod=r.märke.0.clone() namn=r.märke.1.clone() /> }.into_any();
            let kvar = (r.återstår_öre > 0).then(|| {
                view! { <span class="diskret">{format!(" · {} kvar", r.återstår)}</span> }
            });
            let handlingar = (panel.is_none() && r.återstår_öre > 0).then(|| {
                view! {
                    <p class="knappar dokument-handlingar">
                        <a
                            class="knapp knapp-primar"
                            href=format!("{här}&panel={}", Panel::Betalning.kod())
                        >
                            "Registrera betalning"
                        </a>
                    </p>
                }
            });
            let paneler = match (panel, r.betalning.clone(), r.rätta.clone()) {
                (Some(Panel::Betalning), Some(v), _) => view! {
                    <Betalningspanel
                        action=format!("{lista}/{}/betalning", d.id)
                        stäng=här.clone()
                        v=v
                    />
                }
                .into_any(),
                (Some(Panel::RättaBetalning), _, Some(rv)) => view! {
                    <RättaBetalningspanel
                        action=format!("{lista}/{}/betalningar/{}/ratta", d.id, rv.id)
                        stäng=här.clone()
                        r=rv
                    />
                }
                .into_any(),
                _ => ().into_any(),
            };
            let betalningar =
                view! { <Betalningar bas=bas.clone() här=här.clone() r=r /> }.into_any();
            (
                status,
                kvar.into_any(),
                handlingar.into_any(),
                paneler,
                betalningar,
            )
        }
        None => (
            view! { <span class="markering">"Utfärdad"</span> }.into_any(),
            ().into_any(),
            ().into_any(),
            ().into_any(),
            ().into_any(),
        ),
    };
    view! {
        <div class="fakturaformular-ram" class:med-forhandsvisning=visa>
            {förhandsvisning}
            <article class="dokument kundfaktura">
                <header class="formular-huvud">
                    <div>
                        <p class="dokument-nummer">{format!("Kundfaktura {}", d.nummer)}</p>
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
                {paneler}
                <dl class="metadata">
                    <dt>"Kund"</dt>
                    <dd>{adress}</dd>
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
                <section>
                    <h3>"Historik"</h3>
                    <ul class="historik">{historik}</ul>
                </section>
            </article>
        </div>
    }
}
