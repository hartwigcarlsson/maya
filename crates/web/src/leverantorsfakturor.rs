//! Fliken Leverantörsfakturor (variant C i `prototype/43-leverantorsfakturor`): listan efter
//! förfallodag och den valda fakturan med originalet till vänster och fälten till höger.
//!
//! Formuläret prövas med `maya_domain::leverantorsfaktura` i webbläsaren och av `core` när det
//! registreras, med samma koder, och rutan "Bokförs som" visar konteringen medan användaren
//! skriver. Utan JavaScript skickas formuläret till servern, som skickar tillbaka hit med det
//! användaren skrev; sidan prövar det igen och visar felen vid fälten.

use leptos::prelude::*;
use leptos_router::hooks::use_query_map;
use maya_domain::belopp::{Saldo, tolka_med_tecken};
use maya_domain::foretag::Momsperiod;
use maya_domain::kontoplan::Kontonummer;
use maya_domain::leverantor::{self, NyLeverantör, OmvändSkattskyldighet};
use maya_domain::leverantorsfaktura::{
    Fält, Förutsättningar, Krediterbar, Leverantörsval, MOMSSATSER, NY, NyLeverantörsfaktura,
    NyRad, Rad, Registrerad, Spärr, moms_med_tecken, Åtgärd,
};
use maya_domain::rakenskapsar::Räkenskapsår;
use maya_domain::reskontra::Grupp;
use serde::{Deserialize, Serialize};

use crate::app::LoggaIn;
use crate::betalning::{Betalningar, Betalningspanel, Märke, ReskontraVy, RättaBetalningspanel};
use crate::foretag::IngenÅtkomst;
use crate::sidodata::sida;
use crate::underlag::{Förhandsvisning, Gem, UnderlagVy};
use crate::verifikat::{KontoVy, UtkastBadge, företagets_bas, inloggad};

/// En faktura i listan.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListradVy {
    pub id: String,
    pub utkast: bool,
    pub leverantör: String,
    pub ny_leverantör: bool,
    pub fakturanummer: String,
    /// `ÅÅÅÅ-MM-DD`, eller tomt.
    pub förfallodag: String,
    pub total: String,
    // #147 Betalningar
    /// Det som återstår att betala; för ett utkast totalen.
    pub återstår: String,
    /// Märkets kod och namn för en registrerad faktura.
    pub märke: Option<(String, String)>,
    /// Gruppen i listan.
    pub grupp: Option<Grupp>,
    /// "förfaller om 5 dagar" för en öppen registrerad faktura, annars tomt.
    pub förfallotext: String,
}

/// En post i fakturans historik.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistorikVy {
    pub tid: String,
    pub text: String,
    pub namn: String,
}

/// En rad i registreringens verifikat.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KonteringVy {
    pub konto: String,
    pub kontonamn: String,
    pub debet: String,
    pub kredit: String,
}

/// Den valda fakturan.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FakturaVy {
    pub id: String,
    pub utkast: bool,
    pub original: UnderlagVy,
    /// Formulärets fält som namn och värde, så som fakturan sparades.
    pub fält: Vec<(String, String)>,
    pub leverantörsnamn: String,
    pub ny_leverantör: bool,
    /// Registreringens verifikat: id och nummer.
    pub verifikat: Option<(String, String)>,
    /// Tid och namn.
    pub skapad: (String, String),
    pub registrerad: Option<(String, String)>,
    pub historik: Vec<HistorikVy>,
    pub kontering: Vec<KonteringVy>,
    // #147 Betalningar
    /// Status, betalningar och det som återstår för en registrerad faktura.
    pub reskontra: Option<ReskontraVy>,
    /// Betaluppgifterna som betalningen görs med: leverantörens bankgiro, plusgiro eller
    /// IBAN/BIC och fakturans OCR eller meddelande, som etikett och värde.
    pub betaluppgifter: Vec<(String, String)>,
    // #148 Kreditfaktura och Rätta avskrift
    /// Registreringen är vänd med Rätta avskrift, och fakturan skrivs av igen.
    pub upplåst: bool,
    /// Totalen är negativ.
    pub kreditfaktura: bool,
    /// Originalet som kreditfakturan hänvisar till: id och fakturanummer.
    pub krediterar: Option<(String, String)>,
    /// Kreditfakturorna som hänvisar hit: id, fakturanummer och total.
    pub krediteras_av: Vec<(String, String, String)>,
    /// Upplåsningarna: registreringens verifikat, rättelsen (id och nummer), orsaken, tid och namn.
    pub avskrifter: Vec<AvskriftVy>,
}

/// En upplåsning med Rätta avskrift (#148).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AvskriftVy {
    /// Registreringens verifikat: id och nummer.
    pub verifikat: Option<(String, String)>,
    /// Rättelsen: id och nummer.
    pub rättelse: Option<(String, String)>,
    pub orsak: String,
    /// Tid och namn.
    pub rättad: (String, String),
}

/// Det som formuläret prövar spärrarna mot, i webbläsaren.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FörutsättningarVy {
    pub konton: Vec<KontoVy>,
    /// Start, slut och nästa verifikationsnummer, t.ex. `V9`.
    pub räkenskapsår: Vec<(String, String, String)>,
    /// Från vilket datum och momsperiodens kod.
    pub momsperioder: Vec<(String, String)>,
    /// Id, namn, standardkonto och koden för den förvalda omvända skattskyldigheten.
    pub leverantörer: Vec<(String, String, Option<u16>, String)>,
    /// Id, leverantör, nummer, fakturadatum och total i öre.
    pub registrerade: Vec<(String, String, String, String, i64)>,
    /// Fakturorna som en kreditfaktura kan hänvisa till (#148).
    pub krediterbara: Vec<KrediterbarVy>,
    /// Det som redan har betalats av en upplåst faktura, i öre (#148).
    pub betalt: i64,
    /// Bokföringsmetodens historik: från vilket datum och metodens kod (#149).
    #[serde(default)]
    pub bokföringsmetoder: Vec<(String, String)>,
}

/// En faktura som en kreditfaktura kan hänvisa till, med raderna som registrerades (#148).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KrediterbarVy {
    pub id: String,
    pub leverantör: String,
    pub fakturanummer: String,
    /// I öre.
    pub total: i64,
    /// Koden för den omvända skattskyldigheten.
    pub omvänd: String,
    /// Konto, netto i öre, momssats och moms i öre.
    pub rader: Vec<(u16, i64, u8, i64)>,
}

impl FörutsättningarVy {
    fn förutsättningar(&self) -> Förutsättningar {
        Förutsättningar {
            konton: self
                .konton
                .iter()
                .filter_map(|k| Some((Kontonummer::new(k.nummer)?, k.aktiv)))
                .collect(),
            räkenskapsår: self
                .räkenskapsår
                .iter()
                .filter_map(|(start, slut, _)| {
                    Some(Räkenskapsår::befintligt(
                        start.parse().ok()?,
                        slut.parse().ok()?,
                    ))
                })
                .collect(),
            momsperioder: self
                .momsperioder
                .iter()
                .filter_map(|(från, kod)| Some((från.parse().ok()?, Momsperiod::från_kod(kod)?)))
                .collect(),
            leverantörer: self
                .leverantörer
                .iter()
                .map(|(id, namn, konto, omvänd)| Leverantörsval {
                    id: id.clone(),
                    namn: namn.clone(),
                    standardkonto: konto.and_then(Kontonummer::new),
                    omvänd_skattskyldighet: OmvändSkattskyldighet::från_kod(omvänd)
                        .unwrap_or_default(),
                })
                .collect(),
            registrerade: self
                .registrerade
                .iter()
                .filter_map(|(id, l, nr, datum, total)| {
                    Some(Registrerad {
                        id: id.clone(),
                        leverantör: l.clone(),
                        fakturanummer: nr.clone(),
                        fakturadatum: datum.parse().ok()?,
                        total: *total,
                    })
                })
                .collect(),
            krediterbara: self
                .krediterbara
                .iter()
                .map(|k| Krediterbar {
                    id: k.id.clone(),
                    leverantör: k.leverantör.clone(),
                    fakturanummer: k.fakturanummer.clone(),
                    total: k.total,
                    omvänd_skattskyldighet: OmvändSkattskyldighet::från_kod(&k.omvänd)
                        .unwrap_or_default(),
                    rader: k
                        .rader
                        .iter()
                        .filter_map(|(konto, netto, sats, moms)| {
                            Some(Rad {
                                konto: Kontonummer::new(*konto)?,
                                netto: *netto,
                                momssats: *sats,
                                moms: *moms,
                            })
                        })
                        .collect(),
                })
                .collect(),
            betalt: self.betalt,
        }
    }

    /// "V9 · 2026-03-14" för fakturadatumet, när det ligger i ett räkenskapsår som finns.
    fn bokförs_som(&self, datum: &str) -> Option<String> {
        let d: jiff::civil::Date = datum.trim().parse().ok()?;
        self.räkenskapsår
            .iter()
            .find(|(start, slut, _)| {
                start.parse().is_ok_and(|s: jiff::civil::Date| s <= d)
                    && slut.parse().is_ok_and(|s: jiff::civil::Date| d <= s)
            })
            .map(|(_, _, nästa)| format!("{nästa} · {d}"))
    }

    fn kontonamn(&self, konto: &str) -> String {
        self.konton
            .iter()
            .find(|k| k.nummer.to_string() == konto.trim())
            .map(|k| k.namn.clone())
            .unwrap_or_default()
    }
}

/// Det som sidan Leverantörsfakturor visar.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Leverantörsfakturadata {
    /// Dagens datum, `ÅÅÅÅ-MM-DD`, för grupperna efter förfallodag.
    pub idag: String,
    pub lista: Vec<ListradVy>,
    pub valt: Option<FakturaVy>,
    pub förutsättningar: FörutsättningarVy,
    /// De okopplade underlagen, för en ny faktura.
    pub okopplade: Vec<UnderlagVy>,
    /// Leverantörsskulderna: det som återstår på de registrerade fakturorna (#147).
    pub leverantörsskulder: String,
}

/// Datan för `/foretag/<id>/leverantorsfakturor` och `/foretag/<id>/leverantorsfakturor/ny`.
#[server(prefix = "/api", endpoint = "leverantorsfakturadata", input = server_fn::codec::GetUrl)]
pub async fn hämta_leverantörsfakturadata(
    adress: String,
) -> Result<Leverantörsfakturadata, ServerFnError> {
    match crate::sidodata::Sidbegäran::för(&adress)? {
        Some(begäran) => server::leverantörsfakturadata(&begäran).await,
        None => Ok(Leverantörsfakturadata::default()),
    }
}

