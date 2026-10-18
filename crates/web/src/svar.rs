//! Svaren som webbens handlers delar: ingen åtkomst, serverfel och filer (PDF, CSV).

use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};

/// 403 med en svensk förklaring.
pub(crate) fn ingen_åtkomst() -> Response {
    text(
        StatusCode::FORBIDDEN,
        "Du har inte åtkomst till företaget.\n",
    )
}

/// 500 utan detaljer; felet loggas med var i webben det uppstod.
#[track_caller]
pub(crate) fn serverfel(e: impl std::fmt::Display) -> Response {
    let plats = std::panic::Location::caller();
    tracing::error!("{}:{}: {e}", plats.file(), plats.line());
    text(
        StatusCode::INTERNAL_SERVER_ERROR,
        "Något gick fel i Maya; försök igen.\n",
    )
}

/// Ren text med statuskoden.
pub(crate) fn text(status: StatusCode, text: &'static str) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        text,
    )
        .into_response()
}

/// En PDF `inline`, så att Skriv ut visar den; Exportera-länkens `download` sparar den.
pub(crate) fn pdf(pdf: maya_core::utskrift::Pdf) -> Response {
    fil(
        "application/pdf",
        format!(r#"inline; filename="{}""#, pdf.filnamn),
        pdf.innehåll,
    )
}

/// En CSV-fil som bilaga.
pub(crate) fn csv(filnamn: &str, innehåll: Vec<u8>) -> Response {
    fil(
        "text/csv; charset=utf-8",
        format!(r#"attachment; filename="{filnamn}""#),
        innehåll,
    )
}

fn fil(typ: &'static str, disposition: String, innehåll: Vec<u8>) -> Response {
    (
        [
            (header::CONTENT_TYPE, typ.to_string()),
            (header::CONTENT_DISPOSITION, disposition),
            (header::CACHE_CONTROL, "no-store".to_string()),
        ],
        innehåll,
    )
        .into_response()
}
