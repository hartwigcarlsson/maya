//! Rapporter: i dag bara SIE4-exporten, en fil per räkenskapsår. Balans- och resultaträkningen,
//! grundboken och huvudboken kommer med sina tickets.
//!
//! Filen hämtas med `GET /foretag/<id>/sie4?ar=<start>`. `core` läser tillbaka och stämmer av
//! filen innan den lämnas ut; stoppas exporten skickas användaren tillbaka hit med `fel=sie4`.
//! Årslistan läses med `sidodata::sida`. Länken till filen har `rel="external"`: den är ingen
//! sida i appen.

use leptos::prelude::*;
use leptos_router::hooks::{use_params_map, use_query_map};
use serde::{Deserialize, Serialize};

use crate::app::{LoggaIn, Läge};
use crate::foretag::{Företagsval, IngenÅtkomst};
use crate::sidodata::sida;

/// Koden i `fel=` när exporten stoppades vid återläsningen.
pub const SIE4_STOPPAD: &str = "sie4";

/// Det som sidan visar.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rapportdata {
    /// Räkenskapsåren i ordning, som start och slut `ÅÅÅÅ-MM-DD`.
    pub år: Vec<(String, String)>,
}

/// Datan för `/foretag/<id>/rapporter`.
#[server(prefix = "/api", endpoint = "rapportdata", input = server_fn::codec::GetUrl)]
pub async fn hämta_rapportdata(adress: String) -> Result<Rapportdata, ServerFnError> {
    match crate::sidodata::Sidbegäran::för(&adress)? {
        Some(begäran) => server::rapportdata(&begäran).await,
        None => Ok(Rapportdata::default()),
    }
}

#[cfg(feature = "ssr")]
pub(crate) mod server {
    use std::sync::Arc;

    use axum::extract::{Path, Query, State};
    use axum::http::{StatusCode, header};
    use axum::response::{IntoResponse, Redirect, Response};
    use axum::routing::get;
    use axum::{Extension, Router};
    use jiff::civil::Date;
    use leptos::prelude::ServerFnError;
    use maya_core::atkomst::ÅtkomstFel;
    use maya_core::foretag::{self, FöretagsId};
    use maya_core::inloggning::Inloggad;
    use maya_core::sie4::{self, Sie4Fel};
    use maya_core::{Databas, Klocka};
    use serde::Deserialize;

    use super::{Rapportdata, SIE4_STOPPAD};
    use crate::sidodata::{self, Sidbegäran};

    #[derive(Clone)]
    pub(crate) struct RapportState {
        pub db: Databas,
        pub klocka: Klocka,
    }

    pub(crate) fn routes(state: RapportState) -> Router {
        Router::new()
            .route("/foretag/{id}/sie4", get(exportera))
            .with_state(Arc::new(state))
    }

    /// Hämtar räkenskapsåren för `/foretag/<id>/rapporter`. Utan åtkomst blir datan tom;
    /// sidan visar det ur företagsvalet.
    pub(crate) async fn rapportdata(b: &Sidbegäran) -> Result<Rapportdata, ServerFnError> {
        let delar = b.delar();
        let ["foretag", id, "rapporter"] = delar.as_slice() else {
            return Ok(Rapportdata::default());
        };
        let Ok(företag) = id.parse::<FöretagsId>() else {
            return Ok(Rapportdata::default());
        };
        match foretag::företaget(&b.db, &b.inloggad.aktör, företag).await {
            Ok(f) => Ok(Rapportdata {
                år: f
                    .räkenskapsår
                    .iter()
                    .map(|å| (å.start.to_string(), å.slut.to_string()))
                    .collect(),
            }),
            Err(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte) => {
                Ok(Rapportdata::default())
            }
            Err(e) => Err(sidodata::serverfel(e)),
        }
    }

    #[derive(Deserialize)]
    struct Urval {
        #[serde(default)]
        ar: String,
    }

