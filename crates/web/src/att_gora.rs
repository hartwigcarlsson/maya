//! Att göra, företagets startsida: agendan enligt variant C i `prototype/47-att-gora`, som skal.
//! Överst dagens datum, en sammanfattning och nyckeltalen ur bokföringen. Till vänster
//! tidslinjen med det som har ett datum: i etapp 1 bara utkast med kontantfrist, på fristens
//! dag eller som försenade. Till höger spalten Utan datum med utkasten per period
//! (50-dagarsvarningen i periodens rubrik) och de okopplade underlagen.
//!
//! Ett kort fälls ut på plats: ett utkast med verifikatformuläret, där Bokför går vidare till
//! nästa utkast, och ett underlag med "Vad är det här?".
//!
//! #152: tidslinjen får förfallna kund- och leverantörsfakturor under Försenat och
//! leverantörsfakturorna att betala på sin förfallodag, nedtonade när de förfaller om mer än 7
//! dagar. Kortet visar kopieringschipsen och fäller ut betalningspanelen från fakturans dokument,
//! så att en betalning registreras utan att öppna fakturan. Nyckeltalen får Kundfordringar och
//! Leverantörsskulder. Sakernas sorter kommer från
//! `maya_core::att_gora::Sak`; en ny sort får ett eget kort här ([`Sort`]).
//!
//! #153: spalten Utan datum får Att granska med de nya kunderna och leverantörerna från utkasten
//! som streckade kort, Följ upp med kundfakturorna vars utskick misslyckades eller som inte är
//! skickade (med panelen Skicka: Skicka igen och Lämnad på annat sätt), och fakturautkasten per
//! period tillsammans med verifikatutkasten. Ett fakturautkast öppnas i sin flik med Granska och
//! utfärda eller Granska och registrera.
//!
//! Överst, före allt annat, varningen efter en återställning med `maya restore`, tills
//! användaren kvitterar den.
//!
//! Sidan läser sin data med `sidodata::sida`, så korten fälls ut i routern på klienten.

use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use crate::app::LoggaIn;
use crate::betalning::{BetalningsVy, Betalningspanel};
use crate::foretag::{Företagsval, IngenÅtkomst};
use crate::sidodata::sida;
use crate::underlag::{Förhandsvisning, Gem, UnderlagVy};
use crate::utskick::{SkickaPanel, SkickaVy};
use crate::verifikat::{
    DokumentVy, Formulär, FormulärData, UtkastBadge, företagets_bas, inloggad
};

/// Det som Att göra visar.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttGöradata {
    /// Dagens datum med veckodag.
    pub dag: String,
    /// Varningarna efter en återställning som användaren inte har kvitterat.
    pub återställningar: Vec<ÅterställningVy>,
    pub försenade: usize,
    pub inom_sju_dagar: usize,
    pub utan_datum: usize,
    /// Det som är fel i driften, överst. Tomt när allt fungerar.
    pub driftvarningar: Vec<DriftvarningVy>,
    pub nyckeltal: Vec<NyckeltalVy>,
    pub tidslinje: Vec<DagVy>,
    pub utkast: Vec<PeriodVy>,
    pub underlag: Vec<KortVy>,
    /// #153 Att granska: de nya kunderna och leverantörerna från utkasten.
    #[serde(default)]
    pub att_granska: Vec<KortVy>,
    /// #153 Följ upp: kundfakturorna vars utskick misslyckades eller som inte är skickade.
    #[serde(default)]
    pub följ_upp: Vec<KortVy>,
    /// Kortet som är utfällt och det som visas i det.
    pub öppet: Option<(String, Öppet)>,
    pub notis: Option<String>,
    pub fel: Option<String>,
}

/// Varningen efter en återställning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ÅterställningVy {
    pub id: String,
    pub text: String,
}

/// En driftvarning: samma i alla företag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriftvarningVy {
    pub rubrik: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NyckeltalVy {
    pub namn: String,
    pub värde: String,
    pub under: String,
}

/// En dag i tidslinjen, eller Försenat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DagVy {
    pub rubrik: String,
    /// "i dag", "om 3 dagar" eller tomt för Försenat.
    pub avstånd: String,
    pub försenat: bool,
    pub idag: bool,
    pub kort: Vec<KortVy>,
}

/// En period i spalten Utan datum.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeriodVy {
    pub rubrik: String,
    /// 50-dagarsvarningen, t.ex. "slut för 75 dagar sedan".
    pub varning: Option<String>,
    pub kort: Vec<KortVy>,
}

/// Sorten av sak som ett kort visar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Sort {
    Utkast,
    Underlag,
    /// #152: en förfallen kundfaktura.
    Kundfaktura,
    /// #152: en leverantörsfaktura att betala.
    Leverantörsfaktura,
    /// #153: en kundfaktura vars senaste utskick misslyckades.
    MisslyckatUtskick,
    /// #153: en utfärdad kundfaktura som inte är skickad.
    InteSkickad,
    /// #153: ett utkast till en av Mayas egna kundfakturor.
    Kundfakturautkast,
    /// #153: ett utkast till en extern kundfaktura, eller en upplåst.
    ExternKundfakturautkast,
    /// #153: ett utkast till en leverantörsfaktura, eller en upplåst.
    Leverantörsfakturautkast,
    /// #153: en ny kund från ett utkast.
    NyKund,
    /// #153: en ny leverantör från ett utkast.
    NyLeverantör,
    /// #150: T2-utkastet under kontantmetoden.
    T2Utkast,
}

/// Ett kort i agendan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KortVy {
    /// `utkast-<id>` eller `underlag-<id>`: adressens `oppna` och kortets id.
    pub nyckel: String,
    pub sort: Sort,
    pub rubrik: String,
    pub belopp: Option<String>,
    /// Fristen eller åldern.
    pub när: String,
    /// `""`, `"hot"` (snart eller länge) eller `"bad"` (försenat).
    pub när_klass: String,
    /// Utkastet har kopplade underlag.
    pub gem: bool,
    /// #152: längre fram än 7 dagar, visas nedtonat.
    #[serde(default)]
    pub nedtonad: bool,
    /// #153: fler märken efter sortens: kod (CSS `badge-<kod>`) och text.
    #[serde(default)]
    pub märken: Vec<(String, String)>,
}

/// Det som ett utfällt kort visar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Öppet {
    Utkast {
        dokument: Box<DokumentVy>,
        formulär: FormulärData,
        okopplade: Vec<UnderlagVy>,
    },
    Underlag {
        underlag: Box<UnderlagVy>,
        /// Bokförda verifikat att komplettera: id och text.
        verifikat: Vec<(String, String)>,
        /// Frågan om underlaget ska tas bort visas.
        ta_bort: bool,
    },
    /// #152: en kund- eller leverantörsfaktura med det som återstår.
    Faktura(Box<FakturakortVy>),
    /// #153: en kundfaktura under Följ upp, med panelen Skicka.
    Uppföljning(Box<UppföljningVy>),
    /// #153: ett fakturautkast eller en ny kund eller leverantör, med länken till utkastet.
    Granska(Box<GranskaVy>),
}

/// En utfälld kundfaktura under Följ upp (#153).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UppföljningVy {
    pub id: String,
    /// Adressen till fakturans dokument.
    pub dokument: String,
    /// Kortets adress och adressen som stänger det.
    pub kortet: String,
    pub stäng: String,
    pub skicka: SkickaVy,
}

/// Ett utfällt fakturautkast eller en ny kund eller leverantör (#153).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GranskaVy {
    /// En ny kund eller leverantör visas som ett streckat kort.
    pub streckad: bool,
    /// Etikett och värde.
    pub uppgifter: Vec<(String, String)>,
    /// Utkastets adress och knappens text.
    pub länk: String,
    pub knapp: String,
    pub not: String,
}

/// En utfälld faktura i tidslinjen (#152).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FakturakortVy {
    /// En kundfaktura, annars en leverantörsfaktura.
    pub kund: bool,
    /// Adressen till fakturans dokument.
    pub dokument: String,
    pub återstår: String,
    pub förfallodag: String,
    /// Kundens e-post på fakturan.
    pub kontakt: String,
    /// Etikett och värde att kopiera: BG/PG/IBAN, OCR eller meddelande och beloppet.
    pub kopieringschips: Vec<(String, String)>,
    /// Adressen som fäller ut betalningspanelen, när betalningen kan registreras här.
    pub betala: Option<String>,
    /// Betalningspanelen när den är utfälld: formulärets action, adressen som stänger den och
    /// panelen.
    pub betalning: Option<(String, String, BetalningsVy)>,
}

