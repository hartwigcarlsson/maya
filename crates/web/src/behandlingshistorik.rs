//! Inställningar → Behandlingshistorik: företagets poster med tid, aktör, typ, objekt och
//! ändringen, med urval på typ, aktör och tid, och export av samma urval som CSV och PDF (A2).
//!
//! Urvalet står i adressen (`typ`, `aktor`, `fran`, `till`), så filterformuläret är ett vanligt
//! GET-formulär och exportlänkarna får samma query. Typerna och aktörerna att välja kommer ur
//! historiken själv, så att typer som tillkommer i senare versioner går att välja utan ändring
//! här.
//!
//! Datan kommer från middleware vid serverrenderingen, så länkarna hit har `rel="external"`.

use leptos::prelude::*;
use leptos_router::hooks::use_params_map;
use serde::{Deserialize, Serialize};

use crate::app::{LoggaIn, Läge};
use crate::foretag::{Företagsval, IngenÅtkomst};
use crate::installningar::Inställningsmeny;
use crate::utskrift::Utskrift;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostVy {
    /// I Stockholm, till sekunden.
    pub tid: String,
    pub aktör: String,
    pub kanal: String,
    pub maya: bool,
    pub typ: String,
    pub objekt: String,
    pub text: String,
}

/// Urvalet som det står i adressen; tomt betyder alla.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UrvalVy {
    pub typ: String,
    pub aktor: String,
    pub fran: String,
    pub till: String,
}

/// Det som sidan visar. Middleware hämtar det ur `core` och lägger det på begäran, och det
/// följer med till webbläsaren vid hydreringen.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Behandlingshistorikdata {
    pub poster: Vec<PostVy>,
    /// Typerna i företagets historik.
    pub typer: Vec<String>,
    /// Aktörerna i företagets historik: nyckeln i adressen och namnet.
    pub aktörer: Vec<(String, String)>,
    pub urval: UrvalVy,
    /// Urvalet som query för exportlänkarna, utan `?`; tomt för alla poster.
    pub query: String,
    /// Exportfilernas namn utan ändelse, t.ex. `behandlingshistorik_5560360793_20261018`.
    pub filnamn: String,
}

fn data() -> Behandlingshistorikdata {
    SharedValue::new(|| {
        #[cfg(feature = "ssr")]
        if let Some(parts) = use_context::<axum::http::request::Parts>()
            && let Some(data) = parts.extensions.get::<Behandlingshistorikdata>()
        {
            return data.clone();
        }
        Behandlingshistorikdata::default()
    })
    .into_inner()
}

#[cfg(feature = "ssr")]
pub(crate) mod server {
    use axum::extract::{Request, State};
    use axum::http::{StatusCode, header};
    use axum::middleware::Next;
    use axum::response::{IntoResponse, Response};
    use jiff::civil::Date;
    use maya_core::atkomst::ÅtkomstFel;
    use maya_core::behandlingshistorik::{self, Aktörsval, Urval};
    use maya_core::foretag::FöretagsId;
    use maya_core::inloggning::Inloggad;
    use maya_core::{Databas, Klocka};

    use super::{Behandlingshistorikdata, PostVy, UrvalVy};

    #[derive(Clone)]
    pub(crate) struct BehandlingshistorikState {
        pub db: Databas,
        pub klocka: Klocka,
    }

    /// Urvalet ur en query. Värden som inte går att tolka hoppas över.
    pub(crate) fn urval(query: Option<&str>) -> Urval {
        let mut urval = Urval::default();
        let par = url::form_urlencoded::parse(query.unwrap_or_default().as_bytes());
        for (k, v) in par {
            let v = v.trim();
            if v.is_empty() {
                continue;
            }
            match &*k {
                "typ" => urval.typ = Some(v.to_string()),
                "aktor" => urval.aktör = v.parse::<Aktörsval>().ok(),
                "fran" => urval.från = v.parse::<Date>().ok(),
                "till" => urval.till = v.parse::<Date>().ok(),
                _ => {}
            }
        }
        urval
    }

