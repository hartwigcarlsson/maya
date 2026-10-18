//! Fliken Verifikat: lista och detalj sida vid sida, formuläret för ett nytt verifikat och det
//! bokförda verifikatet som ett dokument (variant C i `prototype/32-verifikat`).
//!
//! Formuläret prövar spärrarna med `maya_domain::verifikat` i webbläsaren, och `core` prövar
//! samma spärrar när det bokför. Utan JavaScript skickas formuläret till servern, som skickar
//! tillbaka hit med det användaren skrev; sidan prövar det igen och visar samma koder.
//!
//! Sidans data hämtas med `sidodata::sida`, så länkarna navigerar i routern på klienten.

use leptos::prelude::*;
use leptos_router::hooks::{use_params_map, use_query_map};
use maya_domain::att_gora::{Frist, Utkastläge, dag_lång, frist, rör_kassan};
use maya_domain::belopp::Belopp;
use maya_domain::kontoplan::Kontonummer;
use maya_domain::rakenskapsar::Räkenskapsår;
use maya_domain::rattelse::{self, Original};
use maya_domain::verifikat::{Fält, Förutsättningar, NyRad, NyttVerifikat, Rad, Spärr};
use serde::{Deserialize, Serialize};

use crate::app::{LoggaIn, Läge};
use crate::foretag::{Företagsval, IngenÅtkomst};
use crate::sidodata::sida;
use crate::underlag::{Förhandsvisning, Gem, UnderlagVy, feltext};
use crate::utskrift::Utskrift;

/// Ett verifikat i listan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListVy {
    pub id: String,
    pub nummer: Option<String>,
    pub datum: String,
    pub beskrivning: String,
    pub summa: String,
}

/// En rad i ett verifikat, i dokumentet, grundboken och panelerna.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RadVy {
    pub konto: String,
    pub kontonamn: String,
    pub debet: String,
    pub kredit: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistorikVy {
    pub tid: String,
    pub typ: String,
    pub namn: String,
    pub kanal: String,
}

/// Ett verifikat som dokument, eller ett utkast som formuläret öppnar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DokumentVy {
    pub id: String,
    pub nummer: Option<String>,
    pub beskrivning: String,
    pub summa: String,
    pub datum: String,
    pub period: String,
    /// Tid, namn och kanal.
    pub bokförd: Option<(String, String, String)>,
    pub skapad: (String, String, String),
    pub rader: Vec<RadVy>,
    pub debet: String,
    pub kredit: String,
    pub historik: Vec<HistorikVy>,
    /// Kopplade underlag, kompletteringar sist.
    pub underlag: Vec<UnderlagVy>,
    /// PDF:ens adress och filnamn, för Skriv ut och Exportera PDF.
    pub pdf: (String, String),
    /// Verifikatet som det här rättar, för en rättelse.
    pub rättar: Option<LänkVy>,
    /// Rättelsen av det här verifikatet, om det är rättat.
    pub rättad_av: Option<LänkVy>,
    /// Verifikatet som ett utkast från "registrera om" är en kopia av.
    pub kopia_av: Option<LänkVy>,
    /// Varför ett bokfört verifikat inte kan rättas.
    pub inte_rättbart: Option<String>,
    /// Det som Rätta… behöver, när verifikatet kan rättas.
    pub rättelse: Option<RättelseVy>,
}

/// En länk till ett annat verifikat, med vem som bokförde det och när.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LänkVy {
    pub id: String,
    /// `V5`, eller tomt för ett utkast.
    pub nummer: String,
    /// Tid, namn och kanal.
    pub bokförd: Option<(String, String, String)>,
}

/// Det som rättelsen i dokumentet prövas mot, i webbläsaren och på servern med samma regler
/// (`maya_domain::rattelse`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RättelseVy {
    /// Originalets nummer och datum.
    pub nummer: String,
    pub datum: String,
    /// Dagens datum, eller originalets om det är senare.
    pub förvalt: String,
    /// Originalets rader: konto, debet och kredit i öre, och text.
    pub rader: Vec<(u16, i64, i64, Option<String>)>,
    /// Räkenskapsårens start och slut.
    pub räkenskapsår: Vec<(String, String)>,
    /// Radernas konton och om de är aktiva.
    pub konton: Vec<(u16, bool)>,
}

impl RättelseVy {
    fn original(&self) -> Option<Original> {
        Some(Original {
            nummer: self.nummer.clone(),
            datum: self.datum.parse().ok()?,
            rader: self
                .rader
                .iter()
                .map(|(konto, debet, kredit, text)| {
                    Some(Rad {
                        konto: Kontonummer::new(*konto)?,
                        debet: Belopp::från_öre(*debet),
                        kredit: Belopp::från_öre(*kredit),
                        text: text.clone(),
                    })
                })
                .collect::<Option<_>>()?,
            rättad_av: None,
            rättar: None,
        })
    }

    fn förutsättningar(&self) -> Förutsättningar {
        Förutsättningar {
            konton: self
                .konton
                .iter()
                .filter_map(|(nr, aktiv)| Some((Kontonummer::new(*nr)?, *aktiv)))
                .collect(),
            räkenskapsår: self
                .räkenskapsår
                .iter()
                .filter_map(|(start, slut)| {
                    Some(Räkenskapsår::befintligt(
                        start.parse().ok()?,
                        slut.parse().ok()?,
                    ))
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KontoVy {
    pub nummer: u16,
    pub namn: String,
    pub aktiv: bool,
}

/// Det formuläret prövar spärrarna mot: kontoplanen och räkenskapsåren.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormulärData {
    pub konton: Vec<KontoVy>,
    /// Start och slut, `ÅÅÅÅ-MM-DD`.
    pub räkenskapsår: Vec<(String, String)>,
    /// Underlagen som formuläret har kopplade när det öppnas: utkastets, eller de som skickades
    /// tillbaka med formuläret.
    pub valda: Vec<UnderlagVy>,
    /// Dagens datum, `ÅÅÅÅ-MM-DD`, för kontantfristens notis. Tomt: ingen notis.
    pub idag: String,
}

impl FormulärData {
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
                .filter_map(|(start, slut)| {
                    Some(Räkenskapsår::befintligt(
                        start.parse().ok()?,
                        slut.parse().ok()?,
                    ))
                })
                .collect(),
        }
    }
}

/// Det som fliken Verifikat visar.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verifikatdata {
    pub lista: Vec<ListVy>,
    pub valt: Option<String>,
    pub dokument: Option<DokumentVy>,
    pub formulär: Option<FormulärData>,
    /// Okopplade underlag att koppla till ett utkast eller komplettera ett verifikat med.
    pub okopplade: Vec<UnderlagVy>,
}

/// Datan för `/foretag/<id>/verifikat` och `/foretag/<id>/verifikat/nytt`.
#[server(prefix = "/api", endpoint = "verifikatdata", input = server_fn::codec::GetUrl)]
pub async fn hämta_verifikatdata(adress: String) -> Result<Verifikatdata, ServerFnError> {
    match crate::sidodata::Sidbegäran::för(&adress)? {
        Some(begäran) => server::verifikatdata(&begäran).await,
        None => Ok(Verifikatdata::default()),
    }
}

pub use maya_domain::rakenskapsar::periodnamn;

#[cfg(feature = "ssr")]
pub(crate) mod server {
    use crate::svar::{ingen_åtkomst, serverfel};
    use std::sync::Arc;

    use axum::extract::{DefaultBodyLimit, Path, Request, State};
    use axum::http::{StatusCode, header};
    use axum::response::{IntoResponse, Redirect, Response};
    use axum::routing::post;
    use axum::{Extension, Router};
    use leptos::prelude::ServerFnError;
    use maya_core::atkomst::ÅtkomstFel;
    use maya_core::foretag::FöretagsId;
    use maya_core::inloggning::Inloggad;
    use maya_core::underlag::{self, UnderlagFel, UnderlagId};
    use maya_core::verifikat::{
        self, Länk, Rättelseval, Signatur, Urval, VerifikatFel, VerifikatId,
    };
    use maya_core::{Databas, Klocka, kontoplan};
    use maya_domain::belopp::Belopp;
    use maya_domain::verifikat::{NyRad, NyttVerifikat};

    use super::{
        DokumentVy, FormulärData, HistorikVy, KontoVy, ListVy, LänkVy, RadVy, RättelseVy,
        Verifikatdata, periodnamn,
    };
    use crate::sidodata::{self, Sidbegäran};
    use crate::underlag::server::{
        STÖRSTA_BEGÄRAN, läs_formulär, underlagsfel, verifikatets_vy, vy,
    };

    #[derive(Clone)]
    pub(crate) struct VerifikatState {
        pub db: Databas,
        pub klocka: Klocka,
    }

