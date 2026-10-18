//! Inloggningen i webben: middleware som gör om sessionscookien till en aktör, och JSON-
//! ändpunkterna som JS-shimmen anropar under passkey-ceremonierna.

use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use maya_auth::{AuthFel, CeremoniId, Passkeys, PublicKeyCredential, RegisterPublicKeyCredential};
use maya_core::config::Config;
use maya_core::inloggning::{self, Inloggad, SESSION_LÄNGST, Session};
use maya_core::{Databas, Klocka};
use serde::Deserialize;
use serde_json::json;

use crate::app::Läge;

pub const SESSIONSCOOKIE: &str = "maya_session";

const SHIM: &str = include_str!("webauthn.js");

#[derive(Clone)]
pub(crate) struct InloggningsState {
    pub config: Arc<Config>,
    pub db: Databas,
    pub klocka: Klocka,
    pub passkeys: Arc<Passkeys>,
}

pub(crate) fn routes(state: InloggningsState) -> Router {
    Router::new()
        .route("/auth/webauthn.js", get(shim))
        .route("/auth/registrering/start", post(börja_registrering))
        .route("/auth/registrering/slutfor", post(slutför_registrering))
        .route("/auth/inloggning/start", post(börja_inloggning))
        .route("/auth/inloggning/slutfor", post(slutför_inloggning))
        .route("/auth/utloggning", post(logga_ut))
        .with_state(state)
}

/// Gör om sessionscookien till den inloggade och lägger den, och sidans [`Läge`], som tillägg
/// på begäran. `core` auktoriserar sedan med aktören.
pub(crate) async fn identifiera(
    State(state): State<InloggningsState>,
    mut req: Request,
    next: Next,
) -> Response {
    let inloggad = match sessionstoken(req.headers()) {
        Some(token) => match inloggning::session(&state.db, &state.klocka, &token).await {
            Ok(inloggad) => inloggad,
            Err(e) => return serverfel(e),
        },
        None => None,
    };
    let läge = match &inloggad {
        Some(inloggad) => Läge::Inloggad {
            visningsnamn: inloggad.användare.visningsnamn.clone(),
        },
        None => match inloggning::finns_användare(&state.db).await {
            Ok(true) => Läge::Utloggad,
            Ok(false) => Läge::IngenAnvändare,
            Err(e) => return serverfel(e),
        },
    };
    req.extensions_mut().insert(läge);
    if let Some(inloggad) = inloggad {
        req.extensions_mut().insert::<Inloggad>(inloggad);
    }
    next.run(req).await
}

/// Avvisar en POST vars `Origin` inte är Mayas, som skydd mot CSRF utöver SameSite=Lax.
pub(crate) async fn kräv_eget_origin(
    State(state): State<InloggningsState>,
    req: Request,
    next: Next,
) -> Response {
    if req.method() == Method::POST {
        let eget = state.config.public_url.origin().ascii_serialization();
        let origin = req
            .headers()
            .get(header::ORIGIN)
            .and_then(|o| o.to_str().ok());
        if origin != Some(eget.as_str()) {
            tracing::warn!(origin = ?origin, "avvisade en POST från ett annat origin");
            return (StatusCode::FORBIDDEN, "Fel origin.\n").into_response();
        }
    }
    next.run(req).await
}

fn sessionstoken(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|kaka| kaka.trim().split_once('='))
        .find(|(namn, _)| *namn == SESSIONSCOOKIE)
        .map(|(_, värde)| värde.to_string())
}

/// Webbläsaren som skapar en session, så att användaren känner igen den i sessionslistan.
fn enhet(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .filter(|v| !v.trim().is_empty())
}

fn sessionscookie(session: &Session) -> String {
    format!(
        "{SESSIONSCOOKIE}={}; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age={}",
        session.token(),
        SESSION_LÄNGST.as_secs()
    )
}

async fn shim() -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        SHIM,
    )
        .into_response()
}

#[derive(Deserialize)]
struct BörjaRegistrering {
    token: String,
}

async fn börja_registrering(
    State(state): State<InloggningsState>,
    Json(begäran): Json<BörjaRegistrering>,
) -> Response {
    match state.passkeys.börja_registrering(&begäran.token).await {
        Ok((ceremoni, utmaning)) => {
            Json(json!({ "ceremoni": ceremoni, "utmaning": utmaning })).into_response()
        }
        Err(e) => fel(e),
    }
}

#[derive(Deserialize)]
struct SlutförRegistrering {
    ceremoni: CeremoniId,
    #[serde(default)]
    namn: String,
    svar: RegisterPublicKeyCredential,
}

async fn slutför_registrering(
    State(state): State<InloggningsState>,
    headers: HeaderMap,
    Json(begäran): Json<SlutförRegistrering>,
) -> Response {
    match state
        .passkeys
        .slutför_registrering(
            begäran.ceremoni,
            &begäran.namn,
            &begäran.svar,
            enhet(&headers),
        )
        .await
    {
        Ok(session) => inloggad(&session),
        Err(e) => fel(e),
    }
}

#[derive(Deserialize)]
struct BörjaInloggning {
    epost: String,
}

async fn börja_inloggning(
    State(state): State<InloggningsState>,
    Json(begäran): Json<BörjaInloggning>,
) -> Response {
    match state.passkeys.börja_inloggning(&begäran.epost).await {
        Ok((ceremoni, utmaning)) => {
            Json(json!({ "ceremoni": ceremoni, "utmaning": utmaning })).into_response()
        }
        Err(e) => fel(e),
    }
}

#[derive(Deserialize)]
struct SlutförInloggning {
    ceremoni: CeremoniId,
    svar: PublicKeyCredential,
}

async fn slutför_inloggning(
    State(state): State<InloggningsState>,
    headers: HeaderMap,
    Json(begäran): Json<SlutförInloggning>,
) -> Response {
    match state
        .passkeys
        .slutför_inloggning(begäran.ceremoni, &begäran.svar, enhet(&headers))
        .await
    {
        Ok(session) => inloggad(&session),
        Err(e) => fel(e),
    }
}

async fn logga_ut(State(state): State<InloggningsState>, headers: HeaderMap) -> Response {
    if let Some(token) = sessionstoken(&headers)
        && let Err(e) = inloggning::logga_ut(&state.db, &token).await
    {
        return serverfel(e);
    }
    let rensa = format!("{SESSIONSCOOKIE}=; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age=0");
    ([(header::SET_COOKIE, rensa)], Redirect::to("/")).into_response()
}

fn inloggad(session: &Session) -> Response {
    (
        [(header::SET_COOKIE, sessionscookie(session))],
        Json(json!({ "ok": true })),
    )
        .into_response()
}

fn fel(e: AuthFel) -> Response {
    if !e.klientens() {
        return serverfel(e);
    }
    tracing::info!("passkey-ceremonin misslyckades: {e}");
    let text = match &e {
        // Detaljerna från webauthn-rs är på engelska och hör hemma i loggen.
        AuthFel::Webauthn(_) => "passkeyn kunde inte verifieras".to_string(),
        other => other.to_string(),
    };
    (StatusCode::BAD_REQUEST, Json(json!({ "fel": text }))).into_response()
}

fn serverfel(e: impl std::fmt::Display) -> Response {
    tracing::error!("inloggningen misslyckades: {e}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "fel": "något gick fel i Maya; försök igen" })),
    )
        .into_response()
}
