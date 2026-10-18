//! Företagen i webben: formuläret för att lägga upp ett företag, företagsväljaren, toppmenyn och
//! företagets sidor. Formuläret kontrolleras med `maya_domain::foretag` i webbläsaren och av
//! `core` på servern, med samma koder.

use leptos::prelude::*;
use leptos_router::hooks::{use_location, use_query_map};
use maya_domain::foretag::{Bokföringsmetod, Fält, Momsperiod, NyttFöretag};
use maya_domain::orgnr::Organisationsnummer;
use serde::{Deserialize, Serialize};

use crate::app::{LoggaIn, Läge};
use crate::sok::Sökfält;

/// Koden när organisationsnumret redan finns i installationen. Bara servern kan se det.
pub const ORGNR_FINNS: &str = "orgnr_finns";

/// Ett företag som användaren har åtkomst till.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FöretagVy {
    pub id: String,
    pub orgnr: String,
    pub namn: String,
    /// Antalet saker att göra i företaget.
    pub att_göra: usize,
}

impl FöretagVy {
    pub fn href(&self) -> String {
        format!("/foretag/{}", self.id)
    }
}

/// Företagen som den inloggade har åtkomst till, för företagsväljaren. Middleware lägger dem på
/// begäran, och de följer med till webbläsaren vid hydreringen.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Företagsval(pub Vec<FöretagVy>);

pub(crate) fn företagsval() -> Företagsval {
    SharedValue::new(|| {
        #[cfg(feature = "ssr")]
        if let Some(parts) = use_context::<axum::http::request::Parts>()
            && let Some(val) = parts.extensions.get::<Företagsval>()
        {
            return val.clone();
        }
        Företagsval::default()
    })
    .into_inner()
}

#[cfg(feature = "ssr")]
pub(crate) mod server {
    use std::sync::Arc;

    use axum::extract::{Request, State};
    use axum::http::{StatusCode, header};
    use axum::middleware::Next;
    use axum::response::{IntoResponse, Redirect, Response};
    use axum::routing::post;
    use axum::{Extension, Form, Router};
    use maya_core::att_gora;
    use maya_core::foretag::{self, FöretagsFel};
    use maya_core::inloggning::Inloggad;
    use maya_core::{Databas, Klocka};
    use maya_domain::foretag::NyttFöretag;
    use serde::Deserialize;

    use super::{FöretagVy, Företagsval, ORGNR_FINNS};

    #[derive(Clone)]
    pub(crate) struct FöretagsState {
        pub db: Databas,
        pub klocka: Klocka,
    }

    pub(crate) fn routes(state: FöretagsState) -> Router {
        Router::new()
            .route("/foretag", post(lägg_upp))
            .with_state(Arc::new(state))
    }

    pub(crate) fn vy(rad: foretag::Företagsrad) -> FöretagVy {
        FöretagVy {
            id: rad.id.to_string(),
            orgnr: rad.orgnr.to_string(),
            namn: rad.namn,
            att_göra: 0,
        }
    }

    /// Lägger företagen som den inloggade har åtkomst till på begäran. Körs efter
    /// `inloggning::identifiera`.
    pub(crate) async fn lägg_på_företagsval(
        State(state): State<FöretagsState>,
        mut req: Request,
        next: Next,
    ) -> Response {
        if let Some(inloggad) = req.extensions().get::<Inloggad>() {
            let antal = match att_gora::antal(&state.db, &inloggad.aktör).await {
                Ok(antal) => antal,
                Err(e) => return serverfel(e),
            };
            match foretag::mina_företag(&state.db, &inloggad.aktör).await {
                Ok(rader) => {
                    let val = Företagsval(
                        rader
                            .into_iter()
                            .map(|rad| {
                                let n = antal.get(&rad.id).copied().unwrap_or_default();
                                FöretagVy {
                                    att_göra: n,
                                    ..vy(rad)
                                }
                            })
                            .collect(),
                    );
                    req.extensions_mut().insert(val);
                }
                Err(e) => return serverfel(e),
            }
        }
        next.run(req).await
    }

    #[derive(Deserialize)]
    struct Formulär {
        #[serde(default)]
        orgnr: String,
        #[serde(default)]
        namn: String,
        #[serde(default)]
        start: String,
        #[serde(default)]
        slut: String,
        #[serde(default)]
        bokforingsmetod: String,
        #[serde(default)]
        momsperiod: String,
    }