/// Fälten för en ny leverantör i det streckade kortet, med formulärets namn.
const NYA_FÄLT: [(&str, &str); 9] = [
    ("ny_namn", "Namn"),
    ("ny_orgnr", "Orgnr"),
    ("ny_momsregnr", "Momsreg-nr"),
    ("ny_gatuadress", "Gatuadress"),
    ("ny_postnummer", "Postnummer"),
    ("ny_postort", "Postort"),
    ("ny_bankgiro", "Bankgiro"),
    ("ny_plusgiro", "Plusgiro"),
    ("ny_standardkonto", "Standardkonto"),
];

/// Formuläret ur fältens värden: fakturans fält, [`NYA_FÄLT`] och `konto_<i>` …
pub(crate) fn formulär(värde: impl Fn(&str) -> Option<String>) -> NyLeverantörsfaktura {
    let v = |namn: &str| värde(namn).unwrap_or_default();
    let antal = (0..200)
        .take_while(|i| värde(&format!("konto_{i}")).is_some())
        .count();
    NyLeverantörsfaktura {
        leverantör: v("leverantor"),
        ny_leverantör: NyLeverantör {
            namn: v("ny_namn"),
            orgnr: v("ny_orgnr"),
            momsregnr: v("ny_momsregnr"),
            gatuadress: v("ny_gatuadress"),
            postnummer: v("ny_postnummer"),
            postort: v("ny_postort"),
            bankgiro: v("ny_bankgiro"),
            plusgiro: v("ny_plusgiro"),
            standardkonto: v("ny_standardkonto"),
            ..NyLeverantör::default()
        },
        fakturanummer: v("fakturanummer"),
        fakturadatum: v("fakturadatum"),
        förfallodag: v("forfallodag"),
        referens: v("referens"),
        totalbelopp: v("totalbelopp"),
        omvänd_skattskyldighet: v("omvand_skattskyldighet"),
        originalvaluta: v("originalvaluta"),
        krediterar: v("krediterar"),
        rader: (0..antal)
            .map(|i| NyRad {
                konto: v(&format!("konto_{i}")),
                netto: v(&format!("netto_{i}")),
                momssats: v(&format!("momssats_{i}")),
                moms: v(&format!("moms_{i}")),
            })
            .collect(),
    }
}

/// Formuläret som fält med namn och värde, så som [`formulär`] läser dem.
#[cfg_attr(not(feature = "ssr"), allow(dead_code))]
pub(crate) fn fält(f: &NyLeverantörsfaktura) -> Vec<(String, String)> {
    let n = &f.ny_leverantör;
    let mut ut: Vec<(String, String)> = [
        ("leverantor", &f.leverantör),
        ("fakturanummer", &f.fakturanummer),
        ("fakturadatum", &f.fakturadatum),
        ("forfallodag", &f.förfallodag),
        ("referens", &f.referens),
        ("totalbelopp", &f.totalbelopp),
        ("omvand_skattskyldighet", &f.omvänd_skattskyldighet),
        ("originalvaluta", &f.originalvaluta),
        ("krediterar", &f.krediterar),
        ("ny_namn", &n.namn),
        ("ny_orgnr", &n.orgnr),
        ("ny_momsregnr", &n.momsregnr),
        ("ny_gatuadress", &n.gatuadress),
        ("ny_postnummer", &n.postnummer),
        ("ny_postort", &n.postort),
        ("ny_bankgiro", &n.bankgiro),
        ("ny_plusgiro", &n.plusgiro),
        ("ny_standardkonto", &n.standardkonto),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.clone()))
    .collect();
    for (i, r) in f.rader.iter().enumerate() {
        ut.push((format!("konto_{i}"), r.konto.clone()));
        ut.push((format!("netto_{i}"), r.netto.clone()));
        ut.push((format!("momssats_{i}"), r.momssats.clone()));
        ut.push((format!("moms_{i}"), r.moms.clone()));
    }
    ut
}

/// Felkoderna i adressen och deras texter.
fn feltext(kod: &str) -> Option<&'static str> {
    Some(match kod {
        "sparrat" => "Fakturan kunde inte registreras. Rätta det som är markerat.",
        "utkast" => "Utkastet kunde inte sparas. Rätta det som är markerat.",
        "inte_okopplat" => "Underlaget är redan kopplat eller original till en annan faktura.",
        "inget_utkast" => "Utkastet finns inte längre, eller är redan registrerat.",
        "for_stort" => "Ett underlag får vara högst 25 MB.",
        // #148 Rätta avskrift
        "inte_registrerad" => "Fakturan är inte registrerad, så avskriften kan inte rättas.",
        "rattelse" => "Registreringen kunde inte rättas i dag. Kontrollera räkenskapsåren.",
        "ingen_fil" => "Välj originalet att ladda upp.",
        // #147 Betalningar
        kod => return crate::betalning::feltext(kod),
    })
}

#[cfg(feature = "ssr")]
pub(crate) mod server {
    use std::sync::Arc;

    use axum::extract::{DefaultBodyLimit, Path, Request, State};
    use axum::response::{IntoResponse, Redirect, Response};
    use axum::routing::post;
    use axum::{Extension, Form, Router};
    use leptos::prelude::ServerFnError;
    use maya_core::atkomst::ÅtkomstFel;
    use maya_core::betalning::{BetalningFel, Faktura};
    use maya_core::foretag::FöretagsId;
    use maya_core::inloggning::Inloggad;
    use maya_core::leverantor;
    use maya_core::leverantorsfaktura::{
        self, FakturaFel, Leverantörsfaktura, LeverantörsfakturaId, Status,
    };
    use maya_core::underlag::{self, UnderlagFel};
    use maya_core::{Databas, Klocka, kontoplan, verifikat};
    use maya_domain::reskontra::{Märke, leverantörsfakturans_märke};
    use maya_domain::tid::minut;

    use super::{
        AvskriftVy, FakturaVy, FörutsättningarVy, HistorikVy, KonteringVy, KrediterbarVy,
        Leverantörsfakturadata, ListradVy, formulär, fält,
    };
    use crate::sidodata::{self, Sidbegäran};
    use crate::svar::{ingen_åtkomst, serverfel};
    use crate::underlag::server::{STÖRSTA_BEGÄRAN, läs_formulär, underlagsfel, vy};
    use crate::verifikat::KontoVy;

    #[derive(Clone)]
    pub(crate) struct LeverantörsfakturaState {
        pub db: Databas,
        pub klocka: Klocka,
    }

    pub(crate) fn routes(state: LeverantörsfakturaState) -> Router {
        Router::new()
            .route("/foretag/{id}/leverantorsfakturor", post(ladda_upp))
            .route("/foretag/{id}/leverantorsfakturor/{f}", post(skicka))
            .route("/foretag/{id}/leverantorsfakturor/{f}/radera", post(radera))
            // #147 Betalningar
            .route(
                "/foretag/{id}/leverantorsfakturor/{f}/betalning",
                post(crate::betalning::server::registrera_leverantörsbetalning),
            )
            .route(
                "/foretag/{id}/leverantorsfakturor/{f}/betalningar/{b}/ratta",
                post(crate::betalning::server::rätta_leverantörsbetalning),
            )
            // #148 Rätta avskrift
            .route(
                "/foretag/{id}/leverantorsfakturor/{f}/ratta-avskrift",
                post(rätta_avskrift),
            )
            .layer(DefaultBodyLimit::max(STÖRSTA_BEGÄRAN))
            .with_state(Arc::new(state))
    }

    pub(crate) async fn leverantörsfakturadata(
        b: &Sidbegäran,
    ) -> Result<Leverantörsfakturadata, ServerFnError> {
        let (företag, ny) = match b.delar().as_slice() {
            ["foretag", id, "leverantorsfakturor"] => (id.parse::<FöretagsId>().ok(), false),
            ["foretag", id, "leverantorsfakturor", "ny"] => (id.parse::<FöretagsId>().ok(), true),
            _ => (None, false),
        };
        let Some(företag) = företag else {
            return Ok(Leverantörsfakturadata::default());
        };
        let valt = if ny {
            None
        } else {
            b.värde("f")
                .and_then(|f| f.parse::<LeverantörsfakturaId>().ok())
        };
        match hämta(b, företag, valt, ny).await {
            Ok(data) => Ok(data),
            Err(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte) => {
                Ok(Leverantörsfakturadata::default())
            }
            Err(e) => Err(sidodata::serverfel(e)),
        }
    }

