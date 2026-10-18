//! axum-routern: sidor, server functions, assets och `/healthz` på en port och ett origin.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use axum::Router;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use leptos::prelude::*;
use leptos_axum::{LeptosRoutes, generate_route_list};
use maya_auth::Passkeys;
use maya_core::config::Config;
use maya_core::installation::RelyingParty;
use maya_core::{Databas, Klocka};

use crate::app::{App, shell};
use crate::assets;
use crate::foretag::server::{self as foretag, FöretagsState};
use crate::ingaende_balans::server::{self as ingaende_balans, IngåendeBalansState};
use crate::inloggning::{self, InloggningsState};
use crate::installningar::server::{self as installningar, InställningsState};
use crate::konto::server as konto;
use crate::kontoplan::server::{self as kontoplan, KontoplansState};
use crate::sok::server as sok;
use crate::underlag::server::{self as underlag, UnderlagState};
use crate::utskrift::server::{self as utskrift, UtskriftsState};
use crate::verifikat::server::{self as verifikat, VerifikatState};

#[derive(Clone)]
struct AppState {
    config: Arc<Config>,
    db: Databas,
}

/// Hela routern. Servern måste köras med `into_make_service_with_connect_info::<SocketAddr>()`
/// för att klientens adress ska bli känd.
pub fn router(config: Arc<Config>, db: Databas) -> Router {
    let leptos_options = LeptosOptions::builder()
        .output_name("maya")
        .site_pkg_dir("pkg")
        .site_addr(config.listen)
        .build();
    let routes = generate_route_list(App);
    let klocka = Klocka::system();
    let passkeys = Passkeys::new(
        &RelyingParty::från_publik_url(&config.public_url),
        db.clone(),
        klocka.clone(),
    )
    .expect("den publika URL:en är kontrollerad i konfigurationen");
    let inloggning = InloggningsState {
        config: config.clone(),
        db: db.clone(),
        klocka,
        passkeys: Arc::new(passkeys),
    };
    let företag = FöretagsState {
        db: db.clone(),
        klocka: inloggning.klocka.clone(),
    };
    let kontoplan = KontoplansState {
        db: db.clone(),
        klocka: inloggning.klocka.clone(),
    };
    let verifikat = VerifikatState {
        db: db.clone(),
        klocka: inloggning.klocka.clone(),
    };
    let inställningar = InställningsState {
        db: db.clone(),
        klocka: inloggning.klocka.clone(),
    };
    let utskrift = UtskriftsState {
        db: db.clone(),
        klocka: inloggning.klocka.clone(),
    };
    let kontext = {
        let db = db.clone();
        let klocka = inloggning.klocka.clone();
        move || {
            provide_context(db.clone());
            provide_context(klocka.clone());
        }
    };
    let underlag = UnderlagState {
        db: db.clone(),
        klocka: inloggning.klocka.clone(),
    };
    let ingående_balans = IngåendeBalansState {
        db: db.clone(),
        klocka: inloggning.klocka.clone(),
    };
    let state = AppState { config, db };

    let pages = Router::new()
        .leptos_routes_with_context(&leptos_options, routes, kontext, {
            let options = leptos_options.clone();
            move || shell(options.clone())
        })
        .fallback(assets::serve_site)
        .with_state(leptos_options)
        .layer(middleware::from_fn_with_state(
            state.db.clone(),
            sok::lägg_på_sökdata,
        ))
        .layer(middleware::from_fn_with_state(
            kontoplan.clone(),
            kontoplan::lägg_på_kontoplan,
        ))
        .layer(middleware::from_fn_with_state(
            verifikat.clone(),
            verifikat::lägg_på_verifikatdata,
        ))
        .layer(middleware::from_fn_with_state(
            underlag.clone(),
            underlag::lägg_på_underlagsdata,
        ))
        .layer(middleware::from_fn_with_state(
            ingående_balans.clone(),
            ingaende_balans::lägg_på_ingående_balans,
        ))
        .layer(middleware::from_fn_with_state(
            inställningar.clone(),
            installningar::lägg_på_inställningar,
        ))
        .layer(middleware::from_fn_with_state(
            inloggning.clone(),
            konto::lägg_på_kontodata,
        ))
        .layer(middleware::from_fn_with_state(
            företag.clone(),
            foretag::lägg_på_företagsval,
        ))
        .layer(middleware::from_fn_with_state(
            inloggning.clone(),
            inloggning::identifiera,
        ))
        .merge(
            kontoplan::routes(kontoplan)
                .layer(middleware::from_fn_with_state(
                    inloggning.clone(),
                    inloggning::identifiera,
                ))
                .layer(middleware::from_fn_with_state(
                    inloggning.clone(),
                    inloggning::kräv_eget_origin,
                )),
        )
        .merge(
            foretag::routes(företag)
                .layer(middleware::from_fn_with_state(
                    inloggning.clone(),
                    inloggning::identifiera,
                ))
                .layer(middleware::from_fn_with_state(
                    inloggning.clone(),
                    inloggning::kräv_eget_origin,
                )),
        )
        .merge(
            verifikat::routes(verifikat)
                .layer(middleware::from_fn_with_state(
                    inloggning.clone(),
                    inloggning::identifiera,
                ))
                .layer(middleware::from_fn_with_state(
                    inloggning.clone(),
                    inloggning::kräv_eget_origin,
                )),
        )
        .merge(
            underlag::routes(underlag)
                .layer(middleware::from_fn_with_state(
                    inloggning.clone(),
                    inloggning::identifiera,
                ))
                .layer(middleware::from_fn_with_state(
                    inloggning.clone(),
                    inloggning::kräv_eget_origin,
                )),
        )
        .merge(
            ingaende_balans::routes(ingående_balans)
                .layer(middleware::from_fn_with_state(
                    inloggning.clone(),
                    inloggning::identifiera,
                ))
                .layer(middleware::from_fn_with_state(
                    inloggning.clone(),
                    inloggning::kräv_eget_origin,
                )),
        )
        .merge(
            installningar::routes(inställningar)
                .layer(middleware::from_fn_with_state(
                    inloggning.clone(),
                    inloggning::identifiera,
                ))
                .layer(middleware::from_fn_with_state(
                    inloggning.clone(),
                    inloggning::kräv_eget_origin,
                )),
        )
        .merge(
            utskrift::routes(utskrift).layer(middleware::from_fn_with_state(
                inloggning.clone(),
                inloggning::identifiera,
            )),
        )
        .merge(
            inloggning::routes(inloggning.clone())
                .layer(middleware::from_fn_with_state(
                    inloggning.clone(),
                    inloggning::identifiera,
                ))
                .layer(middleware::from_fn_with_state(
                    inloggning,
                    inloggning::kräv_eget_origin,
                )),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_public_host,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            record_client_ip,
        ));

    Router::new()
        .route("/healthz", get(healthz))
        .with_state(state)
        .merge(pages)
}

