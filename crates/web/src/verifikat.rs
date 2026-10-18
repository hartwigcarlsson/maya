//! Fliken Verifikat: lista och detalj sida vid sida, formuläret för ett nytt verifikat och det
//! bokförda verifikatet som ett dokument (variant C i `prototype/32-verifikat`).
//!
//! Formuläret prövar spärrarna med `maya_domain::verifikat` i webbläsaren, och `core` prövar
//! samma spärrar när det bokför. Utan JavaScript skickas formuläret till servern, som skickar
//! tillbaka hit med det användaren skrev; sidan prövar det igen och visar samma koder.

use jiff::civil::Date;
use leptos::prelude::*;
use leptos_router::hooks::{use_params_map, use_query_map};
use maya_domain::kontoplan::Kontonummer;
use maya_domain::rakenskapsar::Räkenskapsår;
use maya_domain::verifikat::{Fält, Förutsättningar, NyRad, NyttVerifikat, Spärr};
use serde::{Deserialize, Serialize};

use crate::app::{LoggaIn, Läge};
use crate::foretag::{Företagsval, IngenÅtkomst};

/// Ett verifikat i listan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListVy {
    pub id: String,
    pub nummer: Option<String>,
    pub datum: String,
    pub beskrivning: String,
    pub summa: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

/// Ett verifikat som dokument.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DokumentVy {
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

/// Det som fliken Verifikat visar. Middleware hämtar det ur `core` och lägger det på begäran,
/// och det följer med till webbläsaren vid hydreringen.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verifikatdata {
    pub lista: Vec<ListVy>,
    pub valt: Option<String>,
    pub dokument: Option<DokumentVy>,
    pub formulär: Option<FormulärData>,
}

fn verifikatdata() -> Verifikatdata {
    SharedValue::new(|| {
        #[cfg(feature = "ssr")]
        if let Some(parts) = use_context::<axum::http::request::Parts>()
            && let Some(data) = parts.extensions.get::<Verifikatdata>()
        {
            return data.clone();
        }
        Verifikatdata::default()
    })
    .into_inner()
}

/// Månaden som perioden heter efter, t.ex. "mars 2026".
pub fn periodnamn(datum: Date) -> String {
    const MÅNADER: [&str; 12] = [
        "januari",
        "februari",
        "mars",
        "april",
        "maj",
        "juni",
        "juli",
        "augusti",
        "september",
        "oktober",
        "november",
        "december",
    ];
    format!(
        "{} {}",
        MÅNADER[usize::from(datum.month().unsigned_abs()) - 1],
        datum.year()
    )
}

#[cfg(feature = "ssr")]
pub(crate) mod server {
    use std::sync::Arc;

    use axum::extract::{Path, Request, State};
    use axum::http::{StatusCode, header};
    use axum::middleware::Next;
    use axum::response::{IntoResponse, Redirect, Response};
    use axum::routing::post;
    use axum::{Extension, Form, Router};
    use maya_core::atkomst::ÅtkomstFel;
    use maya_core::foretag::FöretagsId;
    use maya_core::inloggning::Inloggad;
    use maya_core::verifikat::{self, Signatur, Urval, VerifikatFel, VerifikatId};
    use maya_core::{Databas, Klocka, kontoplan};
    use maya_domain::belopp::Belopp;
    use maya_domain::verifikat::{NyRad, NyttVerifikat};

    use super::{
        DokumentVy, FormulärData, HistorikVy, KontoVy, ListVy, RadVy, Verifikatdata, periodnamn,
    };

    #[derive(Clone)]
    pub(crate) struct VerifikatState {
        pub db: Databas,
        pub klocka: Klocka,
    }

    pub(crate) fn routes(state: VerifikatState) -> Router {
        Router::new()
            .route("/foretag/{id}/verifikat", post(bokför))
            .with_state(Arc::new(state))
    }