    async fn hämta(
        b: &Sidbegäran,
        företag: FöretagsId,
        valt: Option<LeverantörsfakturaId>,
        ny: bool,
    ) -> Result<Leverantörsfakturadata, ÅtkomstFel> {
        let (db, aktör) = (&b.db, &b.inloggad.aktör);
        let lista = leverantorsfaktura::lista(db, aktör, företag).await?;
        let konton = kontoplan::konton(db, aktör, företag).await?;
        let kontonamn = |nr: u16| {
            konton
                .iter()
                .find(|k| k.nummer.get() == nr)
                .map(|k| k.namn.clone())
                .unwrap_or_default()
        };
        let faktura = match valt {
            Some(f) => leverantorsfaktura::fakturan(db, aktör, företag, f).await?,
            None => None,
        };
        let förutsättningar = leverantorsfaktura::förutsättningar(
            db,
            aktör,
            företag,
            faktura.as_ref().map(|f| f.id),
        )
        .await?;
        let nästa = verifikat::nästa_nummer(db, aktör, företag).await?;
        let valt = match faktura {
            Some(f) => Some(faktura_vy(b, företag, &f, &kontonamn).await?),
            None => None,
        };
        let okopplade = if ny {
            underlag::lista(db, aktör, företag, underlag::Urval::Okopplade)
                .await?
                .iter()
                .map(vy)
                .collect()
        } else {
            Vec::new()
        };
        let idag = b.klocka.idag();
        let kr = |öre: i64| maya_domain::belopp::Saldo::från_öre(öre).to_string();
        let leverantörsskulder: i64 = lista
            .iter()
            .filter(|r| r.status != Status::Utkast)
            .map(|r| r.återstår.max(0))
            .sum();
        Ok(Leverantörsfakturadata {
            idag: idag.to_string(),
            lista: lista
                .iter()
                .map(|r| {
                    // En upplåst faktura står bland utkasten tills den registreras om (#148).
                    let registrerad = r.förfallodag.filter(|_| r.status == Status::Registrerad);
                    let total = r.total.unwrap_or_default();
                    ListradVy {
                        id: r.id.to_string(),
                        utkast: r.status == Status::Utkast,
                        leverantör: r.leverantörsnamn.clone(),
                        ny_leverantör: r.ny_leverantör,
                        fakturanummer: r.fakturanummer.clone(),
                        förfallodag: r.förfallodag.map(|d| d.to_string()).unwrap_or_default(),
                        total: r.total.map(kr).unwrap_or_default(),
                        återstår: kr(r.återstår),
                        märke: match (r.status, registrerad) {
                            (Status::Upplåst, _) => Some(Märke::Upplåst),
                            (_, Some(_)) if r.kreditfaktura => Some(Märke::Kreditfaktura),
                            (_, Some(d)) => Some(leverantörsfakturans_märke(
                                total,
                                r.krediterat,
                                r.återstår,
                                d,
                                idag,
                            )),
                            _ => None,
                        }
                        .map(|m| (m.kod().to_string(), m.namn().to_string())),
                        grupp: Some(match registrerad {
                            Some(d) => maya_domain::reskontra::grupp(r.återstår, d, idag),
                            None => maya_domain::reskontra::Grupp::Utkast,
                        }),
                        förfallotext: registrerad
                            .filter(|_| r.återstår > 0)
                            .map(|d| maya_domain::reskontra::förfallotext(d, idag))
                            .unwrap_or_default(),
                    }
                })
                .collect(),
            leverantörsskulder: kr(leverantörsskulder),
            valt,
            förutsättningar: FörutsättningarVy {
                konton: konton
                    .iter()
                    .map(|k| KontoVy {
                        nummer: k.nummer.get(),
                        namn: k.namn.clone(),
                        aktiv: k.aktiv,
                    })
                    .collect(),
                räkenskapsår: nästa
                    .iter()
                    .map(|(år, nr)| {
                        (
                            år.start().to_string(),
                            år.slut().to_string(),
                            nr.to_string(),
                        )
                    })
                    .collect(),
                momsperioder: förutsättningar
                    .momsperioder
                    .iter()
                    .map(|(d, m)| (d.to_string(), m.kod().to_string()))
                    .collect(),
                bokföringsmetoder: maya_core::foretag::företaget(&b.db, aktör, företag)
                    .await?
                    .bokföringsmetoder
                    .iter()
                    .map(|(d, m)| (d.to_string(), m.kod().to_string()))
                    .collect(),
                leverantörer: förutsättningar
                    .leverantörer
                    .iter()
                    .map(|l| {
                        (
                            l.id.clone(),
                            l.namn.clone(),
                            l.standardkonto.map(|k| k.get()),
                            l.omvänd_skattskyldighet.kod().to_string(),
                        )
                    })
                    .collect(),
                registrerade: förutsättningar
                    .registrerade
                    .iter()
                    .map(|r| {
                        (
                            r.id.clone(),
                            r.leverantör.clone(),
                            r.fakturanummer.clone(),
                            r.fakturadatum.to_string(),
                            r.total,
                        )
                    })
                    .collect(),
                krediterbara: förutsättningar
                    .krediterbara
                    .iter()
                    .map(|k| KrediterbarVy {
                        id: k.id.clone(),
                        leverantör: k.leverantör.clone(),
                        fakturanummer: k.fakturanummer.clone(),
                        total: k.total,
                        omvänd: k.omvänd_skattskyldighet.kod().to_string(),
                        rader: k
                            .rader
                            .iter()
                            .map(|r| (r.konto.get(), r.netto, r.momssats, r.moms))
                            .collect(),
                    })
                    .collect(),
                betalt: förutsättningar.betalt,
            },
            okopplade,
        })
    }

    async fn faktura_vy(
        b: &Sidbegäran,
        företag: FöretagsId,
        f: &Leverantörsfaktura,
        kontonamn: &impl Fn(u16) -> String,
    ) -> Result<FakturaVy, ÅtkomstFel> {
        let (db, aktör) = (&b.db, &b.inloggad.aktör);
        let original = underlag::underlaget(db, aktör, företag, f.underlag)
            .await?
            .map(|u| vy(&u))
            .unwrap_or_default();
        let kontering = match f.verifikat {
            Some((v, _)) => verifikat::verifikatet(db, aktör, företag, v)
                .await?
                .map(|v| {
                    v.rader
                        .iter()
                        .map(|r| {
                            let konto = r.konto.map(|k| k.get()).unwrap_or_default();
                            KonteringVy {
                                konto: konto.to_string(),
                                kontonamn: kontonamn(konto),
                                debet: r.debet.eller_tomt(),
                                kredit: r.kredit.eller_tomt(),
                            }
                        })
                        .collect()
                })
                .unwrap_or_default(),
            None => Vec::new(),
        };
        // #147 Betalningar: status, betalningarna och panelerna, och betaluppgifterna.
        let mut reskontra = match f.status {
            Status::Utkast => None,
            Status::Registrerad | Status::Upplåst => Some(
                crate::betalning::server::reskontra(
                    b,
                    företag,
                    Faktura::Leverantör(f.id),
                    b.värde("panel").as_deref(),
                )
                .await
                .map_err(|e| match e {
                    BetalningFel::Åtkomst(e) => e,
                    e => ÅtkomstFel::Databas(maya_core::DatabasFel::Korrupt(e.to_string())),
                })?,
            ),
        };
        let mut betaluppgifter = Vec::new();
        if let Some(l) = match (f.status, f.leverantör) {
            (Status::Registrerad, Some(l)) => {
                leverantor::leverantören(db, aktör, företag, l).await?
            }
            _ => None,
        } {
            let u = &l.uppgifter;
            for (etikett, v) in [
                ("Bankgiro", &u.bankgiro),
                ("Plusgiro", &u.plusgiro),
                ("IBAN", &u.iban),
                ("BIC", &u.bic),
            ] {
                if !v.is_empty() {
                    betaluppgifter.push((etikett.to_string(), v.clone()));
                }
            }
        }
        // #148: en kreditfaktura med hänvisning och en upplåst faktura har egna märken.
        if let Some(r) = reskontra.as_mut() {
            let märke = match f.status {
                Status::Upplåst => Some(Märke::Upplåst),
                _ if f.krediterar.is_some() => Some(Märke::Kreditfaktura),
                _ => None,
            };
            if let Some(m) = märke {
                r.märke = (m.kod().into(), m.namn().into());
            }
        }
        let kr = |öre: i64| maya_domain::belopp::Saldo::från_öre(öre).to_string();
        if reskontra.is_some() && !f.formulär.referens.trim().is_empty() {
            betaluppgifter.push((
                "OCR eller meddelande".to_string(),
                f.formulär.referens.trim().to_string(),
            ));
        }
        Ok(FakturaVy {
            id: f.id.to_string(),
            utkast: f.status == Status::Utkast,
            original,
            fält: fält(&f.formulär),
            leverantörsnamn: f.leverantörsnamn.clone(),
            ny_leverantör: f.ny_leverantör,
            verifikat: f.verifikat.map(|(v, n)| (v.to_string(), n.to_string())),
            skapad: (minut(f.skapad.tid), f.skapad.namn.clone()),
            registrerad: f
                .registrerad
                .as_ref()
                .map(|s| (minut(s.tid), s.namn.clone())),
            historik: f
                .historik
                .iter()
                .map(|p| HistorikVy {
                    tid: minut(p.tid),
                    text: p.text(),
                    namn: p.aktörsnamn(),
                })
                .collect(),
            kontering,
            reskontra,
            betaluppgifter,
            upplåst: f.status == Status::Upplåst,
            kreditfaktura: f.total.is_some_and(|t| t < 0),
            krediterar: f
                .krediterar
                .as_ref()
                .map(|k| (k.id.to_string(), k.fakturanummer.clone())),
            krediteras_av: f
                .krediteras_av
                .iter()
                .map(|k| (k.id.to_string(), k.fakturanummer.clone(), kr(k.total)))
                .collect(),
            avskrifter: f
                .avskrifter
                .iter()
                .map(|a| {
                    let länk = |l: &Option<maya_core::verifikat::Länk>| {
                        l.as_ref().map(|l| {
                            (
                                l.id.to_string(),
                                l.nummer.map(|n| n.to_string()).unwrap_or_default(),
                            )
                        })
                    };
                    AvskriftVy {
                        verifikat: länk(&a.verifikat),
                        rättelse: länk(&a.rättelse),
                        orsak: a.orsak.clone(),
                        rättad: (minut(a.rättad.tid), a.rättad.namn.clone()),
                    }
                })
                .collect(),
        })
    }

    type Fält = Vec<(String, String)>;

    fn värde(fält: &Fält, namn: &str) -> Option<String> {
        fält.iter().find(|(k, _)| k == namn).map(|(_, v)| v.clone())
    }

    /// Tillbaka till fakturan med det användaren skrev och felkoden; sidan prövar det igen.
    fn tillbaka(dokumentet: &str, fält: &Fält, fel: &str) -> Response {
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        query.append_pair("tillbaka", "1");
        query.append_pair("fel", fel);
        for (k, v) in super::fält(&formulär(|n| värde(fält, n))) {
            query.append_pair(&k, &v);
        }
        Redirect::to(&format!("{dokumentet}&{}", query.finish())).into_response()
    }

    fn fakturafel(e: FakturaFel, sida: &str) -> Response {
        match e {
            FakturaFel::IngetUtkast => {
                Redirect::to(&format!("{sida}?fel=inget_utkast")).into_response()
            }
            FakturaFel::InteOkopplat => {
                Redirect::to(&format!("{sida}?fel=inte_okopplat")).into_response()
            }
            FakturaFel::Underlag(UnderlagFel::FörStort) => {
                Redirect::to(&format!("{sida}/ny?fel=for_stort")).into_response()
            }
            FakturaFel::Underlag(e) => underlagsfel(e),
            // #148 Rätta avskrift
            FakturaFel::InteRegistrerad => {
                Redirect::to(&format!("{sida}?fel=inte_registrerad")).into_response()
            }
            FakturaFel::Rättelse(_) => {
                Redirect::to(&format!("{sida}?fel=rattelse")).into_response()
            }
            FakturaFel::Åtkomst(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte) => {
                ingen_åtkomst()
            }
            e => serverfel(e),
        }
    }