/// Datan för `/foretag/<id>`.
#[server(prefix = "/api", endpoint = "att_gora", input = server_fn::codec::GetUrl)]
pub async fn hämta_att_göra(adress: String) -> Result<AttGöradata, ServerFnError> {
    match crate::sidodata::Sidbegäran::för(&adress)? {
        Some(begäran) => server::att_göra(&begäran).await,
        None => Ok(AttGöradata::default()),
    }
}

#[cfg(feature = "ssr")]
pub(crate) mod server {
    use crate::svar::{ingen_åtkomst, serverfel};
    use std::sync::Arc;

    use axum::extract::{Path, State};

    use axum::response::{IntoResponse, Redirect, Response};
    use axum::routing::post;
    use axum::{Extension, Form, Router};
    use jiff::civil::Date;
    use leptos::prelude::ServerFnError;
    use maya_core::atkomst::ÅtkomstFel;
    use maya_core::att_gora::{
        self, Fakturasak, Fakturautkast, Fakturautkastsak, NyPart, NyPartsak, Reskontrafaktura,
        Reskontratal, Sak, Uppföljningssak, Utkastsak, ÅterställningsId, Återställningsvarning,
    };
    use maya_core::betalning::{self, BetalningFel, Faktura};
    use maya_core::config::Config;
    use maya_core::foretag::FöretagsId;
    use maya_core::inloggning::Inloggad;
    use maya_core::kundfaktura::KundfakturaId;
    use maya_core::leverantorsfaktura::LeverantörsfakturaId;
    use maya_core::underlag::{self, Underlag, UnderlagFel, UnderlagId};
    use maya_core::verifikat::{self, Urval, VerifikatFel, VerifikatId};
    use maya_core::{Databas, Klocka};
    use maya_domain::att_gora::{
        Frist, Utkastläge, dag_lång, dagar_mellan, länge_okopplat, ordna_utkast, sedan, ålder,
    };
    use maya_domain::belopp::Saldo;
    use maya_domain::rakenskapsar::periodnamn;
    use maya_domain::reskontra::förfallotext;
    use maya_domain::verifikat::NyttVerifikat;
    use serde::Deserialize;

    use super::{
        AttGöradata, DagVy, DriftvarningVy, FakturakortVy, GranskaVy, KortVy, NyckeltalVy,
        PeriodVy, Sort, UppföljningVy, ÅterställningVy, Öppet,
    };
    use crate::sidodata::{self, Sidbegäran};
    use crate::underlag::server::vy;

    #[derive(Clone)]
    pub(crate) struct AttGöraState {
        pub config: Arc<Config>,
        pub db: Databas,
        pub klocka: Klocka,
    }

    pub(crate) fn routes(state: AttGöraState) -> Router {
        Router::new()
            .route("/foretag/{id}/att-gora/underlag/{u}/kvitto", post(kvitto))
            // #145 Vad är det här? → Leverantörsfaktura
            .route(
                "/foretag/{id}/att-gora/underlag/{u}/leverantorsfaktura",
                post(leverantörsfaktura),
            )
            .route(
                "/foretag/{id}/att-gora/underlag/{u}/komplettera",
                post(komplettera),
            )
            .route("/foretag/{id}/att-gora/underlag/{u}/ta-bort", post(ta_bort))
            .route(
                "/foretag/{id}/att-gora/aterstallning/{a}/kvittera",
                post(kvittera),
            )
            // #152 Betalning från kortet
            .route(
                "/foretag/{id}/att-gora/kundfakturor/{f}/betalning",
                post(kundbetalning),
            )
            .route(
                "/foretag/{id}/att-gora/leverantorsfakturor/{f}/betalning",
                post(leverantörsbetalning),
            )
            .with_state(Arc::new(state))
    }

    /// Hämtar det som Att göra visar, för `/foretag/<id>`. Utan åtkomst blir datan tom; sidan
    /// visar det ur företagsvalet.
    pub(crate) async fn att_göra(b: &Sidbegäran) -> Result<AttGöradata, ServerFnError> {
        let delar = b.delar();
        let ["foretag", id] = delar.as_slice() else {
            return Ok(AttGöradata::default());
        };
        let Ok(företag) = id.parse::<FöretagsId>() else {
            return Ok(AttGöradata::default());
        };
        let query: Vec<(String, String)> = url::form_urlencoded::parse(b.fråga().as_bytes())
            .into_owned()
            .collect();
        let state = AttGöraState {
            config: b.config.clone(),
            db: b.db.clone(),
            klocka: b.klocka.clone(),
        };
        match hämta(&state, b, företag, &query).await {
            Ok(data) => Ok(data),
            Err(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte) => {
                Ok(AttGöradata::default())
            }
            Err(e) => Err(sidodata::serverfel(e)),
        }
    }

