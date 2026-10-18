//! Sökningen: ⌘K och sökfältet i toppmenyn öppnar samma sökning efter sidor, handlingar,
//! företagsbyte och verifikat i det valda företaget. Sidorna och handlingarna prövas med
//! `maya_domain::sok` där sökningen visas, verifikaten av `core` på servern.
//!
//! Utan JavaScript skickar sökfältet frågan till `/foretag/<id>/sok`, som visar samma sökning
//! som paletten.

use leptos::prelude::*;
use leptos_router::hooks::{use_navigate, use_params_map, use_query_map};
use maya_domain::sok::Fråga;
use serde::{Deserialize, Serialize};

use crate::app::{LoggaIn, Läge};
use crate::foretag::{FöretagVy, Företagsval, IngenÅtkomst};

/// Något som sökningen leder till.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Träff {
    /// Rubriken som träffen visas under, t.ex. "Gå till" eller "Verifikat".
    pub grupp: String,
    pub titel: String,
    /// Datum och belopp för ett verifikat, organisationsnumret för ett företag.
    pub detalj: String,
    pub href: String,
}

/// Handlingarna, sidorna och företagsbytet som frågan träffar, när `valt` är det valda
/// företaget. En tom fråga träffar alla.
pub fn mål(fråga: &str, valt: &FöretagVy, alla: &[FöretagVy]) -> Vec<Träff> {
    let bas = valt.href();
    // Grupp, titel, fler ord som träffar och vart den leder.
    let mut kandidater: Vec<(&str, String, &str, String, String)> = vec![
        (
            "Handlingar",
            "Nytt verifikat".into(),
            "bokför registrera",
            String::new(),
            format!("{bas}/verifikat/nytt"),
        ),
        (
            "Handlingar",
            "Lägg upp företag".into(),
            "nytt",
            String::new(),
            "/foretag/nytt".into(),
        ),
        (
            "Gå till",
            "Att göra".into(),
            "start",
            String::new(),
            bas.clone(),
        ),
        (
            "Gå till",
            "Verifikat".into(),
            "grundbok",
            String::new(),
            format!("{bas}/verifikat"),
        ),
        (
            "Gå till",
            "Utkast".into(),
            "verifikat",
            String::new(),
            format!("{bas}/verifikat?urval=utkast"),
        ),
        (
            "Gå till",
            "Rapporter".into(),
            "balansräkning resultaträkning",
            String::new(),
            format!("{bas}/rapporter"),
        ),
        (
            "Gå till",
            "Inställningar".into(),
            "kontoplan",
            String::new(),
            format!("{bas}/installningar"),
        ),
    ];
    kandidater.extend(
        alla.iter()
            .filter(|f| f.id != valt.id)
            .map(|f| ("Byt företag", f.namn.clone(), "", f.orgnr.clone(), f.href())),
    );
    let fråga = Fråga::tolka(fråga);
    kandidater
        .into_iter()
        .filter(|(grupp, titel, ord, detalj, _)| {
            fråga
                .as_ref()
                .is_none_or(|f| f.träffar_text(&format!("{grupp} {titel} {ord} {detalj}")))
        })
        .map(|(grupp, titel, _, detalj, href)| Träff {
            grupp: grupp.into(),
            titel,
            detalj,
            href,
        })
        .collect()
}

/// Verifikaten som träffar frågan i företaget `foretag`, för paletten.
#[server(prefix = "/api", endpoint = "sok", input = server_fn::codec::GetUrl)]
pub async fn sök_verifikat(foretag: String, q: String) -> Result<Vec<Träff>, ServerFnError> {
    use maya_core::Databas;
    use maya_core::foretag::FöretagsId;
    use maya_core::inloggning::Inloggad;

    let parts = use_context::<axum::http::request::Parts>()
        .ok_or_else(|| ServerFnError::new("begäran saknas"))?;
    let inloggad = parts
        .extensions
        .get::<Inloggad>()
        .ok_or_else(|| ServerFnError::new("inte inloggad"))?;
    let db = use_context::<Databas>().ok_or_else(|| ServerFnError::new("databasen saknas"))?;
    let företag: FöretagsId = foretag
        .parse()
        .map_err(|_| ServerFnError::new("okänt företag"))?;
    server::verifikatträffar(&db, &inloggad.aktör, företag, &q)
        .await
        .map_err(|e| match e {
            maya_core::atkomst::ÅtkomstFel::IngenÅtkomst
            | maya_core::atkomst::ÅtkomstFel::KanalenFårInte => {
                ServerFnError::new("du har inte åtkomst till företaget")
            }
            e => {
                tracing::error!("sökningen: {e}");
                ServerFnError::new("något gick fel i Maya; försök igen")
            }
        })
}

/// Frågan och verifikaten som sidan `/foretag/<id>/sok` visar. Middleware hämtar dem ur `core`
/// och lägger dem på begäran, och de följer med till webbläsaren vid hydreringen.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sökdata {
    pub verifikat: Vec<Träff>,
}