    async fn lägg_upp(
        State(state): State<Arc<FöretagsState>>,
        inloggad: Option<Extension<Inloggad>>,
        Form(f): Form<Formulär>,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let nytt = NyttFöretag {
            orgnr: f.orgnr,
            namn: f.namn,
            första_år_start: f.start,
            första_år_slut: f.slut,
            bokföringsmetod: f.bokforingsmetod,
            momsperiod: f.momsperiod,
        };
        match foretag::lägg_upp(&state.db, &state.klocka, &inloggad.aktör, &nytt).await {
            Ok(id) => Redirect::to(&format!("/foretag/{id}")).into_response(),
            Err(FöretagsFel::Ogiltigt(_)) => tillbaka(&nytt, None),
            Err(FöretagsFel::OrgnrFinns(_)) => tillbaka(&nytt, Some(ORGNR_FINNS)),
            Err(e) => serverfel(e),
        }
    }

    /// Tillbaka till formuläret med det användaren skrev. Sidan kontrollerar det igen med samma
    /// regler och visar felen; `fel` bär bara det som bara servern kan se.
    fn tillbaka(f: &NyttFöretag, fel: Option<&str>) -> Response {
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        query
            .append_pair("orgnr", &f.orgnr)
            .append_pair("namn", &f.namn)
            .append_pair("start", &f.första_år_start)
            .append_pair("slut", &f.första_år_slut)
            .append_pair("bokforingsmetod", &f.bokföringsmetod)
            .append_pair("momsperiod", &f.momsperiod);
        if let Some(fel) = fel {
            query.append_pair("fel", fel);
        }
        Redirect::to(&format!("/foretag/nytt?{}", query.finish())).into_response()
    }

    fn serverfel(e: impl std::fmt::Display) -> Response {
        tracing::error!("företaget: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            "Något gick fel i Maya; försök igen.\n",
        )
            .into_response()
    }
}

fn inloggad() -> bool {
    matches!(use_context::<Läge>(), Some(Läge::Inloggad { .. }))
}

/// Id:t på företaget som sidan visar, ur sökvägen `/foretag/<id>/…`.
fn valt_id(sökväg: &str) -> Option<String> {
    let rest = sökväg.strip_prefix("/foretag/")?;
    let id = rest.split('/').next()?;
    (!id.is_empty() && id.bytes().all(|b| b.is_ascii_digit())).then(|| id.to_string())
}

/// Toppmenyn när en sida i ett företag visas: företagsväljaren, flikarna och kugghjulet.
#[component]
pub fn Toppmeny() -> impl IntoView {
    let location = use_location();
    let val = use_context::<Företagsval>().unwrap_or_default();
    move || {
        let id = valt_id(&location.pathname.get())?;
        let valt = val.0.iter().find(|f| f.id == id).cloned()?;
        let bas = valt.href();
        Some(view! {
            <div class="toppmeny-foretag">
            <Företagsväljare valt=valt.clone() alla=val.0.clone() />
            <nav class="flikar" aria-label="Huvudmeny">
                <a rel="external" href=bas.clone()>
                    "Att göra"
                    <Antal n=valt.att_göra />
                </a>
                <a href=format!("{bas}/verifikat")>"Verifikat"</a>
                <a rel="external" href=format!("{bas}/rapporter")>
                    "Rapporter"
                </a>
            </nav>
            <Sökfält valt=valt.clone() alla=val.0.clone() />
            <a
                class="knapp knapp-diskret kugghjul"
                rel="external"
                href=format!("{bas}/installningar")
                title="Inställningar"
                aria-label="Inställningar"
            >
                <Kugghjul />
            </a>
            </div>
        })
    }
}

#[component]
fn Företagsväljare(valt: FöretagVy, alla: Vec<FöretagVy>) -> impl IntoView {
    view! {
        <details class="foretagsvaljare">
            <summary class="knapp knapp-diskret">{valt.namn.clone()}</summary>
            <ul>
                {alla
                    .into_iter()
                    .map(|f| {
                        let aktuellt = (f.id == valt.id).then_some("page");
                        let href = f.href();
                        view! {
                            <li>
                                <a rel="external" href=href aria-current=aktuellt>
                                    {f.namn}
                                    <span class="diskret">{f.orgnr}</span>
                                    <Antal n=f.att_göra />
                                </a>
                            </li>
                        }
                    })
                    .collect_view()}
                <li>
                    <a href="/foretag/nytt">"Lägg upp företag…"</a>
                </li>
            </ul>
        </details>
    }
}