    async fn hämta(
        state: &AttGöraState,
        b: &Sidbegäran,
        företag: FöretagsId,
        query: &[(String, String)],
    ) -> Result<AttGöradata, ÅtkomstFel> {
        let inloggad = &b.inloggad;
        let fråga = |namn: &str| {
            query
                .iter()
                .find(|(k, _)| k == namn)
                .map(|(_, v)| v.as_str())
        };
        let (db, aktör) = (&state.db, &inloggad.aktör);
        let a = att_gora::att_göra(db, &state.klocka, &state.config, aktör, företag).await?;
        let idag = a.idag;
        let mut driftvarningar = Vec::new();
        let mut utkast = Vec::new();
        let mut underlagen = Vec::new();
        let mut återställningar = Vec::new();
        let mut fakturorna = Vec::new();
        let mut uppföljningar = Vec::new();
        let mut fakturautkasten = Vec::new();
        let mut nya_parter = Vec::new();
        for sak in a.saker {
            match sak {
                Sak::Återställning(v) => återställningar.push(återställning(&v)),
                Sak::Driftvarning(v) => driftvarningar.push(DriftvarningVy {
                    rubrik: v.rubrik().into(),
                    text: v.text(),
                }),
                Sak::Utkast(u) => utkast.push(u),
                Sak::Underlag(u) => underlagen.push(u),
                Sak::Faktura(f) => fakturorna.push(f),
                // #153
                Sak::Uppföljning(u) => uppföljningar.push(u),
                Sak::Fakturautkast(u) => fakturautkasten.push(u),
                Sak::NyPart(p) => nya_parter.push(p),
            }
        }
        let antal_utkast = utkast.len();
        // #153 Fakturautkasten står per period efter fakturadatumet tillsammans med
        // verifikatutkasten. De har ingen kontantfrist.
        let alla: Vec<Utkastpost> = utkast
            .into_iter()
            .map(Utkastpost::Verifikat)
            .chain(fakturautkasten.iter().cloned().map(Utkastpost::Faktura))
            .collect();
        let ordning = ordna_utkast(alla, Utkastpost::läge, idag);
        // Ordningen som Bokför går vidare i: tidslinjen och sedan perioderna, bara verifikaten.
        let i_ordning: Vec<&Utkastsak> = ordning
            .kontanta
            .iter()
            .map(|(_, u)| u)
            .chain(ordning.perioder.iter().flat_map(|p| p.utkast.iter()))
            .filter_map(Utkastpost::verifikat)
            .collect();

        let mut notis = None;
        let mut öppna = fråga("oppna").map(str::to_string);
        if let Some(id) = fråga("bokford").and_then(|v| v.parse::<VerifikatId>().ok())
            && let Some(v) = verifikat::verifikatet(db, aktör, företag, id).await?
            && let Some(nummer) = v.nummer
        {
            notis = Some(format!("Bokförd som {nummer}. {antal_utkast} utkast kvar."));
            if öppna.is_none() {
                öppna = i_ordning.first().map(|u| format!("utkast-{}", u.id));
            }
        } else if fråga("sparat").is_some() {
            notis = Some("Utkastet sparades.".into());
        } else if fråga("kvitto").is_some() {
            notis = Some("Ett verifikatutkast skapades med underlaget kopplat.".into());
        } else if let Some(v) = fråga("kompletterat") {
            notis = Some(format!(
                "{v} kompletterades med underlaget. Kompletteringen loggades och kan inte tas bort."
            ));
        } else if fråga("borttaget").is_some() {
            notis = Some("Underlaget togs bort, och borttagningen loggades.".into());
        } else if fråga("kvitterad").is_some() {
            notis = Some("Varningen är kvitterad.".into());
        } else if let Some(v) = fråga("betald") {
            notis = Some(format!("Betalningen är bokförd som {v}."));
        } else if fråga("skickat").is_some() {
            notis = Some("Fakturan skickades med e-post, och utskicket loggades.".into());
        } else if fråga("lamnad").is_some() {
            notis = Some("Fakturan markerades som lämnad på annat sätt.".into());
        }
        let fel = fråga("fel")
            .and_then(|kod| match kod {
                "inget_verifikat" => Some("Välj ett bokfört verifikat att komplettera."),
                // #153 Skicka igen från Följ upp
                "skickfel" => Some(
                    "Utskicket misslyckades och loggades. Fakturan är utfärdad och kan skickas igen.",
                ),
                "mottagare" => Some("Mottagaren är ingen e-postadress. Inget skickades."),
                kod => crate::underlag::feltext(kod),
            })
            .map(str::to_string);

        let kort_för_utkast = |u: &Utkastsak| {
            let (när, klass) = match u.frist(idag) {
                Frist::Kontantfrist { senast, försenad } => {
                    let dagar = dagar_mellan(idag, senast);
                    if försenad {
                        (
                            format!("kontantfrist {}, {} d sedan", senast, -dagar),
                            "bad",
                        )
                    } else if dagar == 0 {
                        ("kontantfrist i dag".to_string(), "hot")
                    } else {
                        (format!("kontantfrist {}", dag_lång(senast)), "hot")
                    }
                }
                _ => (ålder(tid_datum(u.skapad), idag), ""),
            };
            KortVy {
                nyckel: format!("utkast-{}", u.id),
                sort: Sort::Utkast,
                rubrik: if u.beskrivning.trim().is_empty() {
                    "Utkast utan beskrivning".into()
                } else {
                    u.beskrivning.clone()
                },
                belopp: Some(u.summa.to_string()),
                när,
                när_klass: klass.into(),
                gem: u.underlag > 0,
                nedtonad: false,
                märken: Vec::new(),
            }
        };
        let bas = format!("/foretag/{företag}");
        let kort_för_post = |p: &Utkastpost| match p {
            Utkastpost::Verifikat(u) => kort_för_utkast(u),
            Utkastpost::Faktura(f) => kort_för_fakturautkast(f, idag),
        };

        // Det som har ett datum: utkasten med kontantfrist och (#152) fakturorna på sin
        // förfallodag, i datumordning.
        let mut daterade: Vec<(Date, KortVy)> = ordning
            .kontanta
            .iter()
            .map(|(senast, u)| (*senast, kort_för_post(u)))
            .collect();
        daterade.extend(
            fakturorna
                .iter()
                .map(|f| (f.förfallodag, kort_för_faktura(f, idag))),
        );
        daterade.sort_by_key(|(dag, _)| *dag);
        let mut tidslinje: Vec<DagVy> = Vec::new();
        let mut försenade = 0;
        let mut inom_sju_dagar = 0;
        for (senast, kort) in daterade {
            let försenat = senast < idag;
            let dagar = dagar_mellan(idag, senast);
            if försenat {
                försenade += 1;
            } else if dagar <= 7 {
                inom_sju_dagar += 1;
            }
            let rubrik = if försenat {
                "Försenat".to_string()
            } else {
                stor_bokstav(&dag_lång(senast))
            };
            match tidslinje.last_mut() {
                Some(d) if d.rubrik == rubrik => d.kort.push(kort),
                _ => tidslinje.push(DagVy {
                    rubrik,
                    avstånd: match dagar {
                        _ if försenat => String::new(),
                        0 => "i dag".into(),
                        1 => "i morgon".into(),
                        n => format!("om {n} dagar"),
                    },
                    försenat,
                    idag: dagar == 0,
                    kort: vec![kort],
                }),
            }
        }
        let perioder: Vec<PeriodVy> = ordning
            .perioder
            .iter()
            .map(|p| PeriodVy {
                rubrik: p
                    .månad
                    .map(|m| stor_bokstav(&periodnamn(m)))
                    .unwrap_or_else(|| "Utan verifikatdatum".into()),
                varning: p.varning.map(|d| format!("slut för {d} dagar sedan")),
                kort: p.utkast.iter().map(kort_för_post).collect(),
            })
            .collect();
        let underlag: Vec<KortVy> = underlagen
            .iter()
            .map(|u| {
                let uppladdad = tid_datum(u.uppladdad.tid);
                let länge = länge_okopplat(uppladdad, idag);
                KortVy {
                    nyckel: format!("underlag-{}", u.id),
                    sort: Sort::Underlag,
                    rubrik: u.filnamn.clone(),
                    belopp: None,
                    när: if länge {
                        format!("{} · 30+ dagar", ålder(uppladdad, idag))
                    } else {
                        ålder(uppladdad, idag)
                    },
                    när_klass: if länge { "hot" } else { "" }.into(),
                    gem: false,
                    nedtonad: false,
                    märken: Vec::new(),
                }
            })
            .collect();
        // #153 Att granska och Följ upp.
        let att_granska: Vec<KortVy> = nya_parter.iter().map(kort_för_ny_part).collect();
        let följ_upp: Vec<KortVy> = uppföljningar
            .iter()
            .map(|u| kort_för_uppföljning(u, idag))
            .collect();
        let utan_datum = perioder.iter().map(|p| p.kort.len()).sum::<usize>()
            + underlag.len()
            + att_granska.len()
            + följ_upp.len();

        let öppet = match öppna.as_deref().and_then(|n| n.split_once('-')) {
            Some(("utkast", id)) => öppet_utkast(state, inloggad, företag, id).await?,
            Some(("underlag", id)) => {
                let ta_bort = fråga("ta_bort").is_some();
                öppet_underlag(state, inloggad, företag, &underlagen, id, ta_bort).await?
            }
            Some((sort @ ("kundfaktura" | "leverantorsfaktura"), id)) => {
                öppen_faktura(b, företag, &fakturorna, sort, id).await?
            }
            // #153
            Some(("utskick", id)) => öppen_uppföljning(b, företag, &uppföljningar, id).await?,
            Some((sort, id)) => {
                let nyckel = format!("{sort}-{id}");
                fakturautkasten
                    .iter()
                    .find(|u| utkastnyckel(u.utkast) == nyckel)
                    .map(|u| öppet_fakturautkast(&bas, u))
                    .or_else(|| {
                        nya_parter
                            .iter()
                            .find(|p| partnyckel(p.part) == nyckel)
                            .map(|p| öppen_ny_part(&bas, p))
                    })
            }
            _ => None,
        }
        .map(|ö| (öppna.unwrap_or_default(), ö));

        Ok(AttGöradata {
            dag: stor_bokstav(&dag_lång(idag)),
            återställningar,
            försenade,
            inom_sju_dagar,
            utan_datum,
            driftvarningar,
            nyckeltal: nyckeltal(&a.nyckeltal),
            tidslinje,
            utkast: perioder,
            underlag,
            att_granska,
            följ_upp,
            öppet,
            notis,
            fel,
        })
    }

    fn återställning(v: &Återställningsvarning) -> ÅterställningVy {
        let tid = |t: jiff::Timestamp| maya_domain::tid::minut(t);
        let backup = tid(v.backup_tid);
        ÅterställningVy {
            id: v.id.to_string(),
            text: format!(
                "Installationen återställdes {} ur backupen från {backup}. Det som \
                 registrerades efter {backup} finns inte längre i Maya och måste registreras om.",
                tid(v.tid)
            ),
        }
    }

    fn nyckeltal(n: &att_gora::Nyckeltal) -> Vec<NyckeltalVy> {
        let kronor = |s: Saldo| format!("{s} kr");
        // #152 Kundfordringar och Leverantörsskulder: saldot ur bokföringen med det förfallna
        // ur reskontran, eller reskontrans obetalda belopp under kontantmetoden.
        let reskontra = |namn: &str, konto: u16, r: &Reskontratal| NyckeltalVy {
            namn: namn.into(),
            värde: kronor(r.saldo),
            under: format!(
                "{} · {}",
                if r.förfallet == Saldo::NOLL {
                    "inget förfallet".to_string()
                } else {
                    format!("varav förfallet {}", kronor(r.förfallet))
                },
                if r.ur_reskontran {
                    "ur reskontran".to_string()
                } else {
                    format!("{konto} ur bokföringen")
                }
            ),
        };
        vec![
            NyckeltalVy {
                namn: "Företagskontot".into(),
                värde: kronor(n.företagskontot),
                under: "1930 per i dag · ur bokföringen".into(),
            },
            reskontra("Kundfordringar", 1510, &n.kundfordringar),
            reskontra("Leverantörsskulder", 2440, &n.leverantörsskulder),
            NyckeltalVy {
                namn: "Resultat hittills i år".into(),
                värde: kronor(n.resultat_i_år),
                under: match n.år {
                    Some(år) => format!("{} – {} · ur bokföringen", år.start(), år.slut()),
                    None => "inget räkenskapsår i dag · ur bokföringen".into(),
                },
            },
        ]
    }

