//! PDF:erna i webben: verifikatet, kontoplanen och behandlingshistoriken ur `core::utskrift`,
//! och behandlingshistorikens CSV ur `core::export`. Skriv ut och Exportera
//! är länkar till samma adress (se [`Utskrift`]), så det är alltid samma PDF.

use leptos::prelude::*;

/// Skriv ut och Exportera PDF för en PDF på `href`. Skriv ut öppnar PDF:en i en ny flik, där
/// webbläsaren skriver ut den; Exportera sparar samma fil som `filnamn`.
#[component]
pub fn Utskrift(href: String, filnamn: String) -> impl IntoView {
    view! {
        <p class="utskrift">
            <a class="skriv-ut" href=href.clone() target="_blank" rel="external noopener">
                "Skriv ut"
            </a>
            " · "
            <a class="exportera-pdf" href=href download=filnamn rel="external">
                "Exportera PDF"
            </a>
        </p>
    }
}

#[cfg(feature = "ssr")]
pub(crate) mod server {
    use std::sync::Arc;

    use axum::Router;
    use axum::extract::{Extension, Path, RawQuery, State};
    use axum::http::{StatusCode, header};
    use axum::response::{IntoResponse, Redirect, Response};
    use axum::routing::get;
    use maya_core::atkomst::ÅtkomstFel;
    use maya_core::foretag::FöretagsId;
    use maya_core::inloggning::Inloggad;
    use maya_core::utskrift::{self, Pdf, UtskriftFel};
    use maya_core::verifikat::VerifikatId;
    use maya_core::{Databas, Klocka, export};

    use crate::behandlingshistorik::server as behandlingshistorik;

    #[derive(Clone)]
    pub(crate) struct UtskriftsState {
        pub db: Databas,
        pub klocka: Klocka,
    }

    pub(crate) fn routes(state: UtskriftsState) -> Router {
        Router::new()
            .route("/foretag/{id}/verifikat/{v}/pdf", get(verifikat))
            .route("/foretag/{id}/installningar/kontoplan/pdf", get(kontoplan))
            .route(
                "/foretag/{id}/installningar/behandlingshistorik/pdf",
                get(behandlingshistorik_pdf),
            )
            .route(
                "/foretag/{id}/installningar/behandlingshistorik/csv",
                get(behandlingshistorik_csv),
            )
            .with_state(Arc::new(state))
    }

    async fn verifikat(
        State(state): State<Arc<UtskriftsState>>,
        Path((id, v)): Path<(String, String)>,
        inloggad: Option<Extension<Inloggad>>,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let Ok(företag) = id.parse::<FöretagsId>() else {
            return ingen_åtkomst();
        };
        let Ok(v) = v.parse::<VerifikatId>() else {
            return finns_inte();
        };
        match utskrift::verifikat(&state.db, &state.klocka, &inloggad.aktör, företag, v).await {
            Ok(Some(pdf)) => svar(pdf),
            Ok(None) => finns_inte(),
            Err(e) => fel(e),
        }
    }

    async fn kontoplan(
        State(state): State<Arc<UtskriftsState>>,
        Path(id): Path<String>,
        inloggad: Option<Extension<Inloggad>>,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let Ok(företag) = id.parse::<FöretagsId>() else {
            return ingen_åtkomst();
        };
        match utskrift::kontoplan(&state.db, &state.klocka, &inloggad.aktör, företag).await {
            Ok(pdf) => svar(pdf),
            Err(e) => fel(e),
        }
    }

    async fn behandlingshistorik_pdf(
        State(state): State<Arc<UtskriftsState>>,
        Path(id): Path<String>,
        RawQuery(query): RawQuery,
        inloggad: Option<Extension<Inloggad>>,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let Ok(företag) = id.parse::<FöretagsId>() else {
            return ingen_åtkomst();
        };
        let urval = behandlingshistorik::urval(query.as_deref());
        match utskrift::behandlingshistorik(
            &state.db,
            &state.klocka,
            &inloggad.aktör,
            företag,
            &urval,
        )
        .await
        {
            Ok(pdf) => svar(pdf),
            Err(e) => fel(e),
        }
    }

    /// Behandlingshistoriken som CSV, som bilaga: den öppnas i ett kalkylprogram, inte här.
    async fn behandlingshistorik_csv(
        State(state): State<Arc<UtskriftsState>>,
        Path(id): Path<String>,
        RawQuery(query): RawQuery,
        inloggad: Option<Extension<Inloggad>>,
    ) -> Response {
        let Some(Extension(inloggad)) = inloggad else {
            return Redirect::to("/").into_response();
        };
        let Ok(företag) = id.parse::<FöretagsId>() else {
            return ingen_åtkomst();
        };
        let urval = behandlingshistorik::urval(query.as_deref());
        match export::behandlingshistorik(
            &state.db,
            &state.klocka,
            &inloggad.aktör,
            företag,
            &urval,
        )
        .await
        {
            Ok(csv) => (
                [
                    (header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()),
                    (
                        header::CONTENT_DISPOSITION,
                        format!(r#"attachment; filename="{}""#, csv.filnamn),
                    ),
                    (header::CACHE_CONTROL, "no-store".to_string()),
                ],
                csv.innehåll,
            )
                .into_response(),
            Err(e) => fel(e.into()),
        }
    }

    /// `inline`, så att Skriv ut visar PDF:en; Exportera-länkens `download` sparar den.
    fn svar(pdf: Pdf) -> Response {
        (
            [
                (header::CONTENT_TYPE, "application/pdf".to_string()),
                (
                    header::CONTENT_DISPOSITION,
                    format!(r#"inline; filename="{}""#, pdf.filnamn),
                ),
                (header::CACHE_CONTROL, "no-store".to_string()),
            ],
            pdf.innehåll,
        )
            .into_response()
    }

    fn fel(e: UtskriftFel) -> Response {
        match e {
            UtskriftFel::Åtkomst(ÅtkomstFel::IngenÅtkomst | ÅtkomstFel::KanalenFårInte) => {
                ingen_åtkomst()
            }
            e => {
                tracing::error!("utskrift: {e}");
                text(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Något gick fel i Maya; försök igen.\n",
                )
            }
        }
    }

    fn ingen_åtkomst() -> Response {
        text(
            StatusCode::FORBIDDEN,
            "Du har inte åtkomst till företaget.\n",
        )
    }

    fn finns_inte() -> Response {
        text(StatusCode::NOT_FOUND, "Verifikatet finns inte.\n")
    }

    fn text(status: StatusCode, text: &'static str) -> Response {
        (
            status,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            text,
        )
            .into_response()
    }
}