/// Antalet saker att göra, som en liten räknare; inget när det är noll.
#[component]
pub(crate) fn Antal(n: usize) -> impl IntoView {
    (n > 0).then(|| view! { <span class="antal">{n}</span> })
}

/// Lucides kugghjul (`settings`).
#[component]
fn Kugghjul() -> impl IntoView {
    view! {
        <svg
            xmlns="http://www.w3.org/2000/svg"
            width="16"
            height="16"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
        >
            <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" />
            <circle cx="12" cy="12" r="3" />
        </svg>
    }
}

/// Startsidan för en inloggad: företagen att välja bland, eller vägen till det första.
#[component]
pub fn Företagslista() -> impl IntoView {
    let val = use_context::<Företagsval>().unwrap_or_default();
    if val.0.is_empty() {
        return view! {
            <section class="tom-sida">
                <h1>"Välkommen till Maya"</h1>
                <p>"Lägg upp ett företag för att börja bokföra."</p>
                <p>
                    <a class="knapp" href="/foretag/nytt">
                        "Lägg upp ett företag"
                    </a>
                </p>
            </section>
        }
        .into_any();
    }
    view! {
        <section class="sida">
            <h1>"Välj företag"</h1>
            <ul class="lista">
                {val
                    .0
                    .into_iter()
                    .map(|f| {
                        let href = f.href();
                        view! {
                            <li>
                                <a rel="external" href=href>
                                    {f.namn}
                                </a>
                                " "
                                <span class="diskret">{f.orgnr}</span>
                                " "
                                <Antal n=f.att_göra />
                            </li>
                        }
                    })
                    .collect_view()}
            </ul>
            <p>
                <a href="/foretag/nytt">"Lägg upp ett företag"</a>
            </p>
        </section>
    }
    .into_any()
}

/// Ett fel vid ett fält i formuläret.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Felvisning {
    fält: &'static str,
    kod: &'static str,
    text: String,
}

/// Felen i formuläret, med samma regler som servern. `finns` är serverns besked att numret redan
/// finns i installationen.
fn kontrollera(f: &NyttFöretag, finns: bool) -> Vec<Felvisning> {
    let mut fel: Vec<Felvisning> = match f.kontrollera() {
        Ok(_) => Vec::new(),
        Err(fel) => fel
            .into_iter()
            .map(|f| Felvisning {
                fält: f.fält.namn(),
                kod: f.kod,
                text: f.text,
            })
            .collect(),
    };
    if finns && let Ok(nr) = Organisationsnummer::tolka(&f.orgnr) {
        fel.insert(
            0,
            Felvisning {
                fält: Fält::Orgnr.namn(),
                kod: ORGNR_FINNS,
                text: format!(
                    "{nr} finns redan i Maya. Be någon som har åtkomst till företaget om en inbjudan."
                ),
            },
        );
    }
    fel
}