    /// Laddar upp originalet och skapar utkastet i ett steg.
    async fn ladda_upp(
        State(state): State<Arc<LeverantörsfakturaState>>,
        Path(id): Path<String>,
        inloggad: Option<Extension<Inloggad>>,
        req: Request,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let Ok(företag) = id.parse::<FöretagsId>() else {
            return ingen_åtkomst();
        };
        let sida = format!("/foretag/{företag}/leverantorsfakturor");
        let formulär = match läs_formulär(req).await {
            Ok(f) => f,
            Err(r) => return *r,
        };
        if formulär.för_stort {
            return Redirect::to(&format!("{sida}/ny?fel=for_stort")).into_response();
        }
        let Some(fil) = formulär.fil else {
            return Redirect::to(&format!("{sida}/ny?fel=ingen_fil")).into_response();
        };
        match leverantorsfaktura::ladda_upp(
            &state.db,
            &state.klocka,
            &inloggad.aktör,
            företag,
            (&fil.filnamn, &fil.mime, &fil.innehåll),
        )
        .await
        {
            Ok((f, u)) => {
                let dubblett = if u.dubbletter.is_empty() {
                    ""
                } else {
                    "&dubblett=1"
                };
                Redirect::to(&format!("{sida}?f={f}&uppladdat=1{dubblett}")).into_response()
            }
            Err(e) => fakturafel(e, &sida),
        }
    }

    /// Spara utkast eller Registrera och bokför.
    async fn skicka(
        State(state): State<Arc<LeverantörsfakturaState>>,
        Path((id, f)): Path<(String, String)>,
        inloggad: Option<Extension<Inloggad>>,
        Form(fält): Form<Fält>,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let (Ok(företag), Ok(f)) = (id.parse::<FöretagsId>(), f.parse::<LeverantörsfakturaId>())
        else {
            return ingen_åtkomst();
        };
        let sida = format!("/foretag/{företag}/leverantorsfakturor");
        let dokumentet = format!("{sida}?f={f}");
        let ny = formulär(|n| värde(&fält, n));
        let aktör = &inloggad.aktör;
        if värde(&fält, "handling").as_deref() == Some("registrera") {
            match leverantorsfaktura::registrera(&state.db, &state.klocka, aktör, företag, f, &ny)
                .await
            {
                // #149: under kontantmetoden bokförs registreringen inte.
                Ok(r) => Redirect::to(&format!(
                    "{dokumentet}&registrerad={}",
                    r.verifikat
                        .map_or_else(|| "0".to_string(), |v| v.nummer.to_string())
                ))
                .into_response(),
                Err(FakturaFel::Spärrat(_)) => tillbaka(&dokumentet, &fält, "sparrat"),
                Err(e) => fakturafel(e, &sida),
            }
        } else {
            match leverantorsfaktura::spara_utkast(&state.db, &state.klocka, aktör, företag, f, &ny)
                .await
            {
                Ok(()) => Redirect::to(&format!("{dokumentet}&sparat=1")).into_response(),
                Err(FakturaFel::Spärrat(_)) => tillbaka(&dokumentet, &fält, "utkast"),
                Err(e) => fakturafel(e, &sida),
            }
        }
    }

    /// Rätta avskrift… (#148): vänder registreringen och låser upp fakturan.
    async fn rätta_avskrift(
        State(state): State<Arc<LeverantörsfakturaState>>,
        Path((id, f)): Path<(String, String)>,
        inloggad: Option<Extension<Inloggad>>,
        Form(fält): Form<Fält>,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let (Ok(företag), Ok(f)) = (id.parse::<FöretagsId>(), f.parse::<LeverantörsfakturaId>())
        else {
            return ingen_åtkomst();
        };
        let sida = format!("/foretag/{företag}/leverantorsfakturor");
        let orsak = värde(&fält, "orsak").unwrap_or_default();
        match leverantorsfaktura::rätta_avskrift(
            &state.db,
            &state.klocka,
            &inloggad.aktör,
            företag,
            f,
            &orsak,
        )
        .await
        {
            Ok(r) => {
                let nummer = r
                    .rättelse
                    .map(|b| b.nummer.to_string())
                    .unwrap_or_else(|| "1".into());
                Redirect::to(&format!("{sida}?f={f}&avskrift_rattad={nummer}")).into_response()
            }
            Err(FakturaFel::InteRegistrerad) => {
                Redirect::to(&format!("{sida}?f={f}&fel=inte_registrerad")).into_response()
            }
            Err(FakturaFel::Rättelse(_)) => {
                Redirect::to(&format!("{sida}?f={f}&panel=ratta-avskrift&fel=rattelse"))
                    .into_response()
            }
            Err(e) => fakturafel(e, &sida),
        }
    }

    async fn radera(
        State(state): State<Arc<LeverantörsfakturaState>>,
        Path((id, f)): Path<(String, String)>,
        inloggad: Option<Extension<Inloggad>>,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let (Ok(företag), Ok(f)) = (id.parse::<FöretagsId>(), f.parse::<LeverantörsfakturaId>())
        else {
            return ingen_åtkomst();
        };
        let sida = format!("/foretag/{företag}/leverantorsfakturor");
        match leverantorsfaktura::radera_utkast(
            &state.db,
            &state.klocka,
            &inloggad.aktör,
            företag,
            f,
        )
        .await
        {
            Ok(()) => Redirect::to(&format!("{sida}?raderat=1")).into_response(),
            Err(e) => fakturafel(e, &sida),
        }
    }
}

/// Länken till leverantörsregistret, för fliken.
pub fn leverantörsregistret(bas: &str) -> String {
    format!("{bas}/leverantorer")
}

/// Fliken Leverantörsfakturor: listan och, när en faktura är vald, fakturan.
#[component]
pub fn LeverantörsfakturorSida() -> impl IntoView {
    sida(hämta_leverantörsfakturadata, |d| {
        leverantörsfakturasida(d, false)
    })
}

/// Uppladdningen av en ny leverantörsfaktura, bredvid listan.
#[component]
pub fn NyLeverantörsfakturaSida() -> impl IntoView {
    sida(hämta_leverantörsfakturadata, |d| {
        leverantörsfakturasida(d, true)
    })
}

fn leverantörsfakturasida(data: Leverantörsfakturadata, ny: bool) -> AnyView {
    if !inloggad() {
        return view! { <LoggaIn /> }.into_any();
    }
    let Some(bas) = företagets_bas() else {
        return view! { <IngenÅtkomst /> }.into_any();
    };
    let fråga = |namn: &str| use_query_map().with_untracked(|q| q.get(namn));
    let notis = if let Some(nummer) = fråga("registrerad") {
        Some(if nummer == "0" {
            "Registrerad. Under kontantmetoden bokförs fakturan när den betalas.".to_string()
        } else {
            format!("Registrerad och bokförd som {nummer}.")
        })
    } else if fråga("sparat").is_some() {
        Some("Utkastet sparades.".into())
    } else if fråga("betald").is_some() {
        // #147 Betalningar
        Some("Betalningen registrerades och verifikatet bokfördes.".into())
    } else if let Some(nummer) = fråga("avskrift_rattad") {
        // #148 Rätta avskrift
        Some(if nummer == "1" {
            "Avskriften rättades. Fakturan är upplåst; skriv av den och registrera om.".to_string()
        } else {
            format!(
                "Avskriften rättades. Registreringen vändes med {nummer}, och fakturan är upplåst; \
                 skriv av den och registrera om."
            )
        })
    } else if fråga("aterbetald").is_some() {
        Some("Återbetalningen registrerades och verifikatet bokfördes.".into())
    } else if fråga("betalning_rattad").is_some() {
        Some("Betalningen rättades. Rättelsen bokfördes och betalningen räknas inte längre.".into())
    } else if fråga("raderat").is_some() {
        Some(
            "Utkastet raderades och raderingen loggades. Originalet är ett okopplat underlag igen."
                .into(),
        )
    } else if fråga("uppladdat").is_some() || fråga("skapat").is_some() {
        Some("Ett utkast skapades med originalet kopplat. Skriv av fakturan.".into())
    } else {
        None
    }
    .map(|text| view! { <p class="notis" role="status">{text}</p> });
    let dubblett = fråga("dubblett").map(|_| {
        view! {
            <p class="notis varningsnotis" role="status">
                "Samma fil finns redan i företaget. Den laddades upp ändå."
            </p>
        }
    });
    let fel = fråga("fel")
        .and_then(|k| feltext(&k))
        .map(|text| view! { <p class="fel" role="alert">{text}</p> });
    let valt_id = data.valt.as_ref().map(|f| f.id.clone());
    let lista = Lista(&bas, &data, valt_id.as_deref());
    let detalj = if ny {
        view! { <NyFaktura bas=bas.clone() okopplade=data.okopplade.clone() /> }.into_any()
    } else {
        match data.valt.clone() {
            Some(f) if f.utkast || f.upplåst => view! {
                <Utkast bas=bas.clone() f=f förutsättningar=data.förutsättningar.clone() />
            }
            .into_any(),
            Some(f) => view! { <Registrerad bas=bas.clone() f=f /> }.into_any(),
            None => view! {
                <div class="tom">
                    <p>"Välj en faktura i listan, eller ladda upp en ny."</p>
                </div>
            }
            .into_any(),
        }
    };
    view! {
        <div class="verifikat-delad">
            <div class="verifikat-spalt">
                <div class="verifikat-spalt-rubrik">
                    <div>
                        <h1>"Leverantörsfakturor"</h1>
                        <p class="diskret leverantorsskulder">
                            {format!("Leverantörsskulder {} kr", data.leverantörsskulder)}
                        </p>
                    </div>
                    <span>
                        <a class="knapp knapp-diskret leverantorsregister" href=leverantörsregistret(&bas)>
                            "Leverantörer"
                        </a>
                        " "
                        <a class="knapp" href=format!("{bas}/leverantorsfakturor/ny")>
                            "Ny"
                        </a>
                    </span>
                </div>
                {lista}
            </div>
            <div class="verifikat-detalj">{notis} {dubblett} {fel} {detalj}</div>
        </div>
    }
    .into_any()
}