    /// #152: kortet för en faktura i tidslinjen.
    fn kort_för_faktura(f: &Fakturasak, idag: Date) -> KortVy {
        let dagar = dagar_mellan(idag, f.förfallodag);
        let (sort, rubrik) = match f.faktura {
            Reskontrafaktura::Kund(_) => (
                Sort::Kundfaktura,
                format!("Faktura {} · {}", f.nummer, f.motpart),
            ),
            Reskontrafaktura::Leverantör(_) => (
                Sort::Leverantörsfaktura,
                format!("{} · {}", f.motpart, f.nummer),
            ),
        };
        KortVy {
            nyckel: fakturanyckel(f.faktura),
            sort,
            rubrik,
            belopp: Some(Saldo::från_öre(f.återstår).to_string()),
            när: förfallotext(f.förfallodag, idag),
            när_klass: match dagar {
                ..0 => "bad",
                0..=7 => "hot",
                _ => "",
            }
            .into(),
            gem: false,
            nedtonad: dagar > 7,
            märken: Vec::new(),
        }
    }

    // #153 Följ upp, fakturautkasten och de nya kunderna och leverantörerna

    /// Ett verifikatutkast eller ett fakturautkast i spalten Utan datum.
    #[derive(Clone)]
    enum Utkastpost {
        Verifikat(Utkastsak),
        Faktura(Fakturautkastsak),
    }

    impl Utkastpost {
        fn läge(&self) -> Utkastläge {
            match self {
                Utkastpost::Verifikat(u) => u.läge(),
                Utkastpost::Faktura(f) => Utkastläge {
                    datum: f.fakturadatum,
                    kassa: false,
                },
            }
        }

        fn verifikat(&self) -> Option<&Utkastsak> {
            match self {
                Utkastpost::Verifikat(u) => Some(u),
                Utkastpost::Faktura(_) => None,
            }
        }
    }

    /// Kortets id och adressens `oppna` för ett fakturautkast.
    fn utkastnyckel(u: Fakturautkast) -> String {
        match u {
            Fakturautkast::Kundfaktura(id) => format!("kundfakturautkast-{id}"),
            Fakturautkast::ExternKundfaktura(id) => format!("externutkast-{id}"),
            Fakturautkast::Leverantörsfaktura(id) => format!("leverantorsfakturautkast-{id}"),
            Fakturautkast::T2(id) => format!("t2-{id}"),
        }
    }

    /// Kortets id och adressens `oppna` för en ny kund eller leverantör.
    fn partnyckel(p: NyPart) -> String {
        match p {
            NyPart::Kund(id) => format!("nykund-{id}"),
            NyPart::Leverantör(id) => format!("nyleverantor-{id}"),
        }
    }

    /// Utkastets adress i sin flik, och vad knappen gör där.
    fn utkastets_länk(bas: &str, u: Fakturautkast) -> (String, &'static str) {
        match u {
            Fakturautkast::Kundfaktura(id) => {
                (format!("{bas}/kundfakturor?f={id}"), "Granska och utfärda")
            }
            Fakturautkast::ExternKundfaktura(id) => (
                format!("{bas}/kundfakturor?f={id}"),
                "Granska och registrera",
            ),
            Fakturautkast::Leverantörsfaktura(id) => (
                format!("{bas}/leverantorsfakturor?f={id}"),
                "Granska och registrera",
            ),
            // #150: T2-utkastet bokförs i fliken Verifikat.
            Fakturautkast::T2(id) => (format!("{bas}/verifikat?v={id}"), "Granska och bokför"),
        }
    }