    pub(crate) fn routes(state: VerifikatState) -> Router {
        Router::new()
            .route("/foretag/{id}/verifikat", post(skicka))
            .route("/foretag/{id}/verifikat/{utkast}/radera", post(radera))
            .route("/foretag/{id}/verifikat/{v}/komplettera", post(komplettera))
            .route("/foretag/{id}/verifikat/{v}/ratta", post(rätta))
            .layer(DefaultBodyLimit::max(STÖRSTA_BEGÄRAN))
            .with_state(Arc::new(state))
    }

    /// Hämtar det som fliken Verifikat visar, för `/foretag/<id>/verifikat` och
    /// `/foretag/<id>/verifikat/nytt`. Utan åtkomst blir datan tom; sidan visar det ur
    /// företagsvalet.
    pub(crate) async fn verifikatdata(b: &Sidbegäran) -> Result<Verifikatdata, ServerFnError> {
        let (företag, nytt) = match b.delar().as_slice() {
            ["foretag", id, "verifikat"] => (id.parse::<FöretagsId>().ok(), false),
            ["foretag", id, "verifikat", "nytt"] => (id.parse::<FöretagsId>().ok(), true),
            _ => (None, false),
        };
        let Some(företag) = företag else {
            return Ok(Verifikatdata::default());
        };
        let urval = b
            .värde("urval")
            .and_then(|u| Urval::från_kod(&u))
            .unwrap_or_default();
        let valt = if nytt { None } else { b.värde("v") };
        // Formuläret som skickades tillbaka har sina underlag i adressen.
        let tillbakaskickade = b.värde("datum").map(|_| b.värden("underlag"));
        match hämta(
            &b.db,
            b.klocka.idag(),
            &b.inloggad,
            företag,
            urval,
            valt,
            nytt,
            tillbakaskickade,
        )
        .await
        {
            Ok(mut data) => {
                if let Some(f) = &mut data.formulär {
                    f.idag = b.klocka.idag().to_string();
                }
                Ok(data)
            }
            Err(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte) => {
                Ok(Verifikatdata::default())
            }
            Err(e) => Err(sidodata::serverfel(e)),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn hämta(
        db: &Databas,
        idag: jiff::civil::Date,
        inloggad: &Inloggad,
        företag: FöretagsId,
        urval: Urval,
        valt: Option<String>,
        nytt: bool,
        tillbakaskickade: Option<Vec<String>>,
    ) -> Result<Verifikatdata, ÅtkomstFel> {
        let aktör = &inloggad.aktör;
        let lista = verifikat::lista(db, aktör, företag, urval)
            .await?
            .into_iter()
            .map(|v| ListVy {
                id: v.id.to_string(),
                nummer: v.nummer.map(|n| n.to_string()),
                datum: v.datum.map(|d| d.to_string()).unwrap_or_default(),
                beskrivning: v.beskrivning,
                summa: v.summa.to_string(),
            })
            .collect();
        let konton = kontoplan::konton(db, aktör, företag).await?;
        let kontonamn = |nr: u16| {
            konton
                .iter()
                .find(|k| k.nummer.get() == nr)
                .map(|k| k.namn.clone())
                .unwrap_or_default()
        };
        let mut dokument = None;
        if let Some(id) = valt.as_deref().and_then(|v| v.parse::<VerifikatId>().ok())
            && let Some(v) = verifikat::verifikatet(db, aktör, företag, id).await?
        {
            let summa = |f: fn(&verifikat::Rad) -> Belopp| {
                Belopp::från_öre(v.rader.iter().map(|r| f(r).öre()).sum()).to_string()
            };
            let original = v.som_original();
            let inte_rättbart = original
                .as_ref()
                .and_then(|o| o.kan_rättas().err())
                .map(|spärr| spärr.to_string());
            let rättelse = match original.filter(|o| o.kan_rättas().is_ok()) {
                Some(o) => {
                    let förutsättningar = verifikat::förutsättningar(db, aktör, företag).await?;
                    Some(RättelseVy {
                        nummer: o.nummer.clone(),
                        datum: o.datum.to_string(),
                        förvalt: idag.max(o.datum).to_string(),
                        rader: o
                            .rader
                            .iter()
                            .map(|r| (r.konto.get(), r.debet.öre(), r.kredit.öre(), r.text.clone()))
                            .collect(),
                        räkenskapsår: förutsättningar
                            .räkenskapsår
                            .iter()
                            .map(|å| (å.start().to_string(), å.slut().to_string()))
                            .collect(),
                        konton: förutsättningar
                            .konton
                            .iter()
                            .filter(|(k, _)| o.rader.iter().any(|r| r.konto == *k))
                            .map(|(k, aktiv)| (k.get(), *aktiv))
                            .collect(),
                    })
                }
                None => None,
            };
            dokument = Some(DokumentVy {
                rättar: v.rättar.as_ref().map(länk),
                rättad_av: v.rättad_av.as_ref().map(länk),
                kopia_av: v.kopia_av.as_ref().map(länk),
                inte_rättbart,
                rättelse,
                id: v.id.to_string(),
                pdf: (
                    format!("/foretag/{företag}/verifikat/{}/pdf", v.id),
                    maya_core::utskrift::verifikatets_filnamn(v.nummer),
                ),
                nummer: v.nummer.map(|n| n.to_string()),
                beskrivning: v.beskrivning.clone(),
                summa: v.summa.to_string(),
                datum: v.datum.map(|d| d.to_string()).unwrap_or_default(),
                period: v.datum.map(periodnamn).unwrap_or_default(),
                bokförd: v.bokförd.as_ref().map(signatur),
                skapad: signatur(&v.skapad),
                debet: summa(|r| r.debet),
                kredit: summa(|r| r.kredit),
                rader: v
                    .rader
                    .iter()
                    .map(|r| RadVy {
                        konto: r.konto.map(|k| k.to_string()).unwrap_or_default(),
                        kontonamn: r.konto.map(|k| kontonamn(k.get())).unwrap_or_default(),
                        debet: r.debet.eller_tomt(),
                        kredit: r.kredit.eller_tomt(),
                        text: r.text.clone().unwrap_or_default(),
                    })
                    .collect(),
                historik: v
                    .historik
                    .iter()
                    .map(|h| HistorikVy {
                        tid: tid(h.post.tid),
                        typ: h.post.typ.clone(),
                        namn: h.namn.clone(),
                        kanal: h.post.aktör.kanal.namn().to_string(),
                    })
                    .collect(),
                underlag: v.underlag.iter().map(verifikatets_vy).collect(),
            });
        }
        let alla_underlag = if dokument.is_some() || nytt {
            underlag::lista(db, aktör, företag, underlag::Urval::Alla).await?
        } else {
            Vec::new()
        };
        let okopplade = alla_underlag
            .iter()
            .filter(|u| u.kopplingar.is_empty())
            .map(vy)
            .collect();
        let utkast = dokument.as_ref().is_some_and(|d| d.nummer.is_none());
        let valda = match tillbakaskickade {
            Some(ids) => ids
                .iter()
                .filter_map(|id| alla_underlag.iter().find(|u| u.id.to_string() == *id))
                .map(vy)
                .collect(),
            None if utkast => dokument
                .as_ref()
                .map(|d| d.underlag.clone())
                .unwrap_or_default(),
            None => Vec::new(),
        };
        let formulär = if nytt || utkast {
            let förutsättningar = verifikat::förutsättningar(db, aktör, företag).await?;
            Some(FormulärData {
                konton: konton
                    .iter()
                    .map(|k| KontoVy {
                        nummer: k.nummer.get(),
                        namn: k.namn.clone(),
                        aktiv: k.aktiv,
                    })
                    .collect(),
                räkenskapsår: förutsättningar
                    .räkenskapsår
                    .iter()
                    .map(|å| (å.start().to_string(), å.slut().to_string()))
                    .collect(),
                valda,
                idag: String::new(),
            })
        } else {
            None
        };
        Ok(Verifikatdata {
            lista,
            valt: dokument.as_ref().and(valt),
            dokument,
            formulär,
            okopplade,
        })
    }

    pub(crate) use maya_domain::tid::minut as tid;

    fn signatur(s: &Signatur) -> (String, String, String) {
        (tid(s.tid), s.namn.clone(), s.aktör.kanal.namn().to_string())
    }

    fn länk(l: &Länk) -> LänkVy {
        LänkVy {
            id: l.id.to_string(),
            nummer: l.nummer.map(|n| n.to_string()).unwrap_or_default(),
            bokförd: l.bokförd.as_ref().map(signatur),
        }
    }

    /// Formuläret, med raderna som `konto_<i>`, `debet_<i>`, `kredit_<i>` och `text_<i>`.
    pub(crate) fn tolka(fält: &[(String, String)]) -> NyttVerifikat {
        let värde = |namn: &str| {
            fält
                .iter()
                .find(|(k, _)| k == namn)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        let antal = fält
            .iter()
            .filter_map(|(k, _)| k.rsplit_once('_')?.1.parse::<usize>().ok())
            .map(|i| i + 1)
            .max()
            .unwrap_or(0)
            .min(500);
        NyttVerifikat {
            datum: värde("datum"),
            beskrivning: värde("beskrivning"),
            rader: (0..antal)
                .map(|i| NyRad {
                    konto: värde(&format!("konto_{i}")),
                    debet: värde(&format!("debet_{i}")),
                    kredit: värde(&format!("kredit_{i}")),
                    text: värde(&format!("text_{i}")),
                })
                .collect(),
        }
    }

    /// Formuläret: Bokför (`handling=bokfor`) eller Spara utkast (`handling=utkast`), för ett
    /// nytt verifikat eller för utkastet i fältet `utkast`.
    ///
    /// Underlagen skickas som `underlag=<id>`, ett fält per underlag, tillsammans med
    /// `underlag_finns=1`; utan det rörs inte utkastets kopplingar. En fil i `fil` laddas upp
    /// först och kopplas. `koppla=<id>`, `koppla_bort=<id>` och `handling=ladda_upp` ändrar bara
    /// vilka underlag formuläret har och skickar tillbaka det (sidan utan JavaScript).
    async fn skicka(
        State(state): State<Arc<VerifikatState>>,
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
        let skickat = match läs_formulär(req).await {
            Ok(f) => f,
            Err(r) => return *r,
        };
        let utkast = match skickat.värde("utkast").filter(|u| !u.is_empty()) {
            Some(u) => match u.parse::<VerifikatId>() {
                Ok(u) => Some(u),
                Err(_) => return inget_utkast(),
            },
            None => None,
        };
        let handling = skickat.värde("handling");
        let spara = handling == Some("utkast");
        let till_att_göra = skickat.värde("efter") == Some("att_gora");
        let formulär = tolka(&skickat.fält);
        let (db, klocka, aktör) = (&state.db, &state.klocka, &inloggad.aktör);
        let mut valda = Vec::new();
        for u in skickat.värden("underlag") {
            if !valda.contains(&u) {
                valda.push(u);
            }
        }
        let tillbaka_med = |fel: Option<&str>, valda: &[String], dubblett: Option<UnderlagId>| {
            tillbaka(företag, utkast, fel, &formulär, Some(valda), dubblett)
        };
        if skickat.för_stort {
            return tillbaka_med(Some("for_stort"), &valda, None);
        }
        let mut dubblett = None;
        if let Some(fil) = &skickat.fil {
            match underlag::ladda_upp(
                db,
                klocka,
                aktör,
                företag,
                &fil.filnamn,
                &fil.mime,
                &fil.innehåll,
            )
            .await
            {
                Ok(u) => {
                    valda.push(u.id.to_string());
                    if !u.dubbletter.is_empty() {
                        dubblett = Some(u.id);
                    }
                }
                Err(UnderlagFel::FörStort) => {
                    return tillbaka_med(Some("for_stort"), &valda, None);
                }
                Err(e) => return underlagsfel(e),
            }
        }
        let koppla = skickat.värde("koppla").filter(|u| !u.is_empty());
        let koppla_bort = skickat.värde("koppla_bort");
        if let Some(u) = koppla
            && !valda.iter().any(|v| v == u)
        {
            valda.push(u.to_string());
        }
        if let Some(u) = koppla_bort {
            valda.retain(|v| v != u);
        }
        if koppla.is_some() || koppla_bort.is_some() || handling == Some("ladda_upp") {
            return tillbaka_med(None, &valda, dubblett);
        }
        let hanterar_underlag = skickat.värde("underlag_finns").is_some() || skickat.fil.is_some();
        let Ok(ids) = valda
            .iter()
            .map(|u| u.parse::<UnderlagId>())
            .collect::<Result<Vec<_>, _>>()
        else {
            return tillbaka_med(Some("okant_underlag"), &[], None);
        };
        let underlag = hanterar_underlag.then_some(ids.as_slice());
        let resultat = match (spara, underlag) {
            (true, Some(u)) => verifikat::spara_utkast_med_underlag(
                db, klocka, aktör, företag, utkast, &formulär, u,
            )
            .await
            .map(|id| format!("/foretag/{företag}/verifikat?v={id}&sparat=1")),
            (true, None) => verifikat::spara_utkast(db, klocka, aktör, företag, utkast, &formulär)
                .await
                .map(|id| format!("/foretag/{företag}/verifikat?v={id}&sparat=1")),
            (false, Some(u)) => {
                verifikat::bokför_med_underlag(db, klocka, aktör, företag, utkast, &formulär, u)
                    .await
                    .map(|b| format!("/foretag/{företag}/verifikat?v={}", b.id))
            }
            (false, None) => match utkast {
                Some(u) => verifikat::bokför_utkast(db, klocka, aktör, företag, u, &formulär)
                    .await
                    .map(|b| format!("/foretag/{företag}/verifikat?v={}", b.id)),
                None => verifikat::bokför(db, klocka, aktör, företag, &formulär)
                    .await
                    .map(|b| format!("/foretag/{företag}/verifikat?v={}", b.id)),
            },
        };
        match resultat {
            Ok(plats) if till_att_göra => {
                Redirect::to(&efter_att_göra(företag, &plats, spara)).into_response()
            }
            Ok(plats) => {
                let plats = match dubblett {
                    Some(u) => format!("{plats}&dubblett={u}"),
                    None => plats,
                };
                Redirect::to(&plats).into_response()
            }
            Err(VerifikatFel::Spärrat(_)) => tillbaka(
                företag,
                utkast,
                spara.then_some("utkast"),
                &formulär,
                hanterar_underlag.then_some(valda.as_slice()),
                dubblett,
            ),
            Err(VerifikatFel::OkäntUnderlag) => tillbaka_med(Some("okant_underlag"), &[], None),
            Err(VerifikatFel::IngetUtkast) => inget_utkast(),
            Err(VerifikatFel::Åtkomst(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte)) => {
                ingen_åtkomst()
            }
            Err(e) => serverfel(e),
        }
    }

    /// Tillbaka till Att göra efter Spara utkast (utkastet utfällt) eller Bokför (nästa utkast
    /// utfällt). `plats` är adressen till verifikatet, med `v=<id>`.
    fn efter_att_göra(företag: FöretagsId, plats: &str, spara: bool) -> String {
        let id = plats
            .split("v=")
            .nth(1)
            .and_then(|rest| rest.split('&').next())
            .unwrap_or_default();
        if spara {
            format!("/foretag/{företag}?oppna=utkast-{id}&sparat=1")
        } else {
            format!("/foretag/{företag}?bokford={id}")
        }
    }

    /// Komplettera ett bokfört verifikat med ett okopplat underlag (`underlag=<id>`) eller en ny
    /// fil (`fil`).
    async fn komplettera(
        State(state): State<Arc<VerifikatState>>,
        Path((id, v)): Path<(String, String)>,
        inloggad: Option<Extension<Inloggad>>,
        req: Request,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let Ok(företag) = id.parse::<FöretagsId>() else {
            return ingen_åtkomst();
        };
        let Ok(v) = v.parse::<VerifikatId>() else {
            return inget_utkast();
        };
        let skickat = match läs_formulär(req).await {
            Ok(f) => f,
            Err(r) => return *r,
        };
        let sida = format!("/foretag/{företag}/verifikat?v={v}");
        if skickat.för_stort {
            return Redirect::to(&format!("{sida}&fel=for_stort")).into_response();
        }
        let (db, klocka, aktör) = (&state.db, &state.klocka, &inloggad.aktör);
        let resultat = match (&skickat.fil, skickat.värde("underlag")) {
            (Some(fil), _) => underlag::ladda_upp_och_komplettera(
                db,
                klocka,
                aktör,
                företag,
                v,
                (&fil.filnamn, &fil.mime, &fil.innehåll),
            )
            .await
            .map(|u| (!u.dubbletter.is_empty()).then_some(u.id)),
            (None, Some(u)) => match u.parse::<UnderlagId>() {
                Ok(u) => underlag::komplettera(db, klocka, aktör, företag, v, u)
                    .await
                    .map(|()| None),
                Err(_) => Err(UnderlagFel::FinnsInte),
            },
            (None, None) => {
                return Redirect::to(&format!("{sida}&fel=ingen_fil")).into_response();
            }
        };
        match resultat {
            Ok(dubblett) => {
                let dubblett = dubblett
                    .map(|u| format!("&dubblett={u}"))
                    .unwrap_or_default();
                Redirect::to(&format!("{sida}&kompletterat=1{dubblett}")).into_response()
            }
            Err(UnderlagFel::FörStort) => {
                Redirect::to(&format!("{sida}&fel=for_stort")).into_response()
            }
            Err(UnderlagFel::FinnsInte) => {
                Redirect::to(&format!("{sida}&fel=okant_underlag")).into_response()
            }
            Err(UnderlagFel::RedanKopplat) => {
                Redirect::to(&format!("{sida}&fel=redan_kopplat")).into_response()
            }
            Err(UnderlagFel::IngetBokförtVerifikat) => inget_utkast(),
            Err(e) => underlagsfel(e),
        }
    }

    /// Rätta ett bokfört verifikat: `datum`, `orsak` och `handling=registrera_om` (Bokför
    /// rättelse och registrera om) eller `handling=bokfor` (Bara bokför rättelse). Med en spärr
    /// går det tillbaka till dokumentet med rättelsen öppen och det användaren skrev; sidan
    /// prövar det igen och visar spärren.
    async fn rätta(
        State(state): State<Arc<VerifikatState>>,
        Path((id, v)): Path<(String, String)>,
        inloggad: Option<Extension<Inloggad>>,
        req: Request,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let Ok(företag) = id.parse::<FöretagsId>() else {
            return ingen_åtkomst();
        };
        let Ok(original) = v.parse::<VerifikatId>() else {
            return inget_verifikat();
        };
        let skickat = match läs_formulär(req).await {
            Ok(f) => f,
            Err(r) => return *r,
        };
        let datum = skickat.värde("datum").unwrap_or_default().to_string();
        let orsak = skickat.värde("orsak").unwrap_or_default().to_string();
        let val = match skickat.värde("handling") {
            Some("registrera_om") => Rättelseval::RegistreraOm,
            _ => Rättelseval::BaraRättelse,
        };
        let resultat = verifikat::rätta(
            &state.db,
            &state.klocka,
            &inloggad.aktör,
            företag,
            original,
            &datum,
            &orsak,
            val,
        )
        .await;
        let sida = format!("/foretag/{företag}/verifikat");
        match resultat {
            Ok(rättad) => {
                let nummer = rättad.rättelse.nummer;
                let plats = match rättad.kopia {
                    Some(kopia) => format!("{sida}?v={kopia}&rattad={nummer}"),
                    None => format!("{sida}?v={}&rattad={nummer}", rättad.rättelse.id),
                };
                Redirect::to(&plats).into_response()
            }
            Err(VerifikatFel::Spärrat(spärrar)) => {
                let mut query = url::form_urlencoded::Serializer::new(String::new());
                query
                    .append_pair("v", &original.to_string())
                    .append_pair("ratta", "1")
                    .append_pair("datum", &datum)
                    .append_pair("orsak", &orsak);
                if let Some(s) = spärrar.first() {
                    query.append_pair("fel", s.kod);
                }
                Redirect::to(&format!("{sida}?{}", query.finish())).into_response()
            }
            // Dokumentet visar varför verifikatet inte kan rättas.
            Err(VerifikatFel::InteRättbart(_)) => {
                Redirect::to(&format!("{sida}?v={original}")).into_response()
            }
            Err(VerifikatFel::IngetVerifikat) => inget_verifikat(),
            Err(VerifikatFel::Åtkomst(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte)) => {
                ingen_åtkomst()
            }
            Err(e) => serverfel(e),
        }
    }

    /// Radera utkast, efter att sidan har frågat och visat en sammanfattning.
    async fn radera(
        State(state): State<Arc<VerifikatState>>,
        Path((id, utkast)): Path<(String, String)>,
        inloggad: Option<Extension<Inloggad>>,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let Ok(företag) = id.parse::<FöretagsId>() else {
            return ingen_åtkomst();
        };
        let Ok(utkast) = utkast.parse::<VerifikatId>() else {
            return inget_utkast();
        };
        match verifikat::radera_utkast(&state.db, &state.klocka, &inloggad.aktör, företag, utkast)
            .await
        {
            Ok(()) => Redirect::to(&format!(
                "/foretag/{företag}/verifikat?urval=utkast&raderat=1"
            ))
            .into_response(),
            Err(VerifikatFel::IngetUtkast | VerifikatFel::Spärrat(_)) => inget_utkast(),
            Err(VerifikatFel::Åtkomst(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte)) => {
                ingen_åtkomst()
            }
            Err(e) => serverfel(e),
        }
    }

    /// Tillbaka till formuläret med det användaren skrev, till utkastet om det fanns ett. Sidan
    /// prövar det igen med samma spärrar och visar dem vid fälten. `valda` är formulärets
    /// underlag, när formuläret hanterar dem.
    fn tillbaka(
        företag: FöretagsId,
        utkast: Option<VerifikatId>,
        fel: Option<&str>,
        f: &NyttVerifikat,
        valda: Option<&[String]>,
        dubblett: Option<UnderlagId>,
    ) -> Response {
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        if let Some(u) = utkast {
            query.append_pair("v", &u.to_string());
        }
        if let Some(fel) = fel {
            query.append_pair("fel", fel);
        }
        if let Some(u) = dubblett {
            query.append_pair("dubblett", &u.to_string());
        }
        query
            .append_pair("datum", &f.datum)
            .append_pair("beskrivning", &f.beskrivning);
        for (i, rad) in f.rader.iter().enumerate() {
            query
                .append_pair(&format!("konto_{i}"), &rad.konto)
                .append_pair(&format!("debet_{i}"), &rad.debet)
                .append_pair(&format!("kredit_{i}"), &rad.kredit)
                .append_pair(&format!("text_{i}"), &rad.text);
        }
        for u in valda.unwrap_or_default() {
            query.append_pair("underlag", u);
        }
        let sida = if utkast.is_some() { "" } else { "/nytt" };
        Redirect::to(&format!(
            "/foretag/{företag}/verifikat{sida}?{}",
            query.finish()
        ))
        .into_response()
    }

    fn inget_utkast() -> Response {
        (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            "Utkastet finns inte, eller är redan bokfört.\n",
        )
            .into_response()
    }

    fn inget_verifikat() -> Response {
        (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            "Verifikatet finns inte, eller är inte bokfört.\n",
        )
            .into_response()
    }
}

/// Företagets sida i sökvägen, eller `None` om användaren saknar åtkomst.
pub(crate) fn företagets_bas() -> Option<String> {
    let id = use_params_map().with_untracked(|p| p.get("id").unwrap_or_default());
    let val = use_context::<Företagsval>().unwrap_or_default();
    val.0.into_iter().find(|f| f.id == id).map(|f| f.href())
}

pub(crate) fn inloggad() -> bool {
    matches!(use_context::<Läge>(), Some(Läge::Inloggad { .. }))
}

/// Fliken Verifikat: listan och, när ett verifikat är valt, dokumentet.
#[component]
pub fn VerifikatSida() -> impl IntoView {
    sida(hämta_verifikatdata, verifikatsida)
}

fn verifikatsida(data: Verifikatdata) -> AnyView {
    if !inloggad() {
        return view! { <LoggaIn /> }.into_any();
    }
    let Some(bas) = företagets_bas() else {
        return view! { <IngenÅtkomst /> }.into_any();
    };
    let fråga = |namn: &str| use_query_map().with_untracked(|q| q.get(namn).is_some());
    let notis = if fråga("raderat") {
        Some("Utkastet raderades och raderingen loggades. Dess underlag är okopplade.".into())
    } else if fråga("sparat") {
        Some("Utkastet sparades.".to_string())
    } else if fråga("kompletterat") {
        Some("Verifikatet kompletterades. Kompletteringen loggades och kan inte tas bort.".into())
    } else {
        rättad_notis(&data)
    }
    .map(|text| view! { <p class="notis" role="status">{text}</p> });
    // Formulärets fel visas i formuläret; dokumentets här.
    let fel = data
        .dokument
        .as_ref()
        .filter(|d| d.nummer.is_some())
        .and_then(|_| use_query_map().with_untracked(|q| q.get("fel")))
        .and_then(|k| feltext(&k))
        .map(|text| view! { <p class="fel" role="alert">{text}</p> });
    let okopplade = data.okopplade.clone();
    let detalj = match data.dokument.clone() {
        Some(d) if d.nummer.is_none() && fråga("radera") => {
            view! { <RaderaUtkast bas=bas.clone() d=d /> }.into_any()
        }
        Some(d) if d.nummer.is_none() => {
            let formulär = data.formulär.clone().unwrap_or_default();
            view! { <Formulär bas=bas.clone() data=formulär okopplade=okopplade utkast=Some(d) /> }
                .into_any()
        }
        Some(d) => view! { <Dokument bas=bas.clone() d=d okopplade=okopplade /> }.into_any(),
        None => view! {
            <div class="tom">
                <p>"Välj ett verifikat i listan."</p>
                <a class="knapp" href=format!("{bas}/verifikat/nytt")>
                    "Nytt verifikat"
                </a>
            </div>
        }
        .into_any(),
    };
    let detalj = view! {
        {notis}
        {fel}
        {detalj}
    }
    .into_any();
    view! { <Delad bas=bas data=data detalj=detalj /> }.into_any()
}

/// Notisen efter en rättelse (`rattad=V7`), med utkastet från "registrera om" om det öppnades.
fn rättad_notis(data: &Verifikatdata) -> Option<String> {
    let nummer = use_query_map().with_untracked(|q| q.get("rattad"))?;
    let siffror = nummer.strip_prefix('V')?;
    if siffror.is_empty() || !siffror.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let kopia_av = data
        .dokument
        .as_ref()
        .filter(|d| d.nummer.is_none())
        .and_then(|d| d.kopia_av.as_ref());
    Some(match kopia_av {
        Some(original) => format!(
            "Rättelsen bokfördes som {nummer}. Här är ett utkast med {}s rader att rätta och \
             bokföra.",
            original.nummer
        ),
        None => format!("Rättelsen bokfördes som {nummer}."),
    })
}

/// En länk till ett annat verifikat i listan, med full sidladdning (sidans data kommer från
/// middleware).
fn verifikatlänk(bas: &str, l: &LänkVy) -> impl IntoView + use<> {
    view! {
        <a href=format!("{bas}/verifikat?v={}", l.id)>
            {l.nummer.clone()}
        </a>
    }
}

/// Rätta… i dokumentet (variant C): de vända raderna, datum och en valfri orsak, och valen Bokför
/// rättelse och registrera om och Bara bokför rättelse. Spärrarna prövas med
/// `maya_domain::rattelse` här och i `core`.
#[component]
fn Rättelsepanel(bas: String, id: String, r: RättelseVy) -> impl IntoView {
    let fråga = |namn: &str| use_query_map().with_untracked(|q| q.get(namn));
    let datum = RwSignal::new(fråga("datum").unwrap_or_else(|| r.förvalt.clone()));
    let orsak = RwSignal::new(fråga("orsak").unwrap_or_default());
    let original = r.original();
    let förutsättningar = r.förutsättningar();
    let spärrar = {
        let original = original.clone();
        Memo::new(move |_| match &original {
            Some(o) => datum.with(|d| {
                o.rättelse(d, "", &förutsättningar)
                    .err()
                    .unwrap_or_default()
            }),
            None => Vec::new(),
        })
    };
    let nummer = r.nummer.clone();
    let beskrivning = move || orsak.with(|o| rattelse::beskrivning(&nummer, o));
    let vända = original
        .as_ref()
        .map(|o| rattelse::vänd(&o.rader))
        .unwrap_or_default();
    let belopp = Belopp::eller_tomt;
    let rader = vända
        .into_iter()
        .map(|rad| {
            view! {
                <tr>
                    <td>{rad.konto.to_string()}</td>
                    <td>{rad.text.unwrap_or_default()}</td>
                    <td class="belopp">{belopp(rad.debet)}</td>
                    <td class="belopp">{belopp(rad.kredit)}</td>
                </tr>
            }
        })
        .collect_view();
    let fel = move || {
        spärrar.with(|s| {
            s.iter()
                .map(|s| {
                    view! {
                        <p class="fel" role="alert" data-kod=s.kod>
                            {s.text.clone()}
                        </p>
                    }
                })
                .collect_view()
        })
    };
    let spärrat = move || !spärrar.with(Vec::is_empty);
    view! {
        <section class="rattelse" aria-labelledby="rattelse-rubrik">
            <div class="rattelse-rubrik">
                <h3 id="rattelse-rubrik">"Rättelse av " {r.nummer.clone()}</h3>
                <a class="knapp knapp-diskret" href=format!("{bas}/verifikat?v={id}")>
                    "Avbryt"
                </a>
            </div>
            <p class="diskret">
                "Rättelsen bokförs som ett nytt verifikat som vänder alla rader i "
                {r.nummer.clone()} ". " {r.nummer.clone()} " visar sedan att det är rättat."
            </p>
            <table class="rader">
                <thead>
                    <tr>
                        <th>"Konto"</th>
                        <th>"Text"</th>
                        <th class="belopp">"Debet"</th>
                        <th class="belopp">"Kredit"</th>
                    </tr>
                </thead>
                <tbody>{rader}</tbody>
            </table>
            <form method="post" action=format!("{bas}/verifikat/{id}/ratta") class="rattelse-formular">
                <label>
                    "Verifikatdatum"
                    <input
                        name="datum"
                        type="date"
                        min=r.datum.clone()
                        value=datum.get_untracked()
                        on:input=move |ev| datum.set(event_target_value(&ev))
                    />
                </label>
                <label>
                    <span>"Orsak " <span class="diskret">"(valfri)"</span></span>
                    <input
                        name="orsak"
                        type="text"
                        placeholder="t.ex. fel konto"
                        value=orsak.get_untracked()
                        on:input=move |ev| orsak.set(event_target_value(&ev))
                    />
                </label>
                <div class="rattelse-fel">{fel}</div>
                <p class="diskret">"Beskrivning: " {beskrivning}</p>
                <div class="knappar">
                    <button class="knapp knapp-primar" type="submit" name="handling" value="registrera_om" disabled=spärrat>
                        "Bokför rättelse och registrera om"
                    </button>
                    <button class="knapp" type="submit" name="handling" value="bokfor" disabled=spärrat>
                        "Bara bokför rättelse"
                    </button>
                </div>
            </form>
        </section>
    }
}

/// Radera utkast frågar först och visar en sammanfattning av utkastet.
#[component]
fn RaderaUtkast(bas: String, d: DokumentVy) -> impl IntoView {
    let beskrivning = if d.beskrivning.trim().is_empty() {
        "Utkast utan beskrivning".to_string()
    } else {
        d.beskrivning.clone()
    };
    let datum = if d.datum.is_empty() {
        "utan datum".to_string()
    } else {
        d.datum.clone()
    };
    let rader = match d.rader.len() {
        1 => "1 rad".to_string(),
        n => format!("{n} rader"),
    };
    let underlag = (!d.underlag.is_empty()).then(|| {
        let namn = d
            .underlag
            .iter()
            .map(|u| u.filnamn.clone())
            .collect::<Vec<_>>()
            .join(", ");
        view! { <p>{namn} " blir okopplade och finns kvar i arkivet."</p> }
    });
    view! {
        <section class="dialog" role="alertdialog" aria-labelledby="radera-rubrik">
            <h2 id="radera-rubrik">"Radera utkastet?"</h2>
            <p>{format!("{beskrivning} · {} kr · {datum} · {rader}", d.summa)}</p>
            {underlag}
            <p class="diskret">
                "Raderingen loggas med vem, när och en sammanfattning av utkastet."
            </p>
            <form method="post" action=format!("{bas}/verifikat/{}/radera", d.id)>
                <a class="knapp" href=format!("{bas}/verifikat?v={}", d.id)>
                    "Avbryt"
                </a>
                " "
                <button class="knapp knapp-fara" type="submit">
                    <Papperskorg />
                    "Radera utkast"
                </button>
            </form>
        </section>
    }
}

/// Lucides penna (`pencil-line`), för utkast.
#[component]
fn Penna() -> impl IntoView {
    view! {
        <svg
            class="ikon"
            xmlns="http://www.w3.org/2000/svg"
            width="12"
            height="12"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
        >
            <path d="M13 21h8" />
            <path d="m15 5 4 4" />
            <path d="M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z" />
        </svg>
    }
}

/// Lucides papperskorg (`trash-2`).
#[component]
fn Papperskorg() -> impl IntoView {
    view! {
        <svg
            class="ikon"
            xmlns="http://www.w3.org/2000/svg"
            width="14"
            height="14"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
        >
            <path d="M3 6h18" />
            <path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6" />
            <path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2" />
            <line x1="10" x2="10" y1="11" y2="17" />
            <line x1="14" x2="14" y1="11" y2="17" />
        </svg>
    }
}

/// Den neutrala streckade badgen med pennikon som utkast har.
#[component]
pub(crate) fn UtkastBadge() -> impl IntoView {
    view! {
        <span class="badge badge-utkast">
            <Penna />
            "Utkast"
        </span>
    }
}

/// Formuläret för ett nytt verifikat, bredvid listan.
#[component]
pub fn NyttVerifikatSida() -> impl IntoView {
    sida(hämta_verifikatdata, nytt_verifikat)
}

fn nytt_verifikat(data: Verifikatdata) -> AnyView {
    if !inloggad() {
        return view! { <LoggaIn /> }.into_any();
    }
    let Some(bas) = företagets_bas() else {
        return view! { <IngenÅtkomst /> }.into_any();
    };
    let formulär = data.formulär.clone().unwrap_or_default();
    let detalj = view! {
        <Formulär bas=bas.clone() data=formulär okopplade=data.okopplade.clone() utkast=None />
    }
    .into_any();
    view! { <Delad bas=bas data=data detalj=detalj /> }.into_any()
}

/// Lista och detalj sida vid sida.
#[component]
fn Delad(bas: String, data: Verifikatdata, detalj: AnyView) -> impl IntoView {
    let urval = use_query_map().with_untracked(|q| q.get("urval").unwrap_or_default());
    let filter = [
        ("alla", "Alla"),
        ("utkast", "Utkast"),
        ("bokforda", "Bokförda"),
    ]
    .into_iter()
    .map(|(kod, namn)| {
        let aktuellt = (kod == urval || (urval.is_empty() && kod == "alla")).then_some("page");
        view! {
            <a href=format!("{bas}/verifikat?urval={kod}") aria-current=aktuellt>
                {namn}
            </a>
        }
    })
    .collect_view();
    let valt = data.valt.clone();
    let rader = data
        .lista
        .into_iter()
        .map(|v| {
            let aktuellt = (Some(&v.id) == valt.as_ref()).then_some("page");
            let href = format!("{bas}/verifikat?v={}", v.id);
            let nummer = match v.nummer.clone() {
                Some(n) => view! { <span class="diskret">{n}</span> }.into_any(),
                None => view! { <UtkastBadge /> }.into_any(),
            };
            view! {
                <li>
                    <a href=href aria-current=aktuellt>
                        <span class="verifikatlista-topp">
                            <span>
                                {nummer}
                                " "
                                {v.beskrivning}
                            </span>
                            <span class="belopp">{v.summa}</span>
                        </span>
                        <span class="diskret">{v.datum}</span>
                    </a>
                </li>
            }
        })
        .collect_view();
    view! {
        <div class="verifikat-delad">
            <div class="verifikat-spalt">
                <div class="verifikat-spalt-rubrik">
                    <h1>"Verifikat"</h1>
                    <a class="knapp" href=format!("{bas}/verifikat/nytt")>
                        "Nytt"
                    </a>
                </div>
                <nav class="filter" aria-label="Filter">
                    {filter}
                </nav>
                <ul class="verifikatlista">{rader}</ul>
            </div>
            <div class="verifikat-detalj">{detalj}</div>
        </div>
    }
}

/// Ett verifikat som dokument: rubrik, nummer, belopp, metadata, rader och historik.
#[component]
fn Dokument(bas: String, d: DokumentVy, okopplade: Vec<UnderlagVy>) -> impl IntoView {
    let status = if d.nummer.is_some() {
        "Bokfört"
    } else {
        "Utkast"
    };
    let bokförd = d.bokförd.clone().map(|(tid, namn, _)| {
        view! {
            <dt>"Bokfört"</dt>
            <dd>{tid} " av " {namn}</dd>
        }
    });
    let (_, skapad_namn, skapad_kanal) = d.skapad.clone();
    let rättad_badge = d.rättad_av.as_ref().map(|l| {
        view! { <span class="badge badge-rattad">"Rättad av " {l.nummer.clone()}</span> }
    });
    let rättad = d.rättad_av.as_ref().map(|l| {
        let vem = l
            .bokförd
            .clone()
            .map(|(tid, namn, _)| format!(" · {tid} av {namn}"))
            .unwrap_or_default();
        view! {
            <dt>"Rättad"</dt>
            <dd>"av " {verifikatlänk(&bas, l)} <span class="diskret">{vem}</span></dd>
        }
    });
    let rättar = d.rättar.as_ref().map(|l| {
        view! {
            <dt>"Rättar"</dt>
            <dd>{verifikatlänk(&bas, l)}</dd>
        }
    });
    let ersätter = d.kopia_av.as_ref().map(|l| {
        view! {
            <dt>"Ersätter"</dt>
            <dd>{verifikatlänk(&bas, l)} <span class="diskret">" (kopia av dess rader)"</span></dd>
        }
    });
    let inte_rättbart = d
        .inte_rättbart
        .clone()
        .map(|text| view! { <p class="notis">{text}</p> });
    let öppen = use_query_map().with_untracked(|q| q.get("ratta").is_some());
    let rätta = match d.rättelse.clone() {
        Some(r) if öppen => {
            view! { <Rättelsepanel bas=bas.clone() id=d.id.clone() r=r /> }.into_any()
        }
        Some(_) => view! {
            <p>
                <a class="knapp" href=format!("{bas}/verifikat?v={}&ratta=1", d.id)>
                    "Rätta…"
                </a>
            </p>
        }
        .into_any(),
        None => ().into_any(),
    };
    let underlag = view! {
        <DokumentetsUnderlag bas=bas id=d.id.clone() underlag=d.underlag.clone() okopplade=okopplade />
    };
    let (pdf, filnamn) = d.pdf.clone();
    view! {
        <div class="med-underlag">
        {underlag}
        <article class="dokument">
            <Utskrift href=pdf filnamn=filnamn />
            <header>
                <p class="dokument-nummer">{d.nummer.clone().unwrap_or_default()}</p>
                <h2>{d.beskrivning.clone()}</h2>
                <p>
                    <span class="markering">{status}</span>
                    " "
                    {rättad_badge}
                    " "
                    <span class="belopp">{d.summa.clone()} " kr"</span>
                </p>
            </header>
            {inte_rättbart}
            {rätta}
            <dl class="metadata">
                <dt>"Verifikatdatum"</dt>
                <dd>{d.datum.clone()} <span class="diskret">" · period " {d.period.clone()}</span></dd>
                {bokförd}
                <dt>"Skapat"</dt>
                <dd>"av " {skapad_namn} " via " {skapad_kanal}</dd>
                {rättad}
                {rättar}
                {ersätter}
            </dl>
            <table class="rader">
                <thead>
                    <tr>
                        <th>"Konto"</th>
                        <th>"Kontonamn"</th>
                        <th>"Text"</th>
                        <th class="belopp">"Debet"</th>
                        <th class="belopp">"Kredit"</th>
                    </tr>
                </thead>
                <tbody>
                    {d
                        .rader
                        .into_iter()
                        .map(|r| {
                            view! {
                                <tr>
                                    <td>{r.konto}</td>
                                    <td>{r.kontonamn}</td>
                                    <td>{r.text}</td>
                                    <td class="belopp">{r.debet}</td>
                                    <td class="belopp">{r.kredit}</td>
                                </tr>
                            }
                        })
                        .collect_view()}
                </tbody>
                <tfoot>
                    <tr>
                        <td colspan="3">"Summa"</td>
                        <td class="belopp">{d.debet}</td>
                        <td class="belopp">{d.kredit}</td>
                    </tr>
                </tfoot>
            </table>
            <section>
                <h3>"Historik"</h3>
                <ul class="historik">
                    {d
                        .historik
                        .into_iter()
                        .map(|h| {
                            view! {
                                <li>
                                    <span class="diskret">{h.tid}</span>
                                    " "
                                    {h.typ}
                                    " "
                                    <span class="diskret">"· " {h.namn} " via " {h.kanal}</span>
                                </li>
                            }
                        })
                        .collect_view()}
                </ul>
            </section>
        </article>
        </div>
    }
}

/// Skickar formuläret som en fil just valdes i, med knappen `knapp` om den finns (bara i
/// webbläsaren; utan JavaScript trycker användaren på knappen själv).
fn skicka_vid_vald_fil(knapp: Option<NodeRef<leptos::html::Button>>) -> impl Fn(leptos::ev::Event) {
    move |_ev: leptos::ev::Event| {
        #[cfg(feature = "hydrate")]
        {
            use wasm_bindgen::JsCast;
            let Some(fält) = _ev
                .target()
                .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
            else {
                return;
            };
            let Some(formulär) = fält.form() else {
                return;
            };
            let _ = match knapp.and_then(|k| k.get_untracked()) {
                Some(knapp) => formulär.request_submit_with_submitter(Some(&*knapp)),
                None => formulär.request_submit(),
            };
        }
        #[cfg(not(feature = "hydrate"))]
        let _ = knapp;
    }
}

/// Underlaget till vänster om ett bokfört verifikat: förhandsvisning, listan med kompletteringar
/// i streckad ram, och Komplettera med underlag.
#[component]
fn DokumentetsUnderlag(
    bas: String,
    id: String,
    underlag: Vec<UnderlagVy>,
    okopplade: Vec<UnderlagVy>,
) -> impl IntoView {
    let dubblett = use_query_map().with_untracked(|q| q.get("dubblett").is_some());
    let förhandsvisning = underlag
        .first()
        .cloned()
        .map(|u| view! { <Förhandsvisning bas=bas.clone() u=u /> });
    let tomt = underlag
        .is_empty()
        .then(|| view! { <p class="diskret">"Inget underlag."</p> });
    let filer = underlag
        .into_iter()
        .map(|u| {
            let (klass, när) = match &u.komplettering {
                Some((tid, namn)) => (
                    "ufil komplettering",
                    format!("Komplettering {tid} av {namn}"),
                ),
                None => ("ufil", "Vid bokföring".to_string()),
            };
            view! {
                <div class=klass>
                    <Gem />
                    <a href=format!("{bas}/underlag?u={}&visa=alla", u.id)>
                        {u.filnamn}
                    </a>
                    <span class="diskret">{när}</span>
                </div>
            }
        })
        .collect_view();
    let komplettera = format!("{bas}/verifikat/{id}/komplettera");
    let val = okopplade
        .into_iter()
        .map(|u| {
            view! {
                <form class="ufil" method="post" action=komplettera.clone()>
                    <Gem />
                    <span>{u.filnamn}</span>
                    <input type="hidden" name="underlag" value=u.id />
                    <button class="knapp" type="submit">
                        "Koppla"
                    </button>
                </form>
            }
        })
        .collect_view();
    view! {
        <aside class="underlagsspalt" aria-label="Underlag">
            {förhandsvisning}
            {dubblett
                .then(|| {
                    view! {
                        <p class="notis varningsnotis" role="status">
                            "Samma fil finns redan i företaget. Den laddades upp ändå."
                        </p>
                    }
                })}
            <div class="ufiler">{filer} {tomt}</div>
            <details class="komplettera">
                <summary class="knapp knapp-diskret">
                    <Gem />
                    "Komplettera med underlag"
                </summary>
                <p class="diskret">"Kopplingen loggas och kan inte tas bort."</p>
                <div class="ufiler">{val}</div>
                <form
                    class="uppladdning"
                    method="post"
                    action=komplettera.clone()
                    enctype="multipart/form-data"
                >
                    <label class="slapp">
                        <span>"Släpp en fil här eller välj en fil"</span>
                        <input type="file" name="fil" required on:change=skicka_vid_vald_fil(None) />
                    </label>
                    <button class="knapp" type="submit">
                        "Ladda upp och komplettera"
                    </button>
                </form>
            </details>
        </aside>
    }
}

/// Underlaget till vänster om formuläret: förhandsvisning av det första, de kopplade med Koppla
/// bort, uppladdning och okopplade underlag att välja. Fälten hör till formuläret
/// `verifikatformular` genom attributet `form`, så att det användaren skrev följer med. Med
/// JavaScript ändras valet utan att sidan skickas.
#[component]
fn FormuläretsUnderlag(
    bas: String,
    valda: RwSignal<Vec<UnderlagVy>>,
    tillgängliga: Vec<UnderlagVy>,
) -> impl IntoView {
    let fråga = |namn: &str| use_query_map().with_untracked(|q| q.get(namn));
    let dubblett = fråga("dubblett").and_then(|id| {
        valda
            .with_untracked(|v| v.iter().find(|u| u.id == id).cloned())
            .map(|u| {
                let andra = u
                    .dubbletter
                    .iter()
                    .map(|(namn, _)| namn.clone())
                    .collect::<Vec<_>>()
                    .join(", ");
                view! {
                    <p class="notis varningsnotis" role="status">
                        {format!(
                            "Samma fil som {} finns redan i företaget ({andra}). Den laddades upp ändå.",
                            u.filnamn,
                        )}
                    </p>
                }
            })
    });
    let fel = fråga("fel")
        .and_then(|k| feltext(&k))
        .map(|text| view! { <p class="fel" role="alert">{text}</p> });
    let förhandsvisning = {
        let bas = bas.clone();
        move || {
            valda
                .with(|v| v.first().cloned())
                .map(|u| view! { <Förhandsvisning bas=bas.clone() u=u /> })
        }
    };
    let kopplade = move || {
        valda
            .get()
            .into_iter()
            .map(|u| {
                let id = u.id.clone();
                let koppla_bort = move |ev: leptos::ev::MouseEvent| {
                    ev.prevent_default();
                    valda.update(|v| v.retain(|u| u.id != id));
                };
                view! {
                    <div class="ufil">
                        <Gem />
                        <span>{u.filnamn.clone()}</span>
                        <input type="hidden" form="verifikatformular" name="underlag" value=u.id.clone() />
                        <button
                            class="knapp knapp-diskret"
                            type="submit"
                            form="verifikatformular"
                            name="koppla_bort"
                            value=u.id.clone()
                            aria-label=format!("Koppla bort {}", u.filnamn)
                            on:click=koppla_bort
                        >
                            "Koppla bort"
                        </button>
                    </div>
                }
            })
            .collect_view()
    };
    let att_välja = move || {
        tillgängliga
            .iter()
            .filter(|u| valda.with(|v| v.iter().all(|vald| vald.id != u.id)))
            .map(|u| {
                let vald = u.clone();
                let koppla = move |ev: leptos::ev::MouseEvent| {
                    ev.prevent_default();
                    let vald = vald.clone();
                    valda.update(|v| v.push(vald));
                };
                view! {
                    <div class="ufil">
                        <Gem />
                        <span>{u.filnamn.clone()}</span>
                        <button
                            class="knapp"
                            type="submit"
                            form="verifikatformular"
                            name="koppla"
                            value=u.id.clone()
                            on:click=koppla
                        >
                            "Koppla"
                        </button>
                    </div>
                }
            })
            .collect_view()
    };
    let ladda_upp = NodeRef::<leptos::html::Button>::new();
    view! {
        <aside class="underlagsspalt" aria-label="Underlag">
            {förhandsvisning}
            {dubblett}
            {fel}
            <div class="ufiler">{kopplade}</div>
            <label class="slapp">
                <span>"Släpp underlaget här eller välj en fil"</span>
                <span class="diskret">"Högst 25 MB"</span>
                <input
                    type="file"
                    name="fil"
                    form="verifikatformular"
                    on:change=skicka_vid_vald_fil(Some(ladda_upp))
                />
            </label>
            <p>
                <button
                    class="knapp knapp-diskret"
                    type="submit"
                    form="verifikatformular"
                    name="handling"
                    value="ladda_upp"
                    node_ref=ladda_upp
                >
                    "Ladda upp"
                </button>
            </p>
            <details class="valj-okopplat">
                <summary class="knapp knapp-diskret">"Välj ett okopplat underlag"</summary>
                <div class="ufiler">{att_välja}</div>
            </details>
        </aside>
    }
}

/// Formuläret för ett nytt verifikat eller ett utkast. Spärrarna visas vid sina fält medan
/// användaren skriver, och Bokför är inaktiv med antalet problem kvar. Spara utkast går alltid
/// att trycka på.
/// Formuläret för ett nytt verifikat eller ett utkast. `efter = "att_gora"` skickar tillbaka
/// till Att göra efter Spara utkast och Bokför.
#[component]
pub(crate) fn Formulär(
    bas: String,
    data: FormulärData,
    okopplade: Vec<UnderlagVy>,
    utkast: Option<DokumentVy>,
    #[prop(optional)] efter: Option<&'static str>,
) -> impl IntoView {
    // De kopplade och de okopplade kan väljas; ett bortkopplat kan kopplas igen före sparning.
    let mut tillgängliga = data.valda.clone();
    tillgängliga.extend(
        okopplade
            .into_iter()
            .filter(|u| data.valda.iter().all(|v| v.id != u.id)),
    );
    let valda = RwSignal::new(data.valda.clone());
    let från_servern = use_query_map().with_untracked(|q| q.get("datum").is_some());
    let utkastets = utkast
        .as_ref()
        .filter(|_| !från_servern)
        .map(|d| NyttVerifikat {
            datum: d.datum.clone(),
            beskrivning: d.beskrivning.clone(),
            rader: d
                .rader
                .iter()
                .map(|r| NyRad {
                    konto: r.konto.clone(),
                    debet: r.debet.clone(),
                    kredit: r.kredit.clone(),
                    text: r.text.clone(),
                })
                .collect(),
        });
    let ej_sparat = use_query_map().with_untracked(|q| q.get("fel").as_deref() == Some("utkast"));
    let ifyllt = use_query_map().with_untracked(|q| {
        let fält = |namn: &str| q.get(namn).unwrap_or_default();
        let antal = (0..500)
            .take_while(|i| q.get(&format!("konto_{i}")).is_some())
            .count();
        NyttVerifikat {
            datum: fält("datum"),
            beskrivning: fält("beskrivning"),
            rader: (0..antal)
                .map(|i| NyRad {
                    konto: fält(&format!("konto_{i}")),
                    debet: fält(&format!("debet_{i}")),
                    kredit: fält(&format!("kredit_{i}")),
                    text: fält(&format!("text_{i}")),
                })
                .collect(),
        }
    });
    let mut start = utkastets.unwrap_or(ifyllt);
    let antal_rader = start.rader.len().max(4);
    start.rader.resize(antal_rader, NyRad::default());
    let verifikat = RwSignal::new(start);
    let förutsättningar = data.förutsättningar();
    let fristnotis = kontantfristens_notis(data.idag.parse().ok(), verifikat);
    let spärrar = Memo::new(move |_| {
        verifikat.with(|v| v.kontrollera(&förutsättningar).err().unwrap_or_default())
    });
    let hydrerad = RwSignal::new(false);
    Effect::new(move |_| hydrerad.set(true));

    // Bara Bokför stoppas av spärrarna; Spara utkast skickar alltid.
    let bokför = move |ev: leptos::ev::MouseEvent| {
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
                    s.iter().find(|s| s.fält == fält).map(|s: &Spärr| {
                        let aktivera = (s.kod == "konto_inaktivt").then(|| {
                            let Fält::Konto(i) = fält else {
                                unreachable!()
                            };
                            let konto = verifikat.with_untracked(|v| v.rader[i].konto.clone());
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
    let bokför_text = move || match spärrar.with(Vec::len) {
        0 => "Bokför".to_string(),
        1 => "Bokför · 1 problem kvar".to_string(),
        n => format!("Bokför · {n} problem kvar"),
    };
    let lägg_till_rad = move |_| verifikat.update(|v| v.rader.push(NyRad::default()));
    let antal = Memo::new(move |_| verifikat.with(|v| v.rader.len()));
    let värde = move |f: fn(&NyttVerifikat) -> String| verifikat.with_untracked(f);
    let rader = {
        let spärr_vid = spärr_vid.clone();
        move || {
            (0..antal.get())
                .map(|i| {
                    let fält = move |f: fn(&NyRad) -> &String| {
                        verifikat.with_untracked(|v| f(&v.rader[i]).clone())
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
                                    value=fält(|r| &r.konto)
                                    on:input=move |ev| {
                                        verifikat.update(|v| v.rader[i].konto = event_target_value(&ev))
                                    }
                                />
                                {spärr_vid(Fält::Konto(i))}
                            </td>
                            <td>
                                <input
                                    name=format!("text_{i}")
                                    type="text"
                                    aria-label="Radtext"
                                    value=fält(|r| &r.text)
                                    on:input=move |ev| {
                                        verifikat.update(|v| v.rader[i].text = event_target_value(&ev))
                                    }
                                />
                            </td>
                            <td>
                                <input
                                    name=format!("debet_{i}")
                                    type="text"
                                    inputmode="decimal"
                                    class="belopp"
                                    aria-label="Debet"
                                    value=fält(|r| &r.debet)
                                    on:input=move |ev| {
                                        verifikat.update(|v| v.rader[i].debet = event_target_value(&ev))
                                    }
                                />
                                {spärr_vid(Fält::Belopp(i))}
                            </td>
                            <td>
                                <input
                                    name=format!("kredit_{i}")
                                    type="text"
                                    inputmode="decimal"
                                    class="belopp"
                                    aria-label="Kredit"
                                    value=fält(|r| &r.kredit)
                                    on:input=move |ev| {
                                        verifikat.update(|v| v.rader[i].kredit = event_target_value(&ev))
                                    }
                                />
                            </td>
                        </tr>
                    }
                })
                .collect_view()
        }
    };
    let utkast_id = utkast.as_ref().map(|d| d.id.clone());
    let rubrik = match &utkast {
        Some(d) => {
            let namn = if d.beskrivning.trim().is_empty() {
                "Utkast".to_string()
            } else {
                d.beskrivning.clone()
            };
            let kopia_av = d.kopia_av.as_ref().map(|l| {
                view! { <span class="diskret">" · kopia av " {verifikatlänk(&bas, l)}</span> }
            });
            view! {
                <h2>{namn}</h2>
                <p>
                    <UtkastBadge />
                    " "
                    <span class="diskret">"Skapat av " {d.skapad.1.clone()} " · " {d.skapad.0.clone()}</span>
                    {kopia_av}
                </p>
            }
            .into_any()
        }
        None => view! { <h2>"Nytt verifikat"</h2> }.into_any(),
    };
    let radera = utkast_id.clone().map(|id| {
        view! {
            <a class="knapp knapp-fara-diskret" href=format!("{bas}/verifikat?v={id}&radera=1")>
                <Papperskorg />
                "Radera utkast"
            </a>
        }
    });
    let aktiva = data
        .konton
        .iter()
        .filter(|k| k.aktiv)
        .map(|k| view! { <option value=k.nummer.to_string() label=k.namn.clone()></option> })
        .collect_view();

    let underlag =
        view! { <FormuläretsUnderlag bas=bas.clone() valda=valda tillgängliga=tillgängliga /> };
    view! {
        <div class="med-underlag">
        {underlag}
        <form
            id="verifikatformular"
            class="verifikatformular"
            method="post"
            action=format!("{bas}/verifikat")
            enctype="multipart/form-data"
            novalidate
        >
            {rubrik}
            {ej_sparat
                .then(|| {
                    view! {
                        <p class="fel" role="alert">
                            "Utkastet kunde inte sparas. Rätta det som är markerat."
                        </p>
                    }
                })}
            <input type="hidden" name="utkast" value=utkast_id.clone().unwrap_or_default() />
            {efter.map(|e| view! { <input type="hidden" name="efter" value=e /> })}
            {fristnotis}
            <input type="hidden" name="underlag_finns" value="1" />
            <div class="falt-rad">
                <div>
                    <label for="datum">"Verifikatdatum"</label>
                    <input
                        id="datum"
                        name="datum"
                        type="date"
                        value=värde(|v| v.datum.clone())
                        on:input=move |ev| verifikat.update(|v| v.datum = event_target_value(&ev))
                    />
                    {spärr_vid(Fält::Datum)}
                </div>
                <div class="bred">
                    <label for="beskrivning">"Beskrivning"</label>
                    <input
                        id="beskrivning"
                        name="beskrivning"
                        type="text"
                        placeholder="Vad hände?"
                        value=värde(|v| v.beskrivning.clone())
                        on:input=move |ev| {
                            verifikat.update(|v| v.beskrivning = event_target_value(&ev))
                        }
                    />
                    {spärr_vid(Fält::Beskrivning)}
                </div>
            </div>
            <datalist id="konton">{aktiva}</datalist>
            <table class="rader">
                <thead>
                    <tr>
                        <th>"Konto"</th>
                        <th>"Text"</th>
                        <th class="belopp">"Debet"</th>
                        <th class="belopp">"Kredit"</th>
                    </tr>
                </thead>
                <tbody>{rader}</tbody>
            </table>
            {spärr_vid(Fält::Rader)}
            {spärr_vid(Fält::Summa)}
            <p>
                <button class="knapp knapp-diskret" type="button" on:click=lägg_till_rad>
                    "+ Rad"
                </button>
            </p>
            <p class="knappar">
                <button
                    class="knapp knapp-primar"
                    type="submit"
                    name="handling"
                    value="bokfor"
                    on:click=bokför
                    disabled=move || hydrerad.get() && !spärrar.with(Vec::is_empty)
                >
                    {bokför_text}
                </button>
                <button class="knapp" type="submit" name="handling" value="utkast">
                    "Spara utkast"
                </button>
                {radera}
            </p>
        </form>
        </div>
    }
}

/// Notisen om kontantfristen (T1, BFL 5:2) när formuläret har ett datum och en rad på kassan
/// (grupp 191). Fristen varnar men spärrar inte.
fn kontantfristens_notis(
    idag: Option<jiff::civil::Date>,
    verifikat: RwSignal<NyttVerifikat>,
) -> impl Fn() -> Option<AnyView> {
    let frist = Memo::new(move |_| {
        let idag = idag?;
        verifikat.with(|v| {
            let läge = Utkastläge {
                datum: Some(v.datum.trim().parse().ok()?),
                kassa: rör_kassan(v.rader.iter().filter_map(|r| {
                    r.konto
                        .trim()
                        .parse::<u16>()
                        .ok()
                        .and_then(Kontonummer::new)
                })),
            };
            match frist(&läge, idag) {
                Frist::Kontantfrist { senast, försenad } => Some((senast, försenad)),
                _ => None,
            }
        })
    });
    move || {
        frist.get().map(|(senast, försenad)| {
            let text = if försenad {
                format!(
                    "Kontantfristen gick ut {}. Bokför så snart som möjligt.",
                    dag_lång(senast)
                )
            } else {
                format!(
                    "Kontantfristen: verifikatet för pengar in i eller ut ur kassan och ska \
                     bokföras senast {} (BFL 5:2).",
                    dag_lång(senast)
                )
            };
            view! { <p class="notis notis-frist" role="status">{text}</p> }.into_any()
        })
    }
}