fn sökdata() -> Sökdata {
    SharedValue::new(|| {
        #[cfg(feature = "ssr")]
        if let Some(parts) = use_context::<axum::http::request::Parts>()
            && let Some(data) = parts.extensions.get::<Sökdata>()
        {
            return data.clone();
        }
        Sökdata::default()
    })
    .into_inner()
}

#[cfg(feature = "ssr")]
pub(crate) mod server {
    use axum::extract::{Request, State};
    use axum::http::{StatusCode, header};
    use axum::middleware::Next;
    use axum::response::{IntoResponse, Response};
    use maya_core::Databas;
    use maya_core::aktor::Aktör;
    use maya_core::atkomst::ÅtkomstFel;
    use maya_core::foretag::FöretagsId;
    use maya_core::inloggning::Inloggad;
    use maya_core::verifikat;

    use super::{Sökdata, Träff};

    /// Så många verifikat visar sökningen.
    const HÖGST: usize = 20;

    pub(crate) async fn verifikatträffar(
        db: &Databas,
        aktör: &Aktör,
        företag: FöretagsId,
        fråga: &str,
    ) -> Result<Vec<Träff>, ÅtkomstFel> {
        Ok(verifikat::sök(db, aktör, företag, fråga)
            .await?
            .into_iter()
            .take(HÖGST)
            .map(|v| Träff {
                grupp: "Verifikat".into(),
                titel: format!(
                    "{} · {}",
                    v.nummer
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "Utkast".into()),
                    v.beskrivning
                ),
                detalj: match v.datum {
                    Some(d) => format!("{d} · {}", v.summa),
                    None => v.summa.to_string(),
                },
                href: format!("/foretag/{företag}/verifikat?v={}", v.id),
            })
            .collect())
    }

    /// Hämtar verifikaten som sidan `/foretag/<id>/sok?q=` visar. Körs efter
    /// `inloggning::identifiera`.
    pub(crate) async fn lägg_på_sökdata(
        State(db): State<Databas>,
        mut req: Request,
        next: Next,
    ) -> Response {
        let Some(inloggad) = req.extensions().get::<Inloggad>().cloned() else {
            return next.run(req).await;
        };
        let delar: Vec<&str> = req.uri().path().trim_matches('/').split('/').collect();
        let Some(företag) = (match delar.as_slice() {
            ["foretag", id, "sok"] => id.parse::<FöretagsId>().ok(),
            _ => None,
        }) else {
            return next.run(req).await;
        };
        let fråga = req
            .uri()
            .query()
            .and_then(|q| {
                url::form_urlencoded::parse(q.as_bytes())
                    .find(|(k, _)| k == "q")
                    .map(|(_, v)| v.into_owned())
            })
            .unwrap_or_default();
        match verifikatträffar(&db, &inloggad.aktör, företag, &fråga).await {
            Ok(verifikat) => {
                req.extensions_mut().insert(Sökdata { verifikat });
            }
            // Sidan visar att användaren saknar åtkomst, ur företagsvalet.
            Err(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte) => {}
            Err(e) => {
                tracing::error!("sökningen: {e}");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
                    "Något gick fel i Maya; försök igen.\n",
                )
                    .into_response();
            }
        }
        next.run(req).await
    }
}

/// Sökfältet i toppmenyn. Med JavaScript öppnar det paletten, liksom ⌘K (Ctrl+K) var som helst
/// i företaget; utan skickar det frågan till sidan `/foretag/<id>/sok`.
#[component]
pub fn Sökfält(valt: FöretagVy, alla: Vec<FöretagVy>) -> impl IntoView {
    let öppen = RwSignal::new(false);
    #[cfg(feature = "hydrate")]
    {
        let lyssnare = window_event_listener(leptos::ev::keydown, move |ev| {
            if (ev.meta_key() || ev.ctrl_key()) && ev.key().eq_ignore_ascii_case("k") {
                ev.prevent_default();
                öppen.update(|ö| *ö = !*ö);
            }
        });
        on_cleanup(move || lyssnare.remove());
    }
    let action = format!("{}/sok", valt.href());
    let palett = move || {
        öppen.get().then(|| {
            let stäng = Callback::new(move |()| öppen.set(false));
            view! {
                <div
                    class="palett-bakgrund"
                    on:click=move |ev| {
                        if ev.target() == ev.current_target() {
                            öppen.set(false);
                        }
                    }
                >
                    <div class="palett" role="dialog" aria-modal="true" aria-label="Sök">
                        <Sökning
                            valt=valt.clone()
                            alla=alla.clone()
                            fråga=String::new()
                            verifikat=Vec::new()
                            stäng=stäng
                        />
                    </div>
                </div>
            }
        })
    };
    view! {
        <form
            class="sokfalt"
            action=action
            role="search"
            on:submit=move |ev| {
                ev.prevent_default();
                öppen.set(true);
            }
        >
            <input
                name="q"
                type="search"
                placeholder="Sök eller gå till…"
                aria-label="Sök"
                autocomplete="off"
                on:focus=move |_| öppen.set(true)
            />
            <kbd>"⌘K"</kbd>
        </form>
        {palett}
    }
}