    fn kort_för_fakturautkast(f: &Fakturautkastsak, idag: Date) -> KortVy {
        let (sort, sorten) = match f.utkast {
            Fakturautkast::Kundfaktura(_) if f.kreditfaktura => {
                (Sort::Kundfakturautkast, "Kreditfaktura")
            }
            Fakturautkast::Kundfaktura(_) => (Sort::Kundfakturautkast, "Kundfaktura"),
            Fakturautkast::ExternKundfaktura(_) => {
                (Sort::ExternKundfakturautkast, "Extern kundfaktura")
            }
            Fakturautkast::Leverantörsfaktura(_) => {
                (Sort::Leverantörsfakturautkast, "Leverantörsfaktura")
            }
            Fakturautkast::T2(_) => (Sort::T2Utkast, "Obetalda fakturor vid årets slut"),
        };
        let rubrik = [
            sorten.to_string(),
            f.fakturanummer.clone(),
            f.motpart.clone(),
        ]
        .into_iter()
        .filter(|d| !d.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
        let mut märken = Vec::new();
        if f.upplåst {
            märken.push(("upplast".to_string(), "upplåst".to_string()));
        }
        if f.ny_motpart {
            let ny = match f.utkast {
                Fakturautkast::Leverantörsfaktura(_) => "ny leverantör",
                _ => "ny kund",
            };
            märken.push(("ny".to_string(), ny.to_string()));
        }
        KortVy {
            nyckel: utkastnyckel(f.utkast),
            sort,
            rubrik,
            belopp: f.total.map(|t| Saldo::från_öre(t).to_string()),
            när: match (f.utkast, f.fakturadatum) {
                (Fakturautkast::T2(_), Some(d)) => format!("verifikatdatum {}", dag_lång(d)),
                (_, Some(d)) => format!("fakturadatum {}", dag_lång(d)),
                (_, None) => "inget fakturadatum".into(),
            },
            när_klass: match f.fakturadatum {
                Some(d) if dagar_mellan(d, idag) > 50 => "hot",
                _ => "",
            }
            .into(),
            gem: matches!(
                f.utkast,
                Fakturautkast::ExternKundfaktura(_) | Fakturautkast::Leverantörsfaktura(_)
            ),
            nedtonad: false,
            märken,
        }
    }

    fn kort_för_ny_part(p: &NyPartsak) -> KortVy {
        KortVy {
            nyckel: partnyckel(p.part),
            sort: match p.part {
                NyPart::Kund(_) => Sort::NyKund,
                NyPart::Leverantör(_) => Sort::NyLeverantör,
            },
            rubrik: p.namn.clone(),
            belopp: None,
            när: match p.part {
                NyPart::Kund(_) => "läggs i registret när fakturan utfärdas eller registreras",
                NyPart::Leverantör(_) => "läggs i registret när fakturan registreras",
            }
            .into(),
            när_klass: String::new(),
            gem: false,
            nedtonad: false,
            märken: Vec::new(),
        }
    }

    fn kort_för_uppföljning(u: &Uppföljningssak, idag: Date) -> KortVy {
        let (sort, när, klass) = match &u.misslyckat {
            Some(m) => (
                Sort::MisslyckatUtskick,
                format!(
                    "till {} misslyckades {}",
                    m.till,
                    sedan(tid_datum(m.tid), idag)
                ),
                "bad",
            ),
            None => (
                Sort::InteSkickad,
                format!("utfärdad {}", sedan(u.fakturadatum, idag)),
                "",
            ),
        };
        KortVy {
            nyckel: format!("utskick-{}", u.faktura),
            sort,
            rubrik: format!("Faktura {} · {}", u.nummer, u.kund),
            belopp: Some(Saldo::från_öre(u.total).to_string()),
            när,
            när_klass: klass.into(),
            gem: false,
            nedtonad: false,
            märken: Vec::new(),
        }
    }

    fn öppet_fakturautkast(bas: &str, f: &Fakturautkastsak) -> Öppet {
        let (länk, knapp) = utkastets_länk(bas, f.utkast);
        let mut uppgifter = Vec::new();
        if !f.motpart.is_empty() {
            uppgifter.push((
                match f.utkast {
                    Fakturautkast::Leverantörsfaktura(_) => "Leverantör",
                    _ => "Kund",
                }
                .to_string(),
                f.motpart.clone(),
            ));
        }
        if !f.fakturanummer.is_empty() {
            uppgifter.push(("Fakturanummer".into(), f.fakturanummer.clone()));
        }
        uppgifter.push((
            match f.utkast {
                Fakturautkast::T2(_) => "Verifikatdatum",
                _ => "Fakturadatum",
            }
            .into(),
            f.fakturadatum
                .map(|d| d.to_string())
                .unwrap_or_else(|| "inte ifyllt".into()),
        ));
        if let Some(t) = f.total {
            uppgifter.push(("Totalt".into(), format!("{} kr", Saldo::från_öre(t))));
        }
        let not = match f.utkast {
            Fakturautkast::Kundfaktura(_) => {
                "Utfärdandet görs i dialogen Granska och utfärda, med PDF:en och utskicket."
            }
            Fakturautkast::ExternKundfaktura(_) => {
                "Registreringen görs med originalet bredvid formuläret i Kundfakturor."
            }
            Fakturautkast::Leverantörsfaktura(_) => {
                "Registreringen görs med originalet bredvid formuläret i Leverantörsfakturor."
            }
            Fakturautkast::T2(_) => {
                "Maya skapade utkastet med det som var obetalt vid årets slut på fakturorna \
                 under kontantmetoden och räknar om det när fakturorna och betalningarna \
                 ändras. Det kan inte ändras för hand. När det är bokfört betalas fakturorna \
                 mot 1510 och 2440, och verifikatet vänds inte i det nya året."
            }
        };
        Öppet::Granska(Box::new(GranskaVy {
            streckad: false,
            uppgifter,
            länk,
            knapp: format!("{knapp} ↗"),
            not: not.into(),
        }))
    }

    fn öppen_ny_part(bas: &str, p: &NyPartsak) -> Öppet {
        let (länk, _) = utkastets_länk(bas, p.utkast);
        let not = match p.part {
            NyPart::Kund(_) => {
                "Kunden hör till utkastet och läggs i kundregistret när fakturan utfärdas eller \
                 registreras. Raderas utkastet försvinner den."
            }
            NyPart::Leverantör(_) => {
                "Leverantören hör till utkastet och läggs i leverantörsregistret när fakturan \
                 registreras. Raderas utkastet försvinner den."
            }
        };
        Öppet::Granska(Box::new(GranskaVy {
            streckad: true,
            uppgifter: p
                .uppgifter
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
            länk,
            knapp: "Granska utkastet ↗".into(),
            not: not.into(),
        }))
    }

    /// En kundfaktura under Följ upp med panelen Skicka, som i fakturans dokument (#138).
    async fn öppen_uppföljning(
        b: &Sidbegäran,
        företag: FöretagsId,
        uppföljningar: &[Uppföljningssak],
        id: &str,
    ) -> Result<Option<Öppet>, ÅtkomstFel> {
        let Some(u) = uppföljningar.iter().find(|u| u.faktura.to_string() == id) else {
            return Ok(None);
        };
        let (_, skicka) = crate::utskick::server::utskicken(b, företag, u.faktura, true)
            .await
            .map_err(|e| match e {
                maya_core::utskick::UtskickFel::Åtkomst(e) => e,
                e => ÅtkomstFel::Databas(maya_core::DatabasFel::Korrupt(e.to_string())),
            })?;
        let Some(skicka) = skicka else {
            return Ok(None);
        };
        let bas = format!("/foretag/{företag}");
        Ok(Some(Öppet::Uppföljning(Box::new(UppföljningVy {
            id: u.faktura.to_string(),
            dokument: format!("{bas}/kundfakturor?f={}", u.faktura),
            kortet: format!("{bas}?oppna=utskick-{}", u.faktura),
            stäng: bas,
            skicka,
        }))))
    }

    /// `kundfaktura-<id>` eller `leverantorsfaktura-<id>`: kortets id och adressens `oppna`.
    fn fakturanyckel(f: Reskontrafaktura) -> String {
        match f {
            Reskontrafaktura::Kund(id) => format!("kundfaktura-{id}"),
            Reskontrafaktura::Leverantör(id) => format!("leverantorsfaktura-{id}"),
        }
    }

    /// #152: en utfälld faktura med kopieringschipsen, och betalningspanelen från fakturans
    /// dokument när adressen har `panel=betalning`.
    async fn öppen_faktura(
        b: &Sidbegäran,
        företag: FöretagsId,
        fakturorna: &[Fakturasak],
        sort: &str,
        id: &str,
    ) -> Result<Option<Öppet>, ÅtkomstFel> {
        let nyckel = format!("{sort}-{id}");
        let Some(f) = fakturorna
            .iter()
            .find(|f| fakturanyckel(f.faktura) == nyckel)
        else {
            return Ok(None);
        };
        let bas = format!("/foretag/{företag}");
        let återstår = Saldo::från_öre(f.återstår).to_string();
        let mut vy = FakturakortVy {
            kund: false,
            dokument: String::new(),
            återstår: återstår.clone(),
            förfallodag: f.förfallodag.to_string(),
            kontakt: f.kontakt.clone(),
            kopieringschips: f
                .kopieringschips
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
            betala: None,
            betalning: None,
        };
        // Betalningen registreras med samma panel och use case som i fakturans dokument
        // (#139, #147), med en egen POST som går tillbaka till Att göra.
        let (faktura, sida) = match f.faktura {
            Reskontrafaktura::Kund(id) => {
                vy.kund = true;
                (Faktura::Kund(id), format!("kundfakturor/{id}"))
            }
            Reskontrafaktura::Leverantör(id) => {
                vy.kopieringschips.push(("Belopp".into(), återstår));
                (Faktura::Leverantör(id), format!("leverantorsfakturor/{id}"))
            }
        };
        vy.dokument = format!("{bas}/{}", sida.replace('/', "?f="));
        vy.betala = Some(format!("{bas}?oppna={nyckel}&panel=betalning"));
        if b.värde("panel").as_deref() == Some("betalning") {
            let r = crate::betalning::server::reskontra(b, företag, faktura, Some("betalning"))
                .await
                .map_err(|e| match e {
                    BetalningFel::Åtkomst(e) => e,
                    e => ÅtkomstFel::Databas(maya_core::DatabasFel::Korrupt(e.to_string())),
                })?;
            vy.betalning = r.betalning.map(|v| {
                (
                    format!("{bas}/att-gora/{sida}/betalning"),
                    format!("{bas}?oppna={nyckel}"),
                    v,
                )
            });
        }
        Ok(Some(Öppet::Faktura(Box::new(vy))))
    }

    /// Ett utkast med formuläret, som i fliken Verifikat.
    async fn öppet_utkast(
        state: &AttGöraState,
        inloggad: &Inloggad,
        företag: FöretagsId,
        id: &str,
    ) -> Result<Option<Öppet>, ÅtkomstFel> {
        let data = crate::verifikat::server::hämta(
            &state.db,
            state.klocka.idag(),
            inloggad,
            företag,
            Urval::Utkast,
            Some(id.to_string()),
            false,
            None,
        )
        .await?;
        Ok(match (data.dokument, data.formulär) {
            (Some(dokument), Some(mut formulär)) if dokument.nummer.is_none() => {
                formulär.idag = state.klocka.idag().to_string();
                Some(Öppet::Utkast {
                    dokument: Box::new(dokument),
                    formulär,
                    okopplade: data.okopplade,
                })
            }
            _ => None,
        })
    }

    /// Ett okopplat underlag med "Vad är det här?".
    async fn öppet_underlag(
        state: &AttGöraState,
        inloggad: &Inloggad,
        företag: FöretagsId,
        okopplade: &[Underlag],
        id: &str,
        ta_bort: bool,
    ) -> Result<Option<Öppet>, ÅtkomstFel> {
        let Some(u) = okopplade.iter().find(|u| u.id.to_string() == id) else {
            return Ok(None);
        };
        let verifikat = verifikat::lista(&state.db, &inloggad.aktör, företag, Urval::Bokförda)
            .await?
            .into_iter()
            .map(|v| {
                let nummer = v.nummer.map(|n| n.to_string()).unwrap_or_default();
                let datum = v.datum.map(|d| d.to_string()).unwrap_or_default();
                (
                    v.id.to_string(),
                    format!("{nummer} · {datum} · {} · {} kr", v.beskrivning, v.summa),
                )
            })
            .collect();
        Ok(Some(Öppet::Underlag {
            underlag: Box::new(vy(u)),
            verifikat,
            ta_bort,
        }))
    }

    fn tid_datum(t: jiff::Timestamp) -> Date {
        maya_domain::tid::datum(t)
    }

    fn stor_bokstav(text: &str) -> String {
        let mut tecken = text.chars();
        match tecken.next() {
            Some(c) => c.to_uppercase().chain(tecken).collect(),
            None => String::new(),
        }
    }

    type Underlagsväg = Path<(String, String)>;

    fn tolka(id: &str, u: &str) -> Option<(FöretagsId, UnderlagId)> {
        Some((id.parse().ok()?, u.parse().ok()?))
    }

    /// Kvitto: ett nytt, tomt verifikatutkast med underlaget kopplat, utfällt i Att göra.
    async fn kvitto(
        State(state): State<Arc<AttGöraState>>,
        Path((id, u)): Underlagsväg,
        inloggad: Option<Extension<Inloggad>>,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let Some((företag, u)) = tolka(&id, &u) else {
            return ingen_åtkomst();
        };
        match verifikat::spara_utkast_med_underlag(
            &state.db,
            &state.klocka,
            &inloggad.aktör,
            företag,
            None,
            &NyttVerifikat::default(),
            &[u],
        )
        .await
        {
            Ok(utkast) => Redirect::to(&format!(
                "/foretag/{företag}?oppna=utkast-{utkast}&kvitto=1"
            ))
            .into_response(),
            Err(VerifikatFel::OkäntUnderlag) => {
                Redirect::to(&format!("/foretag/{företag}?fel=okant_underlag")).into_response()
            }
            Err(VerifikatFel::Åtkomst(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte)) => {
                ingen_åtkomst()
            }
            Err(e) => serverfel(e),
        }
    }

    /// Leverantörsfaktura: ett utkast med underlaget som original, öppnat i fliken
    /// Leverantörsfakturor med originalet till vänster.
    async fn leverantörsfaktura(
        State(state): State<Arc<AttGöraState>>,
        Path((id, u)): Underlagsväg,
        inloggad: Option<Extension<Inloggad>>,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let Some((företag, u)) = tolka(&id, &u) else {
            return ingen_åtkomst();
        };
        match maya_core::leverantorsfaktura::skapa_utkast(
            &state.db,
            &state.klocka,
            &inloggad.aktör,
            företag,
            u,
        )
        .await
        {
            Ok(f) => Redirect::to(&format!(
                "/foretag/{företag}/leverantorsfakturor?f={f}&skapat=1"
            ))
            .into_response(),
            Err(maya_core::leverantorsfaktura::FakturaFel::InteOkopplat) => {
                Redirect::to(&format!("/foretag/{företag}?fel=okant_underlag")).into_response()
            }
            Err(maya_core::leverantorsfaktura::FakturaFel::Åtkomst(
                ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte,
            )) => ingen_åtkomst(),
            Err(e) => serverfel(e),
        }
    }

    #[derive(Deserialize)]
    struct Kompletteringsval {
        #[serde(default)]
        verifikat: String,
    }

    /// Komplettering: underlaget kopplas till ett bokfört verifikat (VF6).
    async fn komplettera(
        State(state): State<Arc<AttGöraState>>,
        Path((id, u)): Underlagsväg,
        inloggad: Option<Extension<Inloggad>>,
        Form(val): Form<Kompletteringsval>,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let Some((företag, u)) = tolka(&id, &u) else {
            return ingen_åtkomst();
        };
        let tillbaka = format!("/foretag/{företag}?oppna=underlag-{u}");
        let Ok(v) = val.verifikat.parse::<VerifikatId>() else {
            return Redirect::to(&format!("{tillbaka}&fel=inget_verifikat")).into_response();
        };
        let (db, aktör) = (&state.db, &inloggad.aktör);
        match underlag::komplettera(db, &state.klocka, aktör, företag, v, u).await {
            Ok(()) => {
                let nummer = match verifikat::verifikatet(db, aktör, företag, v).await {
                    Ok(Some(v)) => v.nummer.map(|n| n.to_string()).unwrap_or_default(),
                    _ => String::new(),
                };
                Redirect::to(&format!("/foretag/{företag}?kompletterat={nummer}")).into_response()
            }
            Err(UnderlagFel::IngetBokförtVerifikat) => {
                Redirect::to(&format!("{tillbaka}&fel=inget_verifikat")).into_response()
            }
            Err(UnderlagFel::RedanKopplat) => {
                Redirect::to(&format!("{tillbaka}&fel=redan_kopplat")).into_response()
            }
            Err(e) => crate::underlag::server::underlagsfel(e),
        }
    }

    /// Dubblett eller fel fil: underlaget tas bort, och borttagningen loggas.
    async fn ta_bort(
        State(state): State<Arc<AttGöraState>>,
        Path((id, u)): Underlagsväg,
        inloggad: Option<Extension<Inloggad>>,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let Some((företag, u)) = tolka(&id, &u) else {
            return ingen_åtkomst();
        };
        match underlag::ta_bort(&state.db, &state.klocka, &inloggad.aktör, företag, u).await {
            Ok(()) => Redirect::to(&format!("/foretag/{företag}?borttaget=1")).into_response(),
            Err(UnderlagFel::HörTillBokfört) => Redirect::to(&format!(
                "/foretag/{företag}?oppna=underlag-{u}&fel=hor_till_bokfort"
            ))
            .into_response(),
            Err(e) => crate::underlag::server::underlagsfel(e),
        }
    }

    /// #152 `POST /foretag/{id}/att-gora/kundfakturor/{f}/betalning`: registrerar betalningen
    /// som i fakturans dokument och visar Att göra med verifikatets nummer, eller kortet igen
    /// med panelen och spärrarna vid fälten.
    async fn kundbetalning(
        State(state): State<Arc<AttGöraState>>,
        Path((id, f)): Path<(String, String)>,
        inloggad: Option<Extension<Inloggad>>,
        Form(fält): Form<Vec<(String, String)>>,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let (Ok(företag), Ok(f)) = (id.parse::<FöretagsId>(), f.parse::<KundfakturaId>()) else {
            return ingen_åtkomst();
        };
        betala(&state, &inloggad, företag, Faktura::Kund(f), &fält).await
    }

    /// #152 `POST /foretag/{id}/att-gora/leverantorsfakturor/{f}/betalning`: som
    /// [`kundbetalning`], för en leverantörsfaktura (#147).
    async fn leverantörsbetalning(
        State(state): State<Arc<AttGöraState>>,
        Path((id, f)): Path<(String, String)>,
        inloggad: Option<Extension<Inloggad>>,
        Form(fält): Form<Vec<(String, String)>>,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let (Ok(företag), Ok(f)) = (id.parse::<FöretagsId>(), f.parse::<LeverantörsfakturaId>())
        else {
            return ingen_åtkomst();
        };
        betala(&state, &inloggad, företag, Faktura::Leverantör(f), &fält).await
    }

    /// Registrerar betalningen av `faktura` och visar Att göra med verifikatets nummer, eller
    /// kortet igen med panelen och spärrarna vid fälten.
    async fn betala(
        state: &AttGöraState,
        inloggad: &Inloggad,
        företag: FöretagsId,
        faktura: Faktura,
        fält: &Vec<(String, String)>,
    ) -> Response {
        let nyckel = match faktura {
            Faktura::Kund(f) => format!("kundfaktura-{f}"),
            Faktura::Leverantör(f) => format!("leverantorsfaktura-{f}"),
        };
        let formulär = crate::betalning::ur_fält(|namn| {
            fält.iter().find(|(k, _)| k == namn).map(|(_, v)| v.clone())
        });
        match betalning::registrera(
            &state.db,
            &state.klocka,
            &inloggad.aktör,
            företag,
            faktura,
            &formulär,
        )
        .await
        {
            Ok(r) => Redirect::to(&format!("/foretag/{företag}?betald={}", r.verifikat.nummer))
                .into_response(),
            Err(BetalningFel::Spärrat(_) | BetalningFel::Verifikat(_)) => {
                crate::betalning::server::tillbaka(
                    &format!("/foretag/{företag}?oppna={nyckel}"),
                    fält,
                    &[("panel", "betalning"), ("fel", "betalning")],
                )
            }
            Err(BetalningFel::FinnsInte) => ingen_åtkomst(),
            Err(e) => crate::betalning::server::betalningsfel(e),
        }
    }

    /// Kvitterar varningen efter en återställning för den inloggade användaren.
    async fn kvittera(
        State(state): State<Arc<AttGöraState>>,
        Path((id, a)): Path<(String, String)>,
        inloggad: Option<Extension<Inloggad>>,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let (Ok(företag), Ok(a)) = (id.parse::<FöretagsId>(), a.parse::<ÅterställningsId>())
        else {
            return ingen_åtkomst();
        };
        match att_gora::kvittera_återställning(&state.db, &state.klocka, &inloggad.aktör, a).await
        {
            Ok(()) => Redirect::to(&format!("/foretag/{företag}?kvitterad=1")).into_response(),
            Err(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte) => ingen_åtkomst(),
            Err(e) => serverfel(e),
        }
    }
}

/// Att göra, företagets startsida.
#[component]
pub fn AttGöra() -> impl IntoView {
    sida(hämta_att_göra, att_göra)
}

fn att_göra(data: AttGöradata) -> AnyView {
    if !inloggad() {
        return view! { <LoggaIn /> }.into_any();
    }
    let Some(bas) = företagets_bas() else {
        return view! { <IngenÅtkomst /> }.into_any();
    };
    let företaget = {
        let val = use_context::<Företagsval>().unwrap_or_default();
        val.0
            .into_iter()
            .find(|f| f.href() == bas)
            .map(|f| format!("{} · {}", f.namn, f.orgnr))
            .unwrap_or_default()
    };
    let mut sammanfattning = Vec::new();
    match data.driftvarningar.len() {
        0 => {}
        1 => sammanfattning.push("1 driftvarning".to_string()),
        n => sammanfattning.push(format!("{n} driftvarningar")),
    }
    if data.försenade > 0 {
        sammanfattning.push(format!("{} försenade", data.försenade));
    }
    sammanfattning.push(format!("{} inom 7 dagar", data.inom_sju_dagar));
    sammanfattning.push(format!("{} utan datum", data.utan_datum));
    let sammanfattning = sammanfattning.join(" · ");
    let öppet = data.öppet.clone();
    let kort = {
        let bas = bas.clone();
        move |k: KortVy| {
            let innehåll = öppet
                .as_ref()
                .filter(|(nyckel, _)| *nyckel == k.nyckel)
                .map(|(_, ö)| ö.clone());
            view! { <Kort bas=bas.clone() k=k öppet=innehåll /> }
        }
    };
    let sida_öppen = data
        .öppet
        .as_ref()
        .is_some_and(|(_, ö)| matches!(ö, Öppet::Underlag { .. }))
        || data.öppet.as_ref().is_some_and(|(nyckel, _)| {
            data.utkast
                .iter()
                .flat_map(|p| p.kort.iter())
                .chain(data.att_granska.iter())
                .chain(data.följ_upp.iter())
                .any(|k| k.nyckel == *nyckel)
        });
    let tidslinje = if data.tidslinje.is_empty() {
        view! { <p class="diskret ag-tom">"Inget med datum."</p> }.into_any()
    } else {
        data.tidslinje
            .iter()
            .cloned()
            .map(|d| {
                let klass = if d.försenat {
                    "ag-day bad"
                } else if d.idag {
                    "ag-day idag"
                } else {
                    "ag-day"
                };
                let kort = d.kort.into_iter().map(kort.clone()).collect_view();
                view! {
                    <div class=klass>
                        {d.rubrik}
                        <span class="diskret">{d.avstånd}</span>
                    </div>
                    {kort}
                }
            })
            .collect_view()
            .into_any()
    };
    let antal_utkast: usize = data.utkast.iter().map(|p| p.kort.len()).sum();
    let utkast = (antal_utkast > 0).then(|| {
        let perioder = data
            .utkast
            .iter()
            .cloned()
            .map(|p| {
                let varning = p.varning.map(|v| {
                    view! { <span class="badge badge-varning">{v}</span> }
                });
                let kort = p.kort.into_iter().map(kort.clone()).collect_view();
                view! {
                    <div class="per">{p.rubrik} {varning}</div>
                    {kort}
                }
            })
            .collect_view();
        view! {
            <div class="ig">
                "Utkast"
                <span class="n">{antal_utkast}</span>
            </div>
            {perioder}
        }
    });
    let underlag = (!data.underlag.is_empty()).then(|| {
        let n = data.underlag.len();
        let kort = data
            .underlag
            .iter()
            .cloned()
            .map(kort.clone())
            .collect_view();
        view! {
            <div class="ig">
                "Okopplade underlag"
                <span class="n">{n}</span>
            </div>
            {kort}
        }
    });
    // #153 Att granska och Följ upp, överst i spalten.
    let grupp = |rubrik: &'static str, kort_: &[KortVy]| {
        (!kort_.is_empty()).then(|| {
            let n = kort_.len();
            let kort_ = kort_.iter().cloned().map(kort.clone()).collect_view();
            view! {
                <div class="ig">
                    {rubrik}
                    <span class="n">{n}</span>
                </div>
                {kort_}
            }
        })
    };
    let att_granska = grupp("Att granska", &data.att_granska);
    let följ_upp = grupp("Följ upp", &data.följ_upp);
    let tomt = (data.utan_datum == 0).then(|| view! { <p class="diskret">"Inget."</p> });
    let nyckeltal = data
        .nyckeltal
        .iter()
        .cloned()
        .map(|n| {
            view! {
                <div class="stat">
                    <span class="diskret">{n.namn}</span>
                    <b>{n.värde}</b>
                    <span class="sub">{n.under}</span>
                </div>
            }
        })
        .collect_view();
    let driftvarningar = (!data.driftvarningar.is_empty()).then(|| {
        let varningar = data
            .driftvarningar
            .iter()
            .cloned()
            .map(|v| view! { <Driftvarning v=v /> })
            .collect_view();
        view! { <div class="driftvarningar">{varningar}</div> }
    });
    let notis = data
        .notis
        .clone()
        .map(|t| view! { <p class="notis" role="status">{t}</p> });
    let fel = data
        .fel
        .clone()
        .map(|t| view! { <p class="fel" role="alert">{t}</p> });
    let återställningar = data
        .återställningar
        .iter()
        .cloned()
        .map(|v| view! { <VarningEfterÅterställning bas=bas.clone() v=v /> })
        .collect_view();