/// Listan i grupper (#147, som kundfakturorna): Förfallna, Förfaller inom 7 dagar, Senare,
/// Utkast och Avslutade, med märket och det som återstår på en registrerad faktura.
#[allow(non_snake_case)]
fn Lista(bas: &str, data: &Leverantörsfakturadata, valt: Option<&str>) -> AnyView {
    if data.lista.is_empty() {
        return view! { <p class="diskret">"Inga leverantörsfakturor än."</p> }.into_any();
    }
    let rad = |r: &ListradVy| {
        let aktuellt = (Some(r.id.as_str()) == valt).then_some("page");
        let namn = if r.leverantör.is_empty() {
            "Ny leverantörsfaktura".to_string()
        } else {
            r.leverantör.clone()
        };
        let förfaller = match (r.förfallotext.is_empty(), r.förfallodag.is_empty()) {
            (false, _) => r.förfallotext.clone(),
            (true, false) => format!("förfaller {}", r.förfallodag),
            (true, true) => String::new(),
        };
        let under = [r.fakturanummer.clone(), förfaller]
            .into_iter()
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join(" · ");
        let märke = match (&r.märke, r.utkast) {
            (_, true) => view! { <UtkastBadge /> }.into_any(),
            (Some((kod, namn)), _) => {
                view! { <Märke kod=kod.clone() namn=namn.clone() /> }.into_any()
            }
            (None, _) => ().into_any(),
        };
        // Det som återstår för en öppen faktura, annars totalen.
        let belopp = if r.förfallotext.is_empty() {
            r.total.clone()
        } else {
            r.återstår.clone()
        };
        let ny = r.ny_leverantör.then(|| {
            view! {
                <span class="badge badge-ny" title="Blir verklig när fakturan registreras">
                    "Ny leverantör"
                </span>
            }
        });
        view! {
            <li>
                <a href=format!("{bas}/leverantorsfakturor?f={}", r.id) aria-current=aktuellt>
                    <span class="verifikatlista-topp">
                        <span>{namn}</span>
                        <span class="belopp">{belopp}</span>
                    </span>
                    <span class="diskret">{märke} " " {under} " " {ny}</span>
                </a>
            </li>
        }
    };
    let grupper = Grupp::ALLA
        .iter()
        .filter_map(|g| {
            let mut i_gruppen: Vec<&ListradVy> =
                data.lista.iter().filter(|r| r.grupp == Some(*g)).collect();
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
                    <h2 class="per lf-grupp">
                        {g.rubrik()} <span class="diskret">{format!(" · {antal}")}</span>
                    </h2>
                    <ul class="verifikatlista">{rader}</ul>
                </li>
            })
        })
        .collect_view();
    view! { <ul class="listgrupper">{grupper}</ul> }.into_any()
}

/// En ny faktura: ladda upp originalet, eller välj ett okopplat underlag.
#[component]
fn NyFaktura(bas: String, okopplade: Vec<UnderlagVy>) -> impl IntoView {
    let val = okopplade
        .into_iter()
        .map(|u| {
            view! {
                <form
                    class="ufil"
                    method="post"
                    action=format!("{bas}/att-gora/underlag/{}/leverantorsfaktura", u.id)
                >
                    <Gem />
                    <span>{u.filnamn}</span>
                    <button class="knapp" type="submit">
                        "Välj"
                    </button>
                </form>
            }
        })
        .collect_view();
    view! {
        <section class="formular">
            <h2>"Ny leverantörsfaktura"</h2>
            <p class="diskret">
                "Originalet arkiveras som underlag med SHA-256 och visas bredvid fälten medan du skriver av fakturan."
            </p>
            <form
                class="uppladdning"
                method="post"
                action=format!("{bas}/leverantorsfakturor")
                enctype="multipart/form-data"
            >
                <label class="slapp">
                    <span>"Släpp originalet här eller välj en fil"</span>
                    <input type="file" name="fil" required />
                </label>
                <button class="knapp knapp-primar" type="submit">
                    "Ladda upp"
                </button>
            </form>
            <h3>"Eller välj ett okopplat underlag"</h3>
            <div class="ufiler">{val}</div>
        </section>
    }
}

/// Ett utkast: originalet till vänster och formuläret till höger.
#[component]
fn Utkast(bas: String, f: FakturaVy, förutsättningar: FörutsättningarVy) -> impl IntoView {
    let fråga = |namn: &str| use_query_map().with_untracked(|q| q.get(namn));
    let tillbaka = fråga("tillbaka").is_some();
    let mut start = if tillbaka {
        formulär(|n| fråga(n))
    } else {
        formulär(|n| f.fält.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone()))
    };
    let antal = start.rader.len().max(3);
    start.rader.resize(
        antal,
        NyRad {
            momssats: "25".into(),
            ..NyRad::default()
        },
    );
    for r in &mut start.rader {
        if r.momssats.trim().is_empty() {
            r.momssats = "25".into();
        }
    }
    let radera = fråga("radera").is_some();
    let här = format!("{bas}/leverantorsfakturor?f={}", f.id);
    let dialog = radera.then(|| {
        view! {
            <section class="dialog" role="alertdialog" aria-labelledby="radera-rubrik">
                <h2 id="radera-rubrik">"Radera utkastet?"</h2>
                <p class="diskret">
                    "Raderingen loggas. Originalet blir ett okopplat underlag igen, och en ny leverantör som bara hörde till utkastet raderas."
                </p>
                <form method="post" action=format!("{bas}/leverantorsfakturor/{}/radera", f.id)>
                    <a class="knapp" href=här.clone()>
                        "Avbryt"
                    </a>
                    " "
                    <button class="knapp knapp-fara" type="submit">
                        "Radera utkast"
                    </button>
                </form>
            </section>
        }
    });
    view! {
        <div class="med-underlag">
            <aside class="underlagsspalt" aria-label="Originalet">
                <Förhandsvisning bas=bas.clone() u=f.original.clone() />
            </aside>
            <div>
                {dialog}
                <Formulär
                    bas=bas.clone()
                    id=f.id.clone()
                    start=start
                    förutsättningar=förutsättningar
                    skapad=f.skapad.clone()
                    upplåst=f.upplåst
                />
            </div>
        </div>
    }
}