/// Formuläret för ett nytt företag. Utan JavaScript skickas det till servern, som skickar
/// tillbaka hit med det användaren skrev om något är fel.
#[component]
pub fn NyttFöretagSida() -> impl IntoView {
    if !inloggad() {
        return view! { <LoggaIn /> }.into_any();
    }
    let (ifyllt, finns) = use_query_map().with_untracked(|q| {
        let fält = |namn: &str| q.get(namn).unwrap_or_default();
        let ifyllt = q.get("orgnr").map(|_| NyttFöretag {
            orgnr: fält("orgnr"),
            namn: fält("namn"),
            första_år_start: fält("start"),
            första_år_slut: fält("slut"),
            bokföringsmetod: fält("bokforingsmetod"),
            momsperiod: fält("momsperiod"),
        });
        (ifyllt, q.get("fel").as_deref() == Some(ORGNR_FINNS))
    });
    let fel = RwSignal::new(
        ifyllt
            .as_ref()
            .map(|f| kontrollera(f, finns))
            .unwrap_or_default(),
    );
    let värden = ifyllt.unwrap_or_else(|| NyttFöretag {
        bokföringsmetod: Bokföringsmetod::Faktura.kod().into(),
        momsperiod: Momsperiod::Kvartal.kod().into(),
        ..NyttFöretag::default()
    });

    let orgnr = NodeRef::<leptos::html::Input>::new();
    let namn = NodeRef::<leptos::html::Input>::new();
    let start = NodeRef::<leptos::html::Input>::new();
    let slut = NodeRef::<leptos::html::Input>::new();
    let metod = NodeRef::<leptos::html::Select>::new();
    let moms = NodeRef::<leptos::html::Select>::new();
    let skicka = move |ev: leptos::ev::SubmitEvent| {
        let input =
            |r: NodeRef<leptos::html::Input>| r.get().map(|e| e.value()).unwrap_or_default();
        let select =
            |r: NodeRef<leptos::html::Select>| r.get().map(|e| e.value()).unwrap_or_default();
        let nytt = NyttFöretag {
            orgnr: input(orgnr),
            namn: input(namn),
            första_år_start: input(start),
            första_år_slut: input(slut),
            bokföringsmetod: select(metod),
            momsperiod: select(moms),
        };
        let nya = kontrollera(&nytt, false);
        if !nya.is_empty() {
            ev.prevent_default();
        }
        fel.set(nya);
    };
    let fel_vid = move |fält: &'static str| {
        move || {
            fel.with(|fel| {
                fel.iter().find(|f| f.fält == fält).map(|f| {
                    view! {
                        <p class="fel" role="alert" data-kod=f.kod>
                            {f.text.clone()}
                        </p>
                    }
                })
            })
        }
    };
    let metod_värde = värden.bokföringsmetod.clone();
    let moms_värde = värden.momsperiod.clone();

    view! {
        <section class="sida">
            <h1>"Lägg upp ett företag"</h1>
            <form class="formular" method="post" action="/foretag" novalidate on:submit=skicka>
                <label for="orgnr">"Organisationsnummer"</label>
                <input
                    id="orgnr"
                    name="orgnr"
                    type="text"
                    inputmode="numeric"
                    placeholder="556036-0793"
                    value=värden.orgnr.clone()
                    node_ref=orgnr
                    required
                />
                {fel_vid(Fält::Orgnr.namn())}
                <label for="namn">"Namn"</label>
                <input
                    id="namn"
                    name="namn"
                    type="text"
                    value=värden.namn.clone()
                    node_ref=namn
                    required
                />
                {fel_vid(Fält::Namn.namn())}
                <fieldset>
                    <legend>"Första räkenskapsåret"</legend>
                    <label for="start">"Från"</label>
                    <input
                        id="start"
                        name="start"
                        type="date"
                        value=värden.första_år_start.clone()
                        node_ref=start
                        required
                    />
                    {fel_vid(Fält::FörstaÅrStart.namn())}
                    <label for="slut">"Till och med"</label>
                    <input
                        id="slut"
                        name="slut"
                        type="date"
                        value=värden.första_år_slut.clone()
                        node_ref=slut
                        required
                    />
                    {fel_vid(Fält::FörstaÅrSlut.namn())}
                    <p class="diskret">
                        "Året slutar den sista dagen i en månad och är högst 18 månader."
                    </p>
                </fieldset>
                <label for="bokforingsmetod">"Bokföringsmetod"</label>
                <select id="bokforingsmetod" name="bokforingsmetod" node_ref=metod>
                    {Bokföringsmetod::ALLA
                        .into_iter()
                        .map(|m| {
                            view! {
                                <option value=m.kod() selected=m.kod() == metod_värde>
                                    {m.namn()}
                                </option>
                            }
                        })
                        .collect_view()}
                </select>
                {fel_vid(Fält::Bokföringsmetod.namn())}
                <label for="momsperiod">"Momsperiod"</label>
                <select id="momsperiod" name="momsperiod" node_ref=moms>
                    {Momsperiod::ALLA
                        .into_iter()
                        .map(|m| {
                            view! {
                                <option value=m.kod() selected=m.kod() == moms_värde>
                                    {m.namn()}
                                </option>
                            }
                        })
                        .collect_view()}
                </select>
                {fel_vid(Fält::Momsperiod.namn())}
                <button class="knapp" type="submit">
                    "Lägg upp företaget"
                </button>
            </form>
        </section>
    }
    .into_any()
}

#[component]
pub(crate) fn IngenÅtkomst() -> impl IntoView {
    #[cfg(feature = "ssr")]
    if let Some(response) = use_context::<leptos_axum::ResponseOptions>() {
        response.set_status(axum::http::StatusCode::FORBIDDEN);
    }
    view! {
        <section class="tom-sida">
            <h1>"Du har inte åtkomst till företaget"</h1>
            <p>"Be någon som har åtkomst om en inbjudan, eller välj ett annat företag."</p>
            <p>
                <a href="/">"Till startsidan"</a>
            </p>
        </section>
    }
}