    view! {
        <section class="agenda">
            {återställningar}
            {notis}
            {fel}
            <div class="ag-rubrik">
                <p class="diskret">{företaget}</p>
                <h1>{data.dag.clone()}</h1>
                <p class="diskret">{sammanfattning}</p>
            </div>
            {driftvarningar}
            <div class="nyck">{nyckeltal}</div>
            <div class=if sida_öppen { "ag-grid sida-oppen" } else { "ag-grid" }>
                <div class="ag-tidslinje">{tidslinje}</div>
                <div class="ag-side">
                    <div class="ag-day">
                        "Utan datum"
                        <span class="diskret">{data.utan_datum}</span>
                    </div>
                    {att_granska}
                    {följ_upp}
                    {utkast}
                    {underlag}
                    {tomt}
                </div>
            </div>
        </section>
    }
    .into_any()
}

/// Varningen efter en återställning, med kvittensen.
#[component]
fn VarningEfterÅterställning(bas: String, v: ÅterställningVy) -> impl IntoView {
    view! {
        <div class="aterstallning" role="alert">
            <div>
                <b>"Installationen är återställd"</b>
                <p>{v.text}</p>
            </div>
            <form method="post" action=format!("{bas}/att-gora/aterstallning/{}/kvittera", v.id)>
                <button class="knapp" type="submit">
                    "Kvittera"
                </button>
            </form>
        </div>
    }
}