/// Formuläret för ett utkast. Spärrarna visas vid sina fält medan användaren skriver, och
/// Registrera och bokför är inaktiv med antalet problem kvar. Spara utkast går alltid att trycka på.
#[component]
fn Formulär(
    bas: String,
    id: String,
    start: NyLeverantörsfaktura,
    förutsättningar: FörutsättningarVy,
    skapad: (String, String),
    /// Registreringen är vänd med Rätta avskrift (#148): fakturan registreras om och raderas inte.
    upplåst: bool,
) -> impl IntoView {
    let faktura = RwSignal::new(start.clone());
    let fö = förutsättningar.förutsättningar();
    let spärrar = {
        let fö = fö.clone();
        Memo::new(move |_| faktura.with(|f| f.kontrollera(&fö).err().unwrap_or_default()))
    };
    let varningar = {
        let fö = fö.clone();
        Memo::new(move |_| faktura.with(|f| f.varningar(&fö)))
    };
    let kontering = {
        let fö = fö.clone();
        Memo::new(move |_| faktura.with(|f| f.förhandsvisning(&fö)))
    };
    let standardkonto = {
        let fö = fö.clone();
        Memo::new(move |_| {
            faktura.with(|f| {
                f.standardkonto(&fö)
                    .map(|k| k.get().to_string())
                    .unwrap_or_default()
            })
        })
    };
    let hydrerad = RwSignal::new(false);
    Effect::new(move |_| hydrerad.set(true));
    let registrera = move |ev: leptos::ev::MouseEvent| {
        if !spärrar.with_untracked(Vec::is_empty) {
            ev.prevent_default();
        }
    };
    let spärr_vid = {
        let bas = bas.clone();
        move |fält: Fält| {
            let bas = bas.clone();
            move || {
                spärrar.with(|s| {
                    s.iter()
                        .filter(|s| s.fält == fält)
                        .map(|s: &Spärr| {
                            let åtgärd = match &s.åtgärd {
                                Some(Åtgärd::Aktivera(konto)) => view! {
                                    " "
                                    <a href=format!(
                                        "{bas}/installningar/kontoplan?sok={}",
                                        konto.get(),
                                    )>"Aktivera"</a>
                                }
                                .into_any(),
                                Some(Åtgärd::Fakturanummer(nummer)) => {
                                    let nummer = nummer.clone();
                                    let suffix = nummer
                                        .rsplit_once('-')
                                        .map(|(_, s)| format!("-{s}"))
                                        .unwrap_or_default();
                                    view! {
                                        " "
                                        <button
                                            class="knapp knapp-diskret"
                                            type="button"
                                            on:click=move |_| {
                                                faktura
                                                    .update(|f| f.fakturanummer = nummer.clone())
                                            }
                                        >
                                            {format!("Lägg till suffix {suffix}")}
                                        </button>
                                    }
                                    .into_any()
                                }
                                None => ().into_any(),
                            };
                            view! {
                                <p class="fel" role="alert" data-kod=s.kod>
                                    {s.text.clone()}
                                    {åtgärd}
                                </p>
                            }
                        })
                        .collect_view()
                })
            }
        }
    };
    // #149: under kontantmetoden på fakturadatumet heter knappen Registrera.
    let kontant = {
        let metoder = förutsättningar.bokföringsmetoder.clone();
        Memo::new(move |_| {
            faktura.with(|f| {
                // En kreditfaktura utan hänvisning bokförs alltid, och en med hänvisning
                // följer originalets skuld.
                !f.totalbelopp.trim().starts_with(['-', '−'])
                    && crate::betalning::kontantmetoden_på(&metoder, &f.fakturadatum)
            })
        })
    };
    let registrera_text = move || {
        let knapp = if kontant.get() {
            "Registrera"
        } else {
            "Registrera och bokför"
        };
        match spärrar.with(Vec::len) {
            0 => knapp.to_string(),
            1 => format!("{knapp} · 1 problem kvar"),
            n => format!("{knapp} · {n} problem kvar"),
        }
    };
    let värde = move |f: fn(&NyLeverantörsfaktura) -> String| faktura.with_untracked(f);
    let textfält = {
        let spärr_vid = spärr_vid.clone();
        move |namn: &'static str,
              etikett: &'static str,
              typ: &'static str,
              fält: Option<Fält>,
              läs: fn(&NyLeverantörsfaktura) -> String,
              sätt: fn(&mut NyLeverantörsfaktura, String)| {
            let fel = fält.map(&spärr_vid);
            view! {
                <div>
                    <label for=namn>{etikett}</label>
                    <input
                        id=namn
                        name=namn
                        type=typ
                        prop:value=move || faktura.with(läs)
                        value=värde(läs)
                        on:input=move |ev| faktura.update(|f| sätt(f, event_target_value(&ev)))
                    />
                    {fel}
                </div>
            }
        }
    };
    let leverantörer = förutsättningar.leverantörer.clone();
    let vald = start.leverantör.clone();
    let val = leverantörer
        .iter()
        .map(|(id, namn, _, _)| {
            view! {
                <option value=id.clone() selected=*id == vald>
                    {namn.clone()}
                </option>
            }
        })
        .collect_view();
    let ny_vald = Memo::new(move |_| faktura.with(|f| f.leverantör.trim() == NY));
    let nya_fält = NYA_FÄLT
        .into_iter()
        .map(|(namn, etikett)| {
            let läs = move |f: &NyLeverantörsfaktura| ny_fält(&f.ny_leverantör, namn).clone();
            let fel = spärr_vid(Fält::NyLeverantör(ny_fältet(namn)));
            view! {
                <div>
                    <label for=namn>{etikett}</label>
                    <input
                        id=namn
                        name=namn
                        type="text"
                        value=faktura.with_untracked(läs)
                        on:input=move |ev| {
                            faktura
                                .update(|f| {
                                    *ny_fält_mut(&mut f.ny_leverantör, namn) = event_target_value(&ev)
                                })
                        }
                    />
                    {fel}
                </div>
            }
        })
        .collect_view();
    let antal = Memo::new(move |_| faktura.with(|f| f.rader.len()));
    let rader = {
        let spärr_vid = spärr_vid.clone();
        let förutsättningar = förutsättningar.clone();
        move || {
            (0..antal.get())
                .map(|i| {
                    let fält = move |f: fn(&NyRad) -> &String| {
                        faktura.with_untracked(|v| f(&v.rader[i]).clone())
                    };
                    let sats = fält(|r| &r.momssats);
                    let satser = MOMSSATSER
                        .into_iter()
                        .map(|s| {
                            view! {
                                <option value=s.to_string() selected=s.to_string() == sats>
                                    {format!("{s} %")}
                                </option>
                            }
                        })
                        .collect_view();
                    let förutsättningar = förutsättningar.clone();
                    let kontonamn = move || {
                        // Raderna kan bli färre (Kontera som originalet, #148).
                        let konto = faktura
                            .with(|f| f.rader.get(i).map(|r| r.konto.clone()).unwrap_or_default());
                        let konto = if konto.trim().is_empty() {
                            standardkonto.get()
                        } else {
                            konto
                        };
                        förutsättningar.kontonamn(&konto)
                    };
                    let beräknad = move || {
                        faktura.with(|f| {
                            let Some(r) = f.rader.get(i) else {
                                return String::new();
                            };
                            let netto = tolka_med_tecken(&r.netto);
                            let sats = r.momssats.trim().parse::<u8>().ok();
                            match (netto, sats) {
                                (Some(n), Some(s)) => Saldo::från_öre(moms_med_tecken(n, s)).to_string(),
                                _ => String::new(),
                            }
                        })
                    };
                    view! {
                        <tr>
                            <td>
                                <input
                                    name=format!("konto_{i}")
                                    type="text"
                                    inputmode="numeric"
                                    list="konton"
                                    aria-label="Konto"
                                    placeholder=move || standardkonto.get()
                                    prop:value=move || faktura.with(|f| f.rader.get(i).map(|r| r.konto.clone()).unwrap_or_default())
                                    value=fält(|r| &r.konto)
                                    on:input=move |ev| {
                                        faktura.update(|f| f.rader[i].konto = event_target_value(&ev))
                                    }
                                />
                                {spärr_vid(Fält::Konto(i))}
                            </td>
                            <td class="diskret">{kontonamn}</td>
                            <td>
                                <input
                                    name=format!("netto_{i}")
                                    type="text"
                                    inputmode="decimal"
                                    class="belopp"
                                    aria-label="Netto"
                                    prop:value=move || faktura.with(|f| f.rader.get(i).map(|r| r.netto.clone()).unwrap_or_default())
                                    value=fält(|r| &r.netto)
                                    on:input=move |ev| {
                                        faktura.update(|f| f.rader[i].netto = event_target_value(&ev))
                                    }
                                />
                                {spärr_vid(Fält::Netto(i))}
                            </td>
                            <td>
                                <select
                                    name=format!("momssats_{i}")
                                    aria-label="Momssats"
                                    prop:value=move || faktura.with(|f| f.rader.get(i).map(|r| r.momssats.clone()).unwrap_or_default())
                                    on:change=move |ev| {
                                        faktura
                                            .update(|f| f.rader[i].momssats = event_target_value(&ev))
                                    }
                                >
                                    {satser}
                                </select>
                                {spärr_vid(Fält::Momssats(i))}
                            </td>
                            <td>
                                <input
                                    name=format!("moms_{i}")
                                    type="text"
                                    inputmode="decimal"
                                    class="belopp"
                                    aria-label="Momsbelopp"
                                    title="Ändra om fakturans moms avviker"
                                    placeholder=beräknad
                                    prop:value=move || faktura.with(|f| f.rader.get(i).map(|r| r.moms.clone()).unwrap_or_default())
                                    value=fält(|r| &r.moms)
                                    on:input=move |ev| {
                                        faktura.update(|f| f.rader[i].moms = event_target_value(&ev))
                                    }
                                />
                                {spärr_vid(Fält::Moms(i))}
                            </td>
                        </tr>
                    }
                })
                .collect_view()
        }
    };
    let lägg_till_rad = move |_| {
        faktura.update(|f| {
            f.rader.push(NyRad {
                momssats: "25".into(),
                ..NyRad::default()
            })
        })
    };
    let omvänd = {
        let fö = fö.clone();
        Memo::new(move |_| faktura.with(|f| f.omvänd_skattskyldighet(&fö)))
    };
    // Valet visas när företaget är momsregistrerat på fakturadatumet, eller när fakturan redan
    // har ett val som då spärrar.
    let visa_omvänd = {
        let fö = fö.clone();
        Memo::new(move |_| {
            faktura.with(|f| fö.momsregistrerat_på(&f.fakturadatum))
                || omvänd.get() != OmvändSkattskyldighet::Ingen
        })
    };
    let omvänd_val = {
        let spärr = spärr_vid(Fält::OmvändSkattskyldighet);
        move || {
            visa_omvänd.get().then(|| {
                let alternativ = OmvändSkattskyldighet::ALLA
                    .into_iter()
                    .map(|o| {
                        view! {
                            <option value=o.kod() selected=move || omvänd.get() == o>
                                {o.namn()}
                            </option>
                        }
                    })
                    .collect_view();
                view! {
                    <div>
                        <label for="omvand_skattskyldighet">"Omvänd skattskyldighet"</label>
                        <select
                            id="omvand_skattskyldighet"
                            name="omvand_skattskyldighet"
                            on:change=move |ev| {
                                faktura.update(|f| f.omvänd_skattskyldighet = event_target_value(&ev))
                            }
                        >
                            {alternativ}
                        </select>
                        {spärr.clone()}
                    </div>
                }
            })
        }
    };
    let momsrubrik = move || {
        if omvänd.get() == OmvändSkattskyldighet::Ingen {
            "Momsbelopp"
        } else {
            "Fiktiv moms"
        }
    };
    let summerar = move || {
        let omvänd = omvänd.get() != OmvändSkattskyldighet::Ingen;
        faktura.with(|f| {
            let summa: i64 = f
                .rader
                .iter()
                .filter_map(|r| {
                    let netto = tolka_med_tecken(&r.netto)?;
                    let sats = r.momssats.trim().parse::<u8>().ok()?;
                    let m = match r.moms.trim() {
                        "" => moms_med_tecken(netto, sats),
                        t => tolka_med_tecken(t)?,
                    };
                    Some(netto + if omvänd { 0 } else { m })
                })
                .sum();
            match tolka_med_tecken(&f.totalbelopp) {
                Some(t) if t == summa && summa != 0 => {
                    view! { <span class="diff-ok">"Summerar till totalen"</span> }.into_any()
                }
                Some(_) => view! {
                    <span class="diff-bad">
                        {format!(
                            "{}: {} kr",
                            if omvänd { "Raderna" } else { "Raderna med moms" },
                            Saldo::från_öre(summa),
                        )}
                    </span>
                }
                .into_any(),
                None => ().into_any(),
            }
        })
    };
    let bokförs_som = {
        let förutsättningar = förutsättningar.clone();
        move || {
            let rubrik = faktura
                .with(|f| förutsättningar.bokförs_som(&f.fakturadatum))
                .map(|s| format!("Bokförs som {s}"))
                .unwrap_or_else(|| "Bokförs som …".into());
            let rader = kontering.with(|k| {
                k.iter()
                    .map(|r| {
                        let konto = r.konto.get().to_string();
                        view! {
                            <tr>
                                <td>{konto.clone()}</td>
                                <td class="diskret">{förutsättningar.kontonamn(&konto)}</td>
                                <td class="belopp">{r.debet.eller_tomt()}</td>
                                <td class="belopp">{r.kredit.eller_tomt()}</td>
                            </tr>
                        }
                    })
                    .collect_view()
            });
            if kontant.get() {
                return view! {
                    <h3>"Bokförs vid betalningen"</h3>
                    <p class="diskret">
                        "Under kontantmetoden bokförs inget när fakturan registreras. \
                         Betalningen bokför kostnaden och momsen."
                    </p>
                }
                .into_any();
            }
            view! {
                <h3>{rubrik}</h3>
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
            }
            .into_any()
        }
    };
    let varningslista = move || {
        varningar.with(|v| {
            v.iter()
                .map(|v| {
                    view! {
                        <p class="notis varningsnotis" role="status" data-kod=v.kod>
                            {v.text.clone()}
                        </p>
                    }
                })
                .collect_view()
        })
    };
    let ny_info = move || {
        ny_vald.get().then(|| {
            view! {
                <p class="notis" role="status">
                    "Den nya leverantören läggs i leverantörsregistret när fakturan registreras."
                </p>
            }
        })
    };
    // #148: hänvisningen till originalet för en kreditfaktura, med leverantörens registrerade
    // fakturor, och Kontera som originalet som fyller raderna proportionellt.
    let hänvisning = {
        let krediterbara = förutsättningar.krediterbara.clone();
        let spärr = spärr_vid(Fält::Krediterar);
        let fö = fö.clone();
        move || {
            let leverantör = faktura.with(|f| f.leverantör.trim().to_string());
            let vald = faktura.with_untracked(|f| f.krediterar.trim().to_string());
            let val: Vec<KrediterbarVy> = krediterbara
                .iter()
                .filter(|k| k.leverantör == leverantör)
                .cloned()
                .collect();
            if val.is_empty() && vald.is_empty() {
                return None;
            }
            let alternativ = val
                .into_iter()
                .map(|k| {
                    view! {
                        <option value=k.id.clone() selected=k.id == vald>
                            {format!(
                                "Faktura {} · {} kr",
                                k.fakturanummer,
                                Saldo::från_öre(k.total),
                            )}
                        </option>
                    }
                })
                .collect_view();
            let fö = fö.clone();
            let kontera = move |_| {
                faktura.update(|f| {
                    if let Some(ny) = f.kontera_som_originalet(&fö) {
                        *f = ny;
                    }
                })
            };
            Some(view! {
                <div class="bred hanvisning-val">
                    <label for="krediterar">"Krediterar (kreditfaktura)"</label>
                    <select
                        id="krediterar"
                        name="krediterar"
                        on:change=move |ev| faktura.update(|f| f.krediterar = event_target_value(&ev))
                    >
                        <option value="" selected=vald.is_empty()>
                            "Ingen hänvisning"
                        </option>
                        {alternativ}
                    </select>
                    " "
                    <button
                        class="knapp knapp-diskret"
                        type="button"
                        on:click=kontera
                        title="Fyller raderna ur originalet, proportionellt mot den negativa totalen"
                    >
                        "Kontera som originalet"
                    </button>
                    {spärr.clone()}
                </div>
            })
        }
    };
    let betalt = förutsättningar.betalt;
    let upplåst_info = upplåst.then(|| {
        let text = if betalt == 0 {
            "Upplåst med Rätta avskrift. Skriv av fakturan och registrera om den.".to_string()
        } else {
            format!(
                "Upplåst med Rätta avskrift. {} kr är redan betalt, och totalen får inte bli lägre.",
                Saldo::från_öre(betalt)
            )
        };
        view! { <p class="notis" role="status">{text}</p> }
    });
    let kreditfaktura = Memo::new(move |_| {
        faktura.with(|f| tolka_med_tecken(&f.totalbelopp).is_some_and(|t| t < 0))
    });
    let aktiva = förutsättningar
        .konton
        .iter()
        .filter(|k| k.aktiv)
        .map(|k| view! { <option value=k.nummer.to_string() label=k.namn.clone()></option> })
        .collect_view();
    let rubrik = move || {
        faktura.with(|f| {
            let namn = if f.leverantör.trim() == NY {
                f.ny_leverantör.namn.trim().to_string()
            } else {
                förutsättningar
                    .leverantörer
                    .iter()
                    .find(|(id, _, _, _)| *id == f.leverantör.trim())
                    .map(|(_, n, _, _)| n.clone())
                    .unwrap_or_default()
            };
            if namn.is_empty() {
                "Ny leverantörsfaktura".to_string()
            } else {
                namn
            }
        })
    };
    view! {
        <form
            id="leverantorsfakturaformular"
            class="lf-formular"
            method="post"
            action=format!("{bas}/leverantorsfakturor/{id}")
            novalidate
        >
            <p class="dokument-nummer">
                {move || if kreditfaktura.get() { "Kreditfaktura" } else { "Leverantörsfaktura" }}
            </p>
            <h2>{rubrik}</h2>
            <p>
                {if upplåst {
                    view! { <span class="badge badge-upplast">"Upplåst"</span> }.into_any()
                } else {
                    view! { <UtkastBadge /> }.into_any()
                }}
                " "
                <span class="diskret">"Skapat av " {skapad.1.clone()} " · " {skapad.0.clone()}</span>
            </p>
            <div class="falt-rad">
                <div class="bred">
                    <label for="leverantor">"Leverantör"</label>
                    <select
                        id="leverantor"
                        name="leverantor"
                        on:change=move |ev| {
                            faktura
                                .update(|f| {
                                    f.leverantör = event_target_value(&ev);
                                    f.omvänd_skattskyldighet.clear();
                                })
                        }
                    >
                        <option value="" selected=start.leverantör.is_empty()>
                            "Välj leverantör…"
                        </option>
                        {val}
                        <option value=NY selected=start.leverantör == NY>
                            "+ Ny leverantör…"
                        </option>
                    </select>
                    {spärr_vid(Fält::Leverantör)}
                    <p class="diskret">
                        {move || {
                            let k = standardkonto.get();
                            if k.is_empty() { String::new() } else { format!("Standardkonto {k}") }
                        }}
                    </p>
                </div>
            </div>
            <div class="kort-streckat" class:dold=move || !ny_vald.get()>
                <b>"Ny leverantör"</b>
                <div class="lf-rutnat">{nya_fält}</div>
                <p class="diskret">
                    "Läggs i leverantörsregistret när fakturan registreras. Raderas utkastet försvinner förslaget."
                </p>
            </div>
            <div class="lf-rutnat">
                {textfält(
                    "fakturanummer",
                    "Fakturanummer",
                    "text",
                    Some(Fält::Fakturanummer),
                    |f| f.fakturanummer.clone(),
                    |f, v| f.fakturanummer = v,
                )}
                {textfält(
                    "fakturadatum",
                    "Fakturadatum",
                    "date",
                    Some(Fält::Fakturadatum),
                    |f| f.fakturadatum.clone(),
                    |f, v| f.fakturadatum = v,
                )}
                {textfält(
                    "forfallodag",
                    "Förfallodag",
                    "date",
                    Some(Fält::Förfallodag),
                    |f| f.förfallodag.clone(),
                    |f, v| f.förfallodag = v,
                )}
                {textfält(
                    "referens",
                    "OCR eller meddelande",
                    "text",
                    None,
                    |f| f.referens.clone(),
                    |f, v| f.referens = v,
                )}
                {textfält(
                    "totalbelopp",
                    "Totalbelopp, SEK",
                    "text",
                    Some(Fält::Totalbelopp),
                    |f| f.totalbelopp.clone(),
                    |f, v| f.totalbelopp = v,
                )}
                {omvänd_val}
                {hänvisning}
                {textfält(
                    "originalvaluta",
                    "Originalvaluta (fritext)",
                    "text",
                    None,
                    |f| f.originalvaluta.clone(),
                    |f, v| f.originalvaluta = v,
                )}
            </div>
            {upplåst_info}
            <datalist id="konton">{aktiva}</datalist>
            <table class="rader">
                <thead>
                    <tr>
                        <th>"Konto"</th>
                        <th>"Kontonamn"</th>
                        <th class="belopp">"Netto"</th>
                        <th>"Moms"</th>
                        <th class="belopp">{momsrubrik}</th>
                    </tr>
                </thead>
                <tbody>{rader}</tbody>
            </table>
            <p>
                <button class="knapp knapp-diskret" type="button" on:click=lägg_till_rad>
                    "+ Rad"
                </button>
                " "
                {summerar}
            </p>
            {spärr_vid(Fält::Rader)}
            {spärr_vid(Fält::Kontering)}
            {varningslista}
            {ny_info}
            <section class="bokfors-som">{bokförs_som}</section>
            <p class="knappar">
                <button
                    class="knapp knapp-primar"
                    type="submit"
                    name="handling"
                    value="registrera"
                    on:click=registrera
                    disabled=move || hydrerad.get() && !spärrar.with(Vec::is_empty)
                >
                    {registrera_text}
                </button>
                <button class="knapp" type="submit" name="handling" value="utkast">
                    {if upplåst { "Spara" } else { "Spara utkast" }}
                </button>
                {(!upplåst)
                    .then(|| {
                        view! {
                            <a
                                class="knapp knapp-fara-diskret"
                                href=format!("{bas}/leverantorsfakturor?f={id}&radera=1")
                            >
                                "Radera utkast"
                            </a>
                        }
                    })}
            </p>
            <p class="diskret">
                "Registrera låser fakturan och bokför den på fakturadatumet med originalet som underlag."
            </p>
        </form>
    }
}