    /// SIE4-filen för räkenskapsåret som börjar `ar`, som en bilaga.
    async fn exportera(
        State(state): State<Arc<RapportState>>,
        Path(id): Path<String>,
        Query(urval): Query<Urval>,
        inloggad: Option<Extension<Inloggad>>,
    ) -> Response {
        let (Some(Extension(inloggad)), Ok(företag)) = (inloggad, id.parse::<FöretagsId>())
        else {
            return ingen_åtkomst();
        };
        let Ok(år) = urval.ar.parse::<Date>() else {
            return okänt_år();
        };
        match sie4::exportera(&state.db, &state.klocka, &inloggad.aktör, företag, år).await {
            Ok(export) => (
                StatusCode::OK,
                [
                    (
                        header::CONTENT_TYPE,
                        "text/plain; charset=IBM437".to_string(),
                    ),
                    (
                        header::CONTENT_DISPOSITION,
                        format!("attachment; filename=\"{}\"", export.filnamn),
                    ),
                    (header::CACHE_CONTROL, "no-store".to_string()),
                ],
                export.innehåll,
            )
                .into_response(),
            Err(e @ (Sie4Fel::Avvikelse(_) | Sie4Fel::Återläsning(_))) => {
                tracing::error!("SIE4-exporten stoppades: {e}");
                Redirect::to(&format!("/foretag/{företag}/rapporter?fel={SIE4_STOPPAD}"))
                    .into_response()
            }
            Err(Sie4Fel::OkäntRäkenskapsår) => okänt_år(),
            Err(Sie4Fel::Åtkomst(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte)) => {
                ingen_åtkomst()
            }
            Err(e) => serverfel(e),
        }
    }

    fn okänt_år() -> Response {
        (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            "Företaget har inget sådant räkenskapsår.\n",
        )
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
        tracing::error!("rapporter: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            "Något gick fel i Maya; försök igen.\n",
        )
            .into_response()
    }
}

/// Rapporter, med SIE4-exporten per räkenskapsår.
#[component]
pub fn Rapporter() -> impl IntoView {
    sida(hämta_rapportdata, rapporter)
}

fn rapporter(data: Rapportdata) -> AnyView {
    if !matches!(use_context::<Läge>(), Some(Läge::Inloggad { .. })) {
        return view! { <LoggaIn /> }.into_any();
    }
    let id = use_params_map().with_untracked(|p| p.get("id").unwrap_or_default());
    let val = use_context::<Företagsval>().unwrap_or_default();
    let Some(företaget) = val.0.into_iter().find(|f| f.id == id) else {
        return view! { <IngenÅtkomst /> }.into_any();
    };
    let stoppad = use_query_map().with_untracked(|q| q.get("fel").as_deref() == Some(SIE4_STOPPAD));
    let bas = företaget.href();
    let år = data
        .år
        .into_iter()
        .map(|(start, slut)| {
            view! {
                <li>
                    <a rel="external" href=format!("{bas}/sie4?ar={start}")>
                        {format!("{start} – {slut}")}
                    </a>
                </li>
            }
        })
        .collect_view();
    view! {
        <section class="sida">
            <p class="diskret">{företaget.namn} " · " {företaget.orgnr}</p>
            <h1>"Rapporter"</h1>
            <p class="tom">"Rapporterna kommer snart."</p>
            <h2>"SIE4"</h2>
            <p>
                "En SIE 4E-fil per räkenskapsår, för revisorn eller ett annat program. Maya läser "
                "tillbaka filen och stämmer av den mot bokföringen innan den laddas ned."
            </p>
            {stoppad
                .then(|| {
                    view! {
                        <p class="fel" role="alert">
                            "Exporten stoppades: filen stämmer inte med bokföringen. Ingen fil "
                            "laddades ned. Försök igen, och kontakta den som driver Maya om det "
                            "händer igen."
                        </p>
                    }
                })}
            <ul class="lista">{år}</ul>
        </section>
    }
    .into_any()
}