/// Docker hälsokontroll når Maya på `localhost`, så `/healthz` kräver inte rätt `Host`.
async fn healthz(State(state): State<AppState>) -> Response {
    match state.db.kontrollera().await {
        Ok(()) => (StatusCode::OK, "ok").into_response(),
        Err(e) => {
            tracing::error!("hälsokontrollen misslyckades: {e}");
            (StatusCode::SERVICE_UNAVAILABLE, "databasen svarar inte").into_response()
        }
    }
}

/// Avvisar en begäran vars `Host` inte är den publika URL:ens värd. Den publika URL:en
/// härleds aldrig ur begäran.
async fn require_public_host(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let expected = state.config.public_host();
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .or_else(|| req.uri().authority().map(|a| a.as_str()));
    match host {
        Some(host) if host.eq_ignore_ascii_case(&expected) => next.run(req).await,
        other => {
            tracing::warn!(host = ?other, "avvisade en begäran med fel Host");
            (
                StatusCode::MISDIRECTED_REQUEST,
                format!("Maya svarar bara på {expected}.\n"),
            )
                .into_response()
        }
    }
}

/// Klientens adress för loggning och sessioner, lagd som tillägg på begäran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientIp(pub IpAddr);

async fn record_client_ip(State(state): State<AppState>, mut req: Request, next: Next) -> Response {
    if let Some(ConnectInfo(peer)) = req.extensions().get::<ConnectInfo<SocketAddr>>().copied() {
        let ip = client_ip(peer.ip(), req.headers(), &state.config);
        req.extensions_mut().insert(ClientIp(ip));
    }
    next.run(req).await
}

/// Klientens adress: anslutningens adress, eller ur `X-Forwarded-For` när anslutningen kommer
/// från en betrodd proxy. Då gäller den sista adressen i kedjan som inte själv är betrodd.
pub fn client_ip(peer: IpAddr, headers: &HeaderMap, config: &Config) -> IpAddr {
    if !config.is_trusted_proxy(peer) {
        return peer;
    }
    let forwarded: Vec<IpAddr> = headers
        .get_all("x-forwarded-for")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .filter_map(|ip| ip.trim().parse().ok())
        .collect();
    forwarded
        .into_iter()
        .rev()
        .find(|ip| !config.is_trusted_proxy(*ip))
        .unwrap_or(peer)
}