/// Sökningen: fältet och träffarna. Paletten och sidan `/foretag/<id>/sok` visar samma.
#[component]
fn Sökning(
    valt: FöretagVy,
    alla: Vec<FöretagVy>,
    fråga: String,
    verifikat: Vec<Träff>,
    #[prop(optional)] stäng: Option<Callback<()>>,
) -> impl IntoView {
    let action = format!("{}/sok", valt.href());
    let företag = valt.id.clone();
    let start = fråga.clone();
    let fråga = RwSignal::new(fråga);
    let verifikat = RwSignal::new(verifikat);
    let vald = RwSignal::new(0usize);
    let träffar = Memo::new(move |_| {
        let mut t = mål(&fråga.get(), &valt, &alla);
        t.extend(verifikat.get());
        t
    });
    let fält = NodeRef::<leptos::html::Input>::new();
    if stäng.is_some() {
        Effect::new(move |_| {
            if let Some(f) = fält.get() {
                let _ = f.focus();
            }
        });
    }
    let navigate = use_navigate();
    let skriv = move |ev: leptos::ev::Event| {
        let ny = event_target_value(&ev);
        fråga.set(ny.clone());
        vald.set(0);
        let företag = företag.clone();
        leptos::task::spawn_local(async move {
            let nya = if ny.trim().is_empty() {
                Ok(Vec::new())
            } else {
                sök_verifikat(företag, ny.clone()).await
            };
            if let Ok(nya) = nya
                && fråga.get_untracked() == ny
            {
                verifikat.set(nya);
            }
        });
    };
    let tangent = move |ev: leptos::ev::KeyboardEvent| {
        let antal = träffar.with_untracked(Vec::len);
        match ev.key().as_str() {
            "ArrowDown" => {
                ev.prevent_default();
                vald.update(|v| *v = (*v + 1).min(antal.saturating_sub(1)));
            }
            "ArrowUp" => {
                ev.prevent_default();
                vald.update(|v| *v = v.saturating_sub(1));
            }
            "Enter" => {
                if let Some(t) = träffar.with_untracked(|t| t.get(vald.get_untracked()).cloned()) {
                    ev.prevent_default();
                    if let Some(stäng) = stäng {
                        stäng.run(());
                    }
                    navigate(&t.href, Default::default());
                }
            }
            "Escape" => {
                if let Some(stäng) = stäng {
                    ev.prevent_default();
                    stäng.run(());
                }
            }
            _ => {}
        }
    };
    let lista = move || {
        let träffar = träffar.get();
        if träffar.is_empty() {
            return view! { <p class="sok-tom">"Inga träffar."</p> }.into_any();
        }
        let mut grupp = String::new();
        let rader = träffar
            .into_iter()
            .enumerate()
            .map(|(i, t)| {
                let rubrik = (t.grupp != grupp).then(|| {
                    grupp = t.grupp.clone();
                    view! { <li class="sok-grupp">{t.grupp.clone()}</li> }
                });
                let detalj = (!t.detalj.is_empty())
                    .then(|| view! { <span class="diskret">{t.detalj.clone()}</span> });
                view! {
                    {rubrik}
                    <li>
                        <a
                            href=t.href
                            class:vald=move || vald.get() == i
                            on:click=move |_| {
                                if let Some(stäng) = stäng {
                                    stäng.run(());
                                }
                            }
                        >
                            <span>{t.titel}</span>
                            {detalj}
                        </a>
                    </li>
                }
            })
            .collect_view();
        view! { <ul class="sok-traffar">{rader}</ul> }.into_any()
    };
    view! {
        <div class="sokning">
            <form action=action role="search">
                <input
                    name="q"
                    type="search"
                    placeholder="Sök verifikat, belopp, datum, sida eller företag…"
                    aria-label="Sök"
                    autocomplete="off"
                    value=start
                    prop:value=fråga
                    node_ref=fält
                    on:input=skriv
                    on:keydown=tangent
                />
            </form>
            {lista}
            <p class="sok-hjalp diskret">
                <kbd>"↑"</kbd> <kbd>"↓"</kbd> " välj · " <kbd>"↵"</kbd> " öppna · " <kbd>"Esc"</kbd>
                " stäng"
            </p>
        </div>
    }
}

/// Sidan `/foretag/<id>/sok`, dit sökfältet skickar frågan utan JavaScript.
#[component]
pub fn SökSida() -> impl IntoView {
    if !matches!(use_context::<Läge>(), Some(Läge::Inloggad { .. })) {
        return view! { <LoggaIn /> }.into_any();
    }
    let id = use_params_map().with_untracked(|p| p.get("id").unwrap_or_default());
    let alla = use_context::<Företagsval>().unwrap_or_default().0;
    let Some(valt) = alla.iter().find(|f| f.id == id).cloned() else {
        return view! { <IngenÅtkomst /> }.into_any();
    };
    let fråga = use_query_map().with_untracked(|q| q.get("q").unwrap_or_default());
    let data = sökdata();
    view! {
        <section class="sida">
            <h1>"Sök"</h1>
            <Sökning valt=valt alla=alla fråga=fråga verifikat=data.verifikat />
        </section>
    }
    .into_any()
}