    /// Hämtar det som fliken Verifikat visar, för `/foretag/<id>/verifikat` och
    /// `/foretag/<id>/verifikat/nytt`. Körs efter `inloggning::identifiera`.
    pub(crate) async fn lägg_på_verifikatdata(
        State(state): State<VerifikatState>,
        mut req: Request,
        next: Next,
    ) -> Response {
        let Some(inloggad) = req.extensions().get::<Inloggad>().cloned() else {
            return next.run(req).await;
        };
        let delar: Vec<&str> = req.uri().path().trim_matches('/').split('/').collect();
        let (företag, nytt) = match delar.as_slice() {
            ["foretag", id, "verifikat"] => (id.parse::<FöretagsId>().ok(), false),
            ["foretag", id, "verifikat", "nytt"] => (id.parse::<FöretagsId>().ok(), true),
            _ => (None, false),
        };
        let Some(företag) = företag else {
            return next.run(req).await;
        };
        let query: Vec<(String, String)> = req
            .uri()
            .query()
            .map(|q| {
                url::form_urlencoded::parse(q.as_bytes())
                    .into_owned()
                    .collect()
            })
            .unwrap_or_default();
        let fråga = |namn: &str| {
            query
                .iter()
                .find(|(k, _)| k == namn)
                .map(|(_, v)| v.clone())
        };
        let urval = fråga("urval")
            .and_then(|u| Urval::från_kod(&u))
            .unwrap_or_default();
        let valt = if nytt { None } else { fråga("v") };
        match hämta(&state.db, &inloggad, företag, urval, valt, nytt).await {
            Ok(data) => {
                req.extensions_mut().insert(data);
            }
            // Sidan visar att användaren saknar åtkomst, ur företagsvalet.
            Err(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte) => {}
            Err(e) => return serverfel(e),
        }
        next.run(req).await
    }

    async fn hämta(
        db: &Databas,
        inloggad: &Inloggad,
        företag: FöretagsId,
        urval: Urval,
        valt: Option<String>,
        nytt: bool,
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
            dokument = Some(DokumentVy {
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
                        konto: r.konto.to_string(),
                        kontonamn: kontonamn(r.konto.get()),
                        debet: belopp(r.debet),
                        kredit: belopp(r.kredit),
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
            });
        }
        let formulär = if nytt {
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
            })
        } else {
            None
        };
        Ok(Verifikatdata {
            lista,
            valt: dokument.as_ref().and(valt),
            dokument,
            formulär,
        })
    }

    fn belopp(b: Belopp) -> String {
        if b == Belopp::NOLL {
            String::new()
        } else {
            b.to_string()
        }
    }

    fn tid(t: jiff::Timestamp) -> String {
        t.to_zoned(maya_domain::tid::stockholm())
            .strftime("%Y-%m-%d %H:%M")
            .to_string()
    }

    fn signatur(s: &Signatur) -> (String, String, String) {
        (tid(s.tid), s.namn.clone(), s.aktör.kanal.namn().to_string())
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

    async fn bokför(
        State(state): State<Arc<VerifikatState>>,
        Path(id): Path<String>,
        inloggad: Option<Extension<Inloggad>>,
        Form(fält): Form<Vec<(String, String)>>,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let Ok(företag) = id.parse::<FöretagsId>() else {
            return ingen_åtkomst();
        };
        let formulär = tolka(&fält);
        match verifikat::bokför(
            &state.db,
            &state.klocka,
            &inloggad.aktör,
            företag,
            &formulär,
        )
        .await
        {
            Ok(b) => {
                Redirect::to(&format!("/foretag/{företag}/verifikat?v={}", b.id)).into_response()
            }
            Err(VerifikatFel::Spärrat(_)) => tillbaka(företag, &formulär),
            Err(VerifikatFel::Åtkomst(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte)) => {
                ingen_åtkomst()
            }
            Err(e) => serverfel(e),
        }
    }

    /// Tillbaka till formuläret med det användaren skrev. Sidan prövar det igen med samma
    /// spärrar och visar dem vid fälten.
    fn tillbaka(företag: FöretagsId, f: &NyttVerifikat) -> Response {
        let mut query = url::form_urlencoded::Serializer::new(String::new());
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
        Redirect::to(&format!(
            "/foretag/{företag}/verifikat/nytt?{}",
            query.finish()
        ))
        .into_response()
    }

    fn ingen_åtkomst() -> Response {
        (
            StatusCode::FORBIDDEN,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            "Du har inte åtkomst till företaget.\n",
        )
            .into_response()
    }

    fn serverfel(e: impl std::fmt::Display) -> Response {
        tracing::error!("verifikat: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            "Något gick fel i Maya; försök igen.\n",
        )
            .into_response()
    }
}