/// En driftvarning, med en länk till sidan Installation.
#[component]
fn Driftvarning(v: DriftvarningVy) -> impl IntoView {
    view! {
        <div class="driftvarning" role="alert">
            <span class="badge badge-drift">"drift"</span>
            <div>
                <b>{v.rubrik}</b>
                <p>{v.text}</p>
            </div>
            <a class="knapp knapp-diskret" href="/installation">
                "Installation"
            </a>
        </div>
    }
}

/// Ett kort i agendan. Rubriken fäller ut och ihop kortet.
#[component]
fn Kort(bas: String, k: KortVy, öppet: Option<Öppet>) -> impl IntoView {
    let href = if öppet.is_some() {
        bas.clone()
    } else {
        format!("{bas}?oppna={}", k.nyckel)
    };
    let märke = match k.sort {
        Sort::Utkast => view! { <UtkastBadge /> }.into_any(),
        Sort::Underlag => view! { <span class="badge badge-okopplat">"okopplat"</span> }.into_any(),
        // #152
        Sort::Kundfaktura => {
            view! { <span class="badge badge-forfallen">"förfallen"</span> }.into_any()
        }
        Sort::Leverantörsfaktura if k.när_klass == "bad" => {
            view! { <span class="badge badge-forfallen">"förfallen"</span> }.into_any()
        }
        Sort::Leverantörsfaktura => {
            view! { <span class="badge badge-obetald">"att betala"</span> }.into_any()
        }
        // #153
        Sort::MisslyckatUtskick => {
            view! { <span class="badge badge-forfallen">"studsade"</span> }.into_any()
        }
        Sort::InteSkickad => {
            view! { <span class="badge badge-obetald">"inte skickad"</span> }.into_any()
        }
        Sort::Kundfakturautkast => {
            view! { <span class="badge badge-utkast">"kundfaktura"</span> }.into_any()
        }
        Sort::ExternKundfakturautkast => {
            view! { <span class="badge badge-utkast">"extern kundfaktura"</span> }.into_any()
        }
        Sort::Leverantörsfakturautkast => {
            view! { <span class="badge badge-utkast">"leverantörsfaktura"</span> }.into_any()
        }
        Sort::NyKund => view! { <span class="badge badge-ny">"ny kund"</span> }.into_any(),
        Sort::NyLeverantör => {
            view! { <span class="badge badge-ny">"ny leverantör"</span> }.into_any()
        }
        // #150
        Sort::T2Utkast => view! { <span class="badge badge-utkast">"T2-utkast"</span> }.into_any(),
    };
    let märken = k
        .märken
        .iter()
        .cloned()
        .map(|(kod, namn)| view! { " " <span class=format!("badge badge-{kod}")>{namn}</span> })
        .collect_view();
    let gem = k.gem.then(|| view! { <Gem /> });
    let belopp = k
        .belopp
        .clone()
        .map(|b| view! { <span class="belopp">{b}</span> });
    let klass = match (öppet.is_some(), k.nedtonad) {
        (true, _) => "acard open",
        (false, true) => "acard nedtonad",
        (false, false) => "acard",
    };
    let innehåll = öppet.map(|ö| {
        let innehåll = match ö {
            Öppet::Utkast {
                dokument,
                formulär,
                okopplade,
            } => view! {
                <Formulär
                    bas=bas.clone()
                    data=formulär
                    okopplade=okopplade
                    utkast=Some(*dokument)
                    efter="att_gora"
                />
            }
            .into_any(),
            Öppet::Underlag {
                underlag,
                verifikat,
                ta_bort,
            } => view! { <VadÄrDetHär bas=bas.clone() u=*underlag verifikat=verifikat ta_bort=ta_bort /> }
                .into_any(),
            Öppet::Faktura(f) => view! { <Fakturakort f=*f /> }.into_any(),
            // #153
            Öppet::Uppföljning(u) => view! { <Uppföljningskort u=*u /> }.into_any(),
            Öppet::Granska(g) => view! { <Granskakort g=*g /> }.into_any(),
        };
        view! { <div class="ab">{innehåll}</div> }
    });
    view! {
        <div class=klass id=k.nyckel.clone()>
            <a class="ah" href=href>
                <span class="ah-text">
                    <b>{k.rubrik.clone()}</b>
                    <span class="s">{märke} {märken} {gem}</span>
                </span>
                <span class="ah-hoger">
                    {belopp}
                    <span class=format!("when {}", k.när_klass)>{k.när.clone()}</span>
                </span>
            </a>
            {innehåll}
        </div>
    }
}