    fn query(urval: &Urval) -> String {
        let mut q = url::form_urlencoded::Serializer::new(String::new());
        if let Some(typ) = &urval.typ {
            q.append_pair("typ", typ);
        }
        if let Some(aktör) = urval.aktör {
            q.append_pair("aktor", &aktör.to_string());
        }
        if let Some(från) = urval.från {
            q.append_pair("fran", &från.to_string());
        }
        if let Some(till) = urval.till {
            q.append_pair("till", &till.to_string());
        }
        q.finish()
    }

    /// Hämtar det som sidan visar, för `/foretag/<id>/installningar/behandlingshistorik`.
    /// Körs efter `inloggning::identifiera`.
    pub(crate) async fn lägg_på_behandlingshistorik(
        State(state): State<BehandlingshistorikState>,
        mut req: Request,
        next: Next,
    ) -> Response {
        let Some(inloggad) = req.extensions().get::<Inloggad>().cloned() else {
            return next.run(req).await;
        };
        let delar: Vec<&str> = req.uri().path().trim_matches('/').split('/').collect();
        let ["foretag", id, "installningar", "behandlingshistorik"] = delar.as_slice() else {
            return next.run(req).await;
        };
        let Ok(företag) = id.parse::<FöretagsId>() else {
            return next.run(req).await;
        };
        let urval = urval(req.uri().query());
        match hämta(&state, &inloggad, företag, &urval).await {
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
        state: &BehandlingshistorikState,
        inloggad: &Inloggad,
        företag: FöretagsId,
        urval: &Urval,
    ) -> Result<Behandlingshistorikdata, ÅtkomstFel> {
        let historik =
            behandlingshistorik::historik(&state.db, &inloggad.aktör, företag, urval).await?;
        let f = maya_core::foretag::företaget(&state.db, &inloggad.aktör, företag).await?;
        let stockholm = maya_domain::tid::stockholm();
        Ok(Behandlingshistorikdata {
            poster: historik
                .poster
                .iter()
                .map(|p| PostVy {
                    tid: p
                        .tid
                        .to_zoned(stockholm.clone())
                        .strftime("%Y-%m-%d %H:%M:%S")
                        .to_string(),
                    aktör: p.aktörsnamn(),
                    kanal: p.aktör.kanal.namn().to_string(),
                    maya: p.aktör.kanal == maya_core::aktor::Kanal::Maya,
                    typ: p.typ.clone(),
                    objekt: p.objekt.clone(),
                    text: p.text(),
                })
                .collect(),
            typer: historik.typer,
            aktörer: historik
                .aktörer
                .iter()
                .map(|(a, namn)| (a.to_string(), namn.clone()))
                .collect(),
            urval: UrvalVy {
                typ: urval.typ.clone().unwrap_or_default(),
                aktor: urval.aktör.map(|a| a.to_string()).unwrap_or_default(),
                fran: urval.från.map(|d| d.to_string()).unwrap_or_default(),
                till: urval.till.map(|d| d.to_string()).unwrap_or_default(),
            },
            query: query(urval),
            filnamn: format!(
                "behandlingshistorik_{}_{}",
                f.orgnr.siffror(),
                state.klocka.idag().strftime("%Y%m%d")
            ),
        })
    }

    fn serverfel(e: impl std::fmt::Display) -> Response {
        tracing::error!("behandlingshistorik: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            "Något gick fel i Maya; försök igen.\n",
        )
            .into_response()
    }
}

/// Inställningar → Behandlingshistorik.
#[component]
pub fn BehandlingshistorikSida() -> impl IntoView {
    if !matches!(use_context::<Läge>(), Some(Läge::Inloggad { .. })) {
        return view! { <LoggaIn /> }.into_any();
    }
    let id = use_params_map().with_untracked(|p| p.get("id").unwrap_or_default());
    let val = use_context::<Företagsval>().unwrap_or_default();
    let Some(företaget) = val.0.into_iter().find(|f| f.id == id) else {
        return view! { <IngenÅtkomst /> }.into_any();
    };
    let bas = företaget.href();
    let data = data();
    let sida = format!("{bas}/installningar/behandlingshistorik");
    let med_query = |sökväg: String| {
        if data.query.is_empty() {
            sökväg
        } else {
            format!("{sökväg}?{}", data.query)
        }
    };
    let pdf = med_query(format!("{sida}/pdf"));
    let csv = med_query(format!("{sida}/csv"));
    let antal = match data.poster.len() {
        1 => "1 post".to_string(),
        n => format!("{n} poster"),
    };
    let u = data.urval.clone();
    let typer = data
        .typer
        .iter()
        .map(|t| {
            let vald = *t == u.typ;
            view! {
                <option value=t.clone() selected=vald>
                    {t.clone()}
                </option>
            }
        })
        .collect_view();
    let aktörer = data
        .aktörer
        .iter()
        .map(|(nyckel, namn)| {
            let vald = *nyckel == u.aktor;
            view! {
                <option value=nyckel.clone() selected=vald>
                    {namn.clone()}
                </option>
            }
        })
        .collect_view();
    let tabell = if data.poster.is_empty() {
        view! { <p class="diskret">"Inga poster i urvalet."</p> }.into_any()
    } else {
        let rader = data
            .poster
            .iter()
            .rev()
            .map(|p| {
                let aktör = if p.maya {
                    p.aktör.clone()
                } else {
                    format!("{} ({})", p.aktör, p.kanal)
                };
                view! {
                    <tr class=if p.maya { "maya" } else { "" }>
                        <td class="tid">{p.tid.clone()}</td>
                        <td>{aktör}</td>
                        <td class="typ">{p.typ.clone()}</td>
                        <td>{p.objekt.clone()}</td>
                        <td>{p.text.clone()}</td>
                    </tr>
                }
            })
            .collect_view();
        view! {
            <table class="rader behandlingshistorik">
                <thead>
                    <tr>
                        <th>"Tid"</th>
                        <th>"Aktör"</th>
                        <th>"Typ"</th>
                        <th>"Objekt"</th>
                        <th>"Ändring"</th>
                    </tr>
                </thead>
                <tbody>{rader}</tbody>
            </table>
        }
        .into_any()
    };
    view! {
        <section class="sida">
            <Inställningsmeny bas=bas.clone() aktiv="/behandlingshistorik" />
            <p class="diskret">{företaget.namn} " · " {företaget.orgnr}</p>
            <h1>"Behandlingshistorik"</h1>
            <p class="diskret">
                "Varje ändring som påverkar bokföringen, med tid och aktör. Posterna kan inte ändras eller raderas."
            </p>
            <form class="historikfilter" method="get" action=sida.clone()>
                <label>
                    "Typ "
                    <select name="typ">
                        <option value="">"Alla typer"</option>
                        {typer}
                    </select>
                </label>
                <label>
                    "Aktör "
                    <select name="aktor">
                        <option value="">"Alla aktörer"</option>
                        {aktörer}
                    </select>
                </label>
                <label>"Från " <input type="date" name="fran" value=u.fran.clone() /></label>
                <label>"Till " <input type="date" name="till" value=u.till.clone() /></label>
                <button class="knapp" type="submit">
                    "Visa"
                </button>
                <a rel="external" href=sida.clone()>
                    "Rensa"
                </a>
            </form>
            <div class="historikexport">
                <p class="diskret">{antal}</p>
                <Utskrift href=pdf filnamn=format!("{}.pdf", data.filnamn) />
                <p class="utskrift">
                    <a
                        class="exportera-csv"
                        href=csv
                        download=format!("{}.csv", data.filnamn)
                        rel="external"
                    >
                        "Exportera CSV"
                    </a>
                </p>
            </div>
            {tabell}
        </section>
    }
    .into_any()
}