fn ny_fältet(namn: &str) -> leverantor::Fält {
    match namn {
        "ny_namn" => leverantor::Fält::Namn,
        "ny_orgnr" => leverantor::Fält::Orgnr,
        "ny_momsregnr" => leverantor::Fält::Momsregnr,
        "ny_gatuadress" => leverantor::Fält::Gatuadress,
        "ny_postnummer" => leverantor::Fält::Postnummer,
        "ny_postort" => leverantor::Fält::Postort,
        "ny_bankgiro" => leverantor::Fält::Bankgiro,
        "ny_plusgiro" => leverantor::Fält::Plusgiro,
        _ => leverantor::Fält::Standardkonto,
    }
}

fn ny_fält<'a>(l: &'a NyLeverantör, namn: &str) -> &'a String {
    match namn {
        "ny_namn" => &l.namn,
        "ny_orgnr" => &l.orgnr,
        "ny_momsregnr" => &l.momsregnr,
        "ny_gatuadress" => &l.gatuadress,
        "ny_postnummer" => &l.postnummer,
        "ny_postort" => &l.postort,
        "ny_bankgiro" => &l.bankgiro,
        "ny_plusgiro" => &l.plusgiro,
        _ => &l.standardkonto,
    }
}

fn ny_fält_mut<'a>(l: &'a mut NyLeverantör, namn: &str) -> &'a mut String {
    match namn {
        "ny_namn" => &mut l.namn,
        "ny_orgnr" => &mut l.orgnr,
        "ny_momsregnr" => &mut l.momsregnr,
        "ny_gatuadress" => &mut l.gatuadress,
        "ny_postnummer" => &mut l.postnummer,
        "ny_postort" => &mut l.postort,
        "ny_bankgiro" => &mut l.bankgiro,
        "ny_plusgiro" => &mut l.plusgiro,
        _ => &mut l.standardkonto,
    }
}