/// Företagets sida i sökvägen, eller `None` om användaren saknar åtkomst.
fn företagets_bas() -> Option<String> {
    let id = use_params_map().with_untracked(|p| p.get("id").unwrap_or_default());
    let val = use_context::<Företagsval>().unwrap_or_default();
    val.0.into_iter().find(|f| f.id == id).map(|f| f.href())
}

fn inloggad() -> bool {
    matches!(use_context::<Läge>(), Some(Läge::Inloggad { .. }))
}

/// Fliken Verifikat: listan och, när ett verifikat är valt, dokumentet.
#[component]
pub fn VerifikatSida() -> impl IntoView {
    if !inloggad() {
        return view! { <LoggaIn /> }.into_any();
    }
    let Some(bas) = företagets_bas() else {
        return view! { <IngenÅtkomst /> }.into_any();
    };
    let data = verifikatdata();
    let detalj = match data.dokument.clone() {
        Some(d) => view! { <Dokument d=d /> }.into_any(),
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
    view! { <Delad bas=bas data=data detalj=detalj /> }.into_any()
}

/// Formuläret för ett nytt verifikat, bredvid listan.
#[component]
pub fn NyttVerifikatSida() -> impl IntoView {
    if !inloggad() {
        return view! { <LoggaIn /> }.into_any();
    }
    let Some(bas) = företagets_bas() else {
        return view! { <IngenÅtkomst /> }.into_any();
    };
    let data = verifikatdata();
    let formulär = data.formulär.clone().unwrap_or_default();
    let detalj = view! { <Formulär bas=bas.clone() data=formulär /> }.into_any();
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
            let nummer = v.nummer.clone().unwrap_or_else(|| "Utkast".to_string());
            view! {
                <li>
                    <a href=href aria-current=aktuellt>
                        <span class="verifikatlista-topp">
                            <span>
                                <span class="diskret">{nummer}</span>
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
fn Dokument(d: DokumentVy) -> impl IntoView {
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
    view! {
        <article class="dokument">
            <header>
                <p class="dokument-nummer">{d.nummer.clone().unwrap_or_default()}</p>
                <h2>{d.beskrivning.clone()}</h2>
                <p>
                    <span class="markering">{status}</span>
                    " "
                    <span class="belopp">{d.summa.clone()} " kr"</span>
                </p>
            </header>
            <dl class="metadata">
                <dt>"Verifikatdatum"</dt>
                <dd>{d.datum.clone()} <span class="diskret">" · period " {d.period.clone()}</span></dd>
                {bokförd}
                <dt>"Skapat"</dt>
                <dd>"av " {skapad_namn} " via " {skapad_kanal}</dd>
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
    }
}

/// Formuläret för ett verifikat. Spärrarna visas vid sina fält medan användaren skriver, och
/// Bokför är inaktiv med antalet problem kvar.
#[component]
fn Formulär(bas: String, data: FormulärData) -> impl IntoView {
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
    let mut start = ifyllt;
    let antal_rader = start.rader.len().max(4);
    start.rader.resize(antal_rader, NyRad::default());
    let verifikat = RwSignal::new(start);
    let förutsättningar = data.förutsättningar();
    let spärrar = Memo::new(move |_| {
        verifikat.with(|v| v.kontrollera(&förutsättningar).err().unwrap_or_default())
    });
    let hydrerad = RwSignal::new(false);
    Effect::new(move |_| hydrerad.set(true));

    let skicka = move |ev: leptos::ev::SubmitEvent| {
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
    let aktiva = data
        .konton
        .iter()
        .filter(|k| k.aktiv)
        .map(|k| view! { <option value=k.nummer.to_string() label=k.namn.clone()></option> })
        .collect_view();

    view! {
        <form
            class="verifikatformular"
            method="post"
            action=format!("{bas}/verifikat")
            novalidate
            on:submit=skicka
        >
            <h2>"Nytt verifikat"</h2>
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
            <p>
                <button
                    class="knapp knapp-primar"
                    type="submit"
                    disabled=move || hydrerad.get() && !spärrar.with(Vec::is_empty)
                >
                    {bokför_text}
                </button>
            </p>
        </form>
    }
}