/// "Vad är det här?" för ett okopplat underlag: Kvitto, Komplettering eller Dubblett eller fel
/// fil.
#[component]
fn VadÄrDetHär(
    bas: String,
    u: UnderlagVy,
    verifikat: Vec<(String, String)>,
    ta_bort: bool,
) -> impl IntoView {
    let väg = format!("{bas}/att-gora/underlag/{}", u.id);
    let komplettering = if verifikat.is_empty() {
        view! {
            <p class="diskret">"Komplettering: det finns inget bokfört verifikat att komplettera."</p>
        }
        .into_any()
    } else {
        let val = verifikat
            .into_iter()
            .map(|(id, text)| view! { <option value=id>{text}</option> })
            .collect_view();
        view! {
            <form class="tchoice-form" method="post" action=format!("{väg}/komplettera")>
                <label for="komplettera-verifikat">"Komplettering av"</label>
                <select id="komplettera-verifikat" name="verifikat">
                    {val}
                </select>
                <button class="knapp" type="submit">
                    <Gem />
                    "Komplettering"
                </button>
            </form>
        }
        .into_any()
    };
    let borttagning = if ta_bort {
        view! {
            <div class="inl">
                <b>"Dubblett eller fel fil"</b>
                <p class="diskret">"Underlaget tas bort ur arkivet. Borttagningen loggas."</p>
                <form method="post" action=format!("{väg}/ta-bort") class="knappar">
                    <button class="knapp knapp-fara" type="submit">
                        "Ta bort underlaget"
                    </button>
                    <a class="knapp" href=format!("{bas}?oppna=underlag-{}", u.id)>
                        "Avbryt"
                    </a>
                </form>
            </div>
        }
        .into_any()
    } else {
        view! {
            <a
                class="knapp knapp-diskret"
                href=format!("{bas}?oppna=underlag-{}&ta_bort=1", u.id)
            >
                "Dubblett eller fel fil…"
            </a>
        }
        .into_any()
    };
    let (uppladdad, av) = u.uppladdad.clone();
    view! {
        <div class="triage">
            <div class="triage-visning">
                <Förhandsvisning bas=bas.clone() u=u.clone() />
            </div>
            <div class="tchoice">
                <h2>"Vad är det här?"</h2>
                <form method="post" action=format!("{väg}/kvitto")>
                    <button class="knapp" type="submit">
                        "Kvitto"
                    </button>
                    <span class="diskret">" Ett nytt verifikatutkast med underlaget."</span>
                </form>
                <form method="post" action=format!("{väg}/leverantorsfaktura")>
                    <button class="knapp" type="submit">
                        "Leverantörsfaktura"
                    </button>
                    <span class="diskret">
                        " Något som ska betalas. Ett utkast med underlaget som original."
                    </span>
                </form>
                {komplettering}
                {borttagning}
                <p class="diskret">
                    {format!("Uppladdat {uppladdad} av {av} · arkiverat med SHA-256")}
                </p>
            </div>
        </div>
    }
}

/// #152: en utfälld faktura i tidslinjen: det som återstår, kopieringschipsen och
/// betalningspanelen, eller länken som fäller ut den.
#[component]
fn Fakturakort(f: FakturakortVy) -> impl IntoView {
    let kontakt = f.kund.then(|| {
        let kontakt = if f.kontakt.is_empty() {
            "ingen e-post på fakturan".to_string()
        } else {
            f.kontakt.clone()
        };
        view! {
            <dt>"Kontakt"</dt>
            <dd>{kontakt}</dd>
        }
    });
    let påminnelser = f.kund.then(|| {
        view! { <p class="diskret">"Maya skickar inga påminnelser. Kontakta kunden själv."</p> }
    });
    let chips = (!f.kopieringschips.is_empty())
        .then(|| view! { <Kopieringschips chips=f.kopieringschips.clone() /> });
    let handlingar = match f.betalning.clone() {
        Some((action, stäng, v)) => {
            view! { <Betalningspanel action=action stäng=stäng v=v /> }.into_any()
        }
        None => {
            let betala = f.betala.clone().map(|href| {
                view! {
                    <a class="knapp knapp-primar" href=href>
                        "Registrera betalning…"
                    </a>
                }
            });
            view! {
                <p class="knappar">
                    {betala}
                    <a class="knapp" href=f.dokument.clone()>
                        "Öppna fakturan"
                    </a>
                </p>
            }
            .into_any()
        }
    };
    view! {
        <dl class="fakturakort">
            <dt>"Kvar att betala"</dt>
            <dd>
                <b>{format!("{} kr", f.återstår)}</b>
            </dd>
            <dt>"Förfallodag"</dt>
            <dd>{f.förfallodag.clone()}</dd>
            {kontakt}
        </dl>
        {chips}
        {påminnelser}
        {handlingar}
    }
}

/// #152: knappar som kopierar betaluppgifterna till urklipp. Utan JavaScript går texten att
/// markera.
#[component]
fn Kopieringschips(chips: Vec<(String, String)>) -> impl IntoView {
    let chips = chips
        .into_iter()
        .map(|(etikett, värde)| {
            let text = värde.clone();
            view! {
                <button
                    type="button"
                    class="copy"
                    title="Kopiera"
                    on:click=move |_| kopiera(&text)
                >
                    <span class="k">{etikett}</span>
                    {värde}
                </button>
            }
        })
        .collect_view();
    view! { <div class="chips">{chips}</div> }
}

/// Lägger `text` i urklipp i webbläsaren.
fn kopiera(text: &str) {
    #[cfg(feature = "hydrate")]
    if let Some(fönster) = web_sys::window() {
        let _ = fönster.navigator().clipboard().write_text(text);
    }
    #[cfg(not(feature = "hydrate"))]
    let _ = text;
}

/// #153: en kundfaktura under Följ upp med panelen Skicka från fakturans dokument, så att den
/// skickas igen eller markeras som lämnad på annat sätt härifrån.
#[component]
fn Uppföljningskort(u: UppföljningVy) -> impl IntoView {
    let lista = u
        .dokument
        .split_once('?')
        .map(|(l, _)| l.to_string())
        .unwrap_or_default();
    view! {
        <SkickaPanel lista=lista id=u.id.clone() s=u.skicka att_göra=(u.kortet, u.stäng) />
        <p class="knappar">
            <a class="knapp" href=u.dokument>
                "Öppna fakturan"
            </a>
        </p>
    }
}

/// #153: ett fakturautkast, eller en ny kund eller leverantör som ett streckat kort, med länken
/// till utkastet i sin flik.
#[component]
fn Granskakort(g: GranskaVy) -> impl IntoView {
    let uppgifter = g
        .uppgifter
        .into_iter()
        .map(|(k, v)| {
            view! {
                <dt>{k}</dt>
                <dd>{v}</dd>
            }
        })
        .collect_view();
    let klass = if g.streckad {
        "kort-streckat granska-kort"
    } else {
        "granska-kort"
    };
    view! {
        <div class=klass>
            <dl class="fakturakort">{uppgifter}</dl>
            <p class="knappar">
                <a class="knapp knapp-primar" href=g.länk>
                    {g.knapp}
                </a>
            </p>
            <p class="diskret">{g.not}</p>
        </div>
    }
}