/// En registrerad faktura: originalet till vänster och uppgifterna, konteringen och historiken
/// till höger.
#[component]
fn Registrerad(bas: String, f: FakturaVy) -> impl IntoView {
    let värde = |namn: &str| {
        f.fält
            .iter()
            .find(|(k, _)| k == namn)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    };
    let rad = |etikett: &'static str, v: String| {
        (!v.trim().is_empty()).then(|| {
            view! {
                <dt>{etikett}</dt>
                <dd>{v}</dd>
            }
        })
    };
    // #148: hänvisningarna åt båda hållen och upplåsningarna.
    let krediterar = f.krediterar.clone().map(|(id, nummer)| {
        view! {
            <dt>"Krediterar"</dt>
            <dd>
                <a href=format!("{bas}/leverantorsfakturor?f={id}")>{format!("Faktura {nummer}")}</a>
            </dd>
        }
    });
    let krediteras_av = (!f.krediteras_av.is_empty()).then(|| {
        let länkar = f
            .krediteras_av
            .iter()
            .map(|(id, nummer, total)| {
                view! {
                    <a href=format!("{bas}/leverantorsfakturor?f={id}")>
                        {format!("Kreditfaktura {nummer}")}
                    </a>
                    <span class="diskret">{format!(" {total} kr")}</span>
                    <br />
                }
            })
            .collect_view();
        view! {
            <dt>"Krediteras av"</dt>
            <dd>{länkar}</dd>
        }
    });
    let avskrifter = (!f.avskrifter.is_empty()).then(|| {
        let rader = f
            .avskrifter
            .iter()
            .map(|a| {
                let rättelse = a.rättelse.clone().map(|(id, nummer)| {
                    view! {
                        "Rättad med "
                        <a href=format!("{bas}/verifikat?v={id}")>{nummer}</a>
                        " "
                    }
                });
                let orsak = (!a.orsak.is_empty()).then(|| format!("· {} ", a.orsak));
                view! {
                    {rättelse}
                    {orsak}
                    <span class="diskret">{format!("· {} av {}", a.rättad.0, a.rättad.1)}</span>
                    <br />
                }
            })
            .collect_view();
        view! {
            <dt>"Rättad avskrift"</dt>
            <dd>{rader}</dd>
        }
    });
    let verifikat = f.verifikat.clone().map(|(id, nummer)| {
        view! {
            <dt>"Verifikat"</dt>
            <dd>
                <a href=format!("{bas}/verifikat?v={id}")>{nummer}</a>
                <span class="diskret">" registrering"</span>
            </dd>
        }
    });
    let registrerad = f
        .registrerad
        .clone()
        .map(|(tid, namn)| rad("Registrerad", format!("{tid} av {namn}")));
    let kontering = f
        .kontering
        .iter()
        .map(|r| {
            view! {
                <tr>
                    <td>{r.konto.clone()}</td>
                    <td class="diskret">{r.kontonamn.clone()}</td>
                    <td class="belopp">{r.debet.clone()}</td>
                    <td class="belopp">{r.kredit.clone()}</td>
                </tr>
            }
        })
        .collect_view();
    let historik = f
        .historik
        .iter()
        .map(|h| {
            view! {
                <li>
                    <span class="diskret">{h.tid.clone()}</span>
                    " "
                    {h.text.clone()}
                    " "
                    <span class="diskret">"· " {h.namn.clone()}</span>
                </li>
            }
        })
        .collect_view();
    // #147 Betalningar: märket, det som återstår, betaluppgifterna, handlingen, panelerna och
    // betalningarna, som på en kundfaktura.
    let här = format!("{bas}/leverantorsfakturor?f={}", f.id);
    let lista = format!("{bas}/leverantorsfakturor");
    let vald_panel = use_query_map().with_untracked(|q| q.get("panel"));
    let (status, kvar, handlingar, panel, betalningar) = match f.reskontra.clone() {
        Some(r) => {
            let status =
                view! { <Märke kod=r.märke.0.clone() namn=r.märke.1.clone() /> }.into_any();
            let kvar = match r.återstår_öre {
                0 => None,
                ö if ö > 0 => Some(format!(" · {} kvar", r.återstår)),
                // #148: ett tillgodohavande hos leverantören.
                ö => Some(format!(" · {} tillgodo", Saldo::från_öre(-ö))),
            }
            .map(|text| view! { <span class="diskret">{text}</span> });
            let panel = match (vald_panel.as_deref(), r.betalning.clone(), r.rätta.clone()) {
                (Some("betalning" | "aterbetala"), Some(v), _) => Some(
                    view! {
                        <Betalningspanel
                            action=format!("{lista}/{}/betalning", f.id)
                            stäng=här.clone()
                            v=v
                        />
                    }
                    .into_any(),
                ),
                (Some("ratta-betalning"), _, Some(rv)) => Some(
                    view! {
                        <RättaBetalningspanel
                            action=format!("{lista}/{}/betalningar/{}/ratta", f.id, rv.id)
                            stäng=här.clone()
                            r=rv
                        />
                    }
                    .into_any(),
                ),
                // #148 Rätta avskrift
                (Some("ratta-avskrift"), _, _) => Some(
                    view! {
                        <RättaAvskriftpanel
                            action=format!("{lista}/{}/ratta-avskrift", f.id)
                            stäng=här.clone()
                            kontering=f.kontering.clone()
                            verifikat=f.verifikat.clone().map(|(_, n)| n).unwrap_or_default()
                        />
                    }
                    .into_any(),
                ),
                _ => None,
            };
            let betala = (r.återstår_öre > 0).then(|| {
                view! {
                    <a class="knapp knapp-primar" href=format!("{här}&panel=betalning")>
                        "Registrera betalning"
                    </a>
                    " "
                }
            });
            // #148: ett tillgodohavande återbetalas, och avskriften rättas.
            let återbetala = (r.återstår_öre < 0).then(|| {
                view! {
                    <a class="knapp knapp-primar" href=format!("{här}&panel=aterbetala")>
                        "Återbetala…"
                    </a>
                    " "
                }
            });
            let handlingar = panel.is_none().then(|| {
                view! {
                    <p class="dokument-handlingar">
                        {betala}
                        {återbetala}
                        <a class="knapp" href=format!("{här}&panel=ratta-avskrift")>
                            "Rätta avskrift…"
                        </a>
                    </p>
                }
            });
            let betalningar =
                view! { <Betalningar bas=bas.clone() här=här.clone() r=r /> }.into_any();
            (status, kvar.into_any(), handlingar, panel, betalningar)
        }
        None => (
            view! { <span class="markering">"Registrerad"</span> }.into_any(),
            ().into_any(),
            None,
            None,
            ().into_any(),
        ),
    };
    let betaluppgifter = (!f.betaluppgifter.is_empty()).then(|| {
        let rader = f
            .betaluppgifter
            .iter()
            .map(|(etikett, v)| {
                view! {
                    <dt>{etikett.clone()}</dt>
                    <dd>
                        <code>{v.clone()}</code>
                    </dd>
                }
            })
            .collect_view();
        view! {
            <section aria-label="Betaluppgifter">
                <h3>"Betala till"</h3>
                <dl class="metadata betaluppgifter">{rader}</dl>
            </section>
        }
    });
    view! {
        <div class="med-underlag">
            <aside class="underlagsspalt" aria-label="Originalet">
                <Förhandsvisning bas=bas.clone() u=f.original.clone() />
            </aside>
            <article class="dokument">
                <header>
                    <p class="dokument-nummer">
                        {format!(
                            "{} {}",
                            if f.kreditfaktura { "Kreditfaktura" } else { "Leverantörsfaktura" },
                            värde("fakturanummer"),
                        )}
                    </p>
                    <h2>{f.leverantörsnamn.clone()}</h2>
                    <p>
                        {status}
                        " "
                        <span class="belopp">{värde("totalbelopp")} " kr"</span>
                        {kvar}
                    </p>
                </header>
                {handlingar}
                {betaluppgifter}
                {panel}
                <dl class="metadata">
                    {rad("Fakturadatum", värde("fakturadatum"))}
                    {rad("Förfallodag", värde("forfallodag"))}
                    {rad("OCR eller meddelande", värde("referens"))}
                    {rad(
                        "Omvänd skattskyldighet",
                        OmvändSkattskyldighet::från_kod(&värde("omvand_skattskyldighet"))
                            .filter(|o| *o != OmvändSkattskyldighet::Ingen)
                            .map(|o| o.namn().to_string())
                            .unwrap_or_default(),
                    )}
                    {rad("Originalvaluta", värde("originalvaluta"))}
                    {krediterar}
                    {krediteras_av}
                    {verifikat}
                    {registrerad}
                    {avskrifter}
                    {rad("Original", f.original.filnamn.clone())}
                </dl>
                <h3>"Kontering"</h3>
                <table class="rader">
                    <thead>
                        <tr>
                            <th>"Konto"</th>
                            <th>"Kontonamn"</th>
                            <th class="belopp">"Debet"</th>
                            <th class="belopp">"Kredit"</th>
                        </tr>
                    </thead>
                    <tbody>{kontering}</tbody>
                </table>
                {betalningar}
                <section>
                    <h3>"Historik"</h3>
                    <ul class="historik">{historik}</ul>
                </section>
            </article>
        </div>
    }
}

/// Panelen Rätta avskrift… (#148): orsaken är valfri, och rutan visar rättelsen som vänder
/// registreringens verifikat i dag. Fakturan blir upplåst med betalningarna kvar.
#[component]
fn RättaAvskriftpanel(
    action: String,
    stäng: String,
    kontering: Vec<KonteringVy>,
    verifikat: String,
) -> impl IntoView {
    let rader = kontering
        .into_iter()
        .map(|r| {
            view! {
                <tr>
                    <td>{r.konto}</td>
                    <td class="diskret">{r.kontonamn}</td>
                    <td class="belopp">{r.kredit}</td>
                    <td class="belopp">{r.debet}</td>
                </tr>
            }
        })
        .collect_view();
    view! {
        <section class="panel avskriftspanel" aria-labelledby="avskrift-rubrik">
            <header class="panelhuvud">
                <h3 id="avskrift-rubrik">"Rätta avskrift"</h3>
                <a class="knapp knapp-diskret" href=stäng>
                    "Avbryt"
                </a>
            </header>
            <p class="diskret">
                {format!(
                    "Registreringen {verifikat} vänds med en rättelse daterad i dag. Fakturan låses upp med betalningarna kvar, och du registrerar om den med samma regler.",
                )}
            </p>
            <form method="post" action=action>
                <div class="falt-grupp">
                    <div>
                        <label for="orsak">"Orsak (valfri)"</label>
                        <input id="orsak" name="orsak" type="text" />
                    </div>
                </div>
                <h4>"Bokförs som rättelse"</h4>
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
                <p class="knappar">
                    <button class="knapp knapp-primar" type="submit">
                        "Rätta avskrift"
                    </button>
                </p>
            </form>
        </section>
    }
}
