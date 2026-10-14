//! PROTOTYP, slängkod. Svarar på ticketet "Kan Claude ansluta till Maya via CIMD utan DCR?".
//!
//! En MCP-server (rmcp, streamable HTTP) bakom en egen minimal OAuth 2.1-server i axum.
//! Ingen `/register`: klienter måste använda Client ID Metadata Documents.
//! Allt ligger i minnet. Allt som Claude skickar loggas, så att loggen är svaret.
//!
//! Miljövariabler:
//!   BASE_URL     publik bas-URL utan avslutande `/` (default http://localhost:8725)
//!   PORT         default 8725
//!   SESSIONS     1 = rmcp:s legacy_session_mode (MCP-Session-Id), 0 = tillståndslöst (default 0)
//!   ACCESS_TTL   access tokens livslängd i sekunder (default 120, kort för att tvinga fram refresh)

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    Form, Json, Router,
    body::Body,
    extract::{Query, Request, State},
    http::{HeaderMap, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Redirect, Response},
    routing::get,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    RoleServer,
    model::{ServerCapabilities, ServerConfig},
    schemars,
    service::RequestContext,
    tool, tool_handler, tool_router,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tracing::{info, warn};

// ---------- tillstånd ----------

struct AppState {
    base: String,
    mcp_url: String,
    access_ttl: Duration,
    http: reqwest::Client,
    pending: Mutex<HashMap<String, AuthRequest>>,
    codes: Mutex<HashMap<String, (AuthRequest, Instant)>>,
    access: Mutex<HashMap<String, Grant>>,
    refresh: Mutex<HashMap<String, Grant>>,
}

#[derive(Clone, Debug, Deserialize)]
struct AuthRequest {
    response_type: Option<String>,
    client_id: String,
    redirect_uri: String,
    code_challenge: Option<String>,
    code_challenge_method: Option<String>,
    state: Option<String>,
    resource: Option<String>,
    scope: Option<String>,
}

#[derive(Clone, Debug)]
struct Grant {
    client_id: String,
    resource: String,
    scope: String,
    expires: Instant,
}

fn random_token() -> String {
    URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>())
}

// ---------- discovery ----------

async fn protected_resource(State(s): State<Arc<AppState>>) -> Json<Value> {
    Json(json!({
        "resource": s.mcp_url,
        "authorization_servers": [s.base],
        "scopes_supported": ["maya"],
        "bearer_methods_supported": ["header"],
    }))
}

async fn authorization_server(State(s): State<Arc<AppState>>) -> Json<Value> {
    // Medvetet ingen registration_endpoint: Claude ska tvingas till CIMD.
    Json(json!({
        "issuer": s.base,
        "authorization_endpoint": format!("{}/authorize", s.base),
        "token_endpoint": format!("{}/token", s.base),
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "token_endpoint_auth_methods_supported": ["none"],
        "code_challenge_methods_supported": ["S256"],
        "client_id_metadata_document_supported": true,
        "authorization_response_iss_parameter_supported": true,
        "scopes_supported": ["maya", "offline_access"],
    }))
}

// ---------- CIMD ----------

fn is_loopback(u: &url::Url) -> bool {
    matches!(u.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
}

/// Exakt match, utom för loopback där porten ignoreras (RFC 8252 §7.3).
fn redirect_matches(registered: &str, given: &str) -> bool {
    if registered == given {
        return true;
    }
    let (Ok(mut r), Ok(mut g)) = (url::Url::parse(registered), url::Url::parse(given)) else {
        return false;
    };
    if is_loopback(&r) && is_loopback(&g) {
        let _ = r.set_port(None);
        let _ = g.set_port(None);
        return r == g;
    }
    false
}

async fn fetch_cimd(s: &AppState, client_id: &str, redirect_uri: &str) -> Result<Value, String> {
    let u = url::Url::parse(client_id).map_err(|e| format!("client_id är ingen URL: {e}"))?;
    if u.scheme() != "https" || u.path() == "/" {
        return Err(format!("client_id måste vara en https-URL med path, fick {client_id}"));
    }
    let resp = s
        .http
        .get(client_id)
        .send()
        .await
        .map_err(|e| format!("kunde inte hämta CIMD: {e}"))?;
    let status = resp.status();
    let cache = resp
        .headers()
        .get(header::CACHE_CONTROL)
        .map(|v| v.to_str().unwrap_or("").to_owned());
    let doc: Value = resp.json().await.map_err(|e| format!("CIMD är inte JSON: {e}"))?;
    info!(%client_id, %status, ?cache, doc = %doc, "CIMD hämtat");
    if doc["client_id"].as_str() != Some(client_id) {
        return Err(format!("CIMD:s client_id {} matchar inte URL:en", doc["client_id"]));
    }
    let ok = doc["redirect_uris"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .any(|r| redirect_matches(r, redirect_uri));
    if !ok {
        return Err(format!("redirect_uri {redirect_uri} finns inte i CIMD:s redirect_uris"));
    }
    Ok(doc)
}

// ---------- /authorize ----------

async fn authorize_get(
    State(s): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<AuthRequest>,
) -> Response {
    info!(request = ?q, user_agent = ?headers.get(header::USER_AGENT), "GET /authorize");
    if q.response_type.as_deref() != Some("code")
        || q.code_challenge_method.as_deref() != Some("S256")
        || q.code_challenge.is_none()
    {
        return error_page("response_type=code och PKCE S256 krävs");
    }
    let doc = match fetch_cimd(&s, &q.client_id, &q.redirect_uri).await {
        Ok(d) => d,
        Err(e) => {
            warn!(%e, "CIMD avvisat");
            return error_page(&e);
        }
    };
    let id = random_token();
    let name = doc["client_name"].as_str().unwrap_or("(namnlös klient)").to_owned();
    let redirect_host = url::Url::parse(&q.redirect_uri)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .unwrap_or_default();
    let loopback_note = if url::Url::parse(&q.redirect_uri).is_ok_and(|u| is_loopback(&u)) {
        "<p><b>Varning:</b> svaret går till en lokal adress. Fortsätt bara om du själv startade inloggningen.</p>"
    } else {
        ""
    };
    let page = format!(
        r#"<!doctype html><meta charset="utf-8"><title>Maya-stub: godkänn</title>
<body style="font:15px system-ui;max-width:36rem;margin:3rem auto">
<h1>Ge {name} åtkomst till Maya-stubben?</h1>
<p>client_id: <code>{cid}</code><br>Svaret skickas till: <b>{redirect_host}</b><br>scope: <code>{scope}</code><br>resource: <code>{resource}</code></p>
{loopback_note}
<form method="post" action="/authorize"><input type="hidden" name="id" value="{id}">
<button name="beslut" value="ja">Godkänn</button> <button name="beslut" value="nej">Neka</button></form>"#,
        cid = q.client_id,
        scope = q.scope.as_deref().unwrap_or("-"),
        resource = q.resource.as_deref().unwrap_or("-"),
    );
    s.pending.lock().unwrap().insert(id, q);
    Html(page).into_response()
}

#[derive(Deserialize)]
struct Decision {
    id: String,
    beslut: String,
}

async fn authorize_post(State(s): State<Arc<AppState>>, Form(d): Form<Decision>) -> Response {
    let Some(q) = s.pending.lock().unwrap().remove(&d.id) else {
        return error_page("okänd eller redan använd begäran");
    };
    let mut target = url::Url::parse(&q.redirect_uri).unwrap();
    {
        let mut qp = target.query_pairs_mut();
        if d.beslut == "ja" {
            let code = random_token();
            qp.append_pair("code", &code);
            s.codes
                .lock()
                .unwrap()
                .insert(code, (q.clone(), Instant::now() + Duration::from_secs(60)));
        } else {
            qp.append_pair("error", "access_denied");
        }
        if let Some(st) = &q.state {
            qp.append_pair("state", st);
        }
        qp.append_pair("iss", &s.base);
    }
    info!(beslut = %d.beslut, redirect = %target, "POST /authorize");
    Redirect::to(target.as_str()).into_response()
}

fn error_page(msg: &str) -> Response {
    (StatusCode::BAD_REQUEST, Html(format!("<meta charset=utf-8><h1>Fel</h1><p>{msg}</p>"))).into_response()
}

// ---------- /token ----------

async fn token(
    State(s): State<Arc<AppState>>,
    headers: HeaderMap,
    Form(f): Form<HashMap<String, String>>,
) -> Response {
    let redacted: HashMap<_, _> = f
        .iter()
        .map(|(k, v)| (k.clone(), if k.contains("token") || k == "code" || k == "code_verifier" { "…".into() } else { v.clone() }))
        .collect();
    info!(form = ?redacted, authorization = headers.contains_key(header::AUTHORIZATION), "POST /token");
    let grant = match f.get("grant_type").map(String::as_str) {
        Some("authorization_code") => {
            let Some((q, exp)) = f.get("code").and_then(|c| s.codes.lock().unwrap().remove(c)) else {
                return oauth_error("invalid_grant", "okänd kod");
            };
            let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(f.get("code_verifier").map_or("", String::as_str)));
            if Instant::now() > exp
                || f.get("client_id") != Some(&q.client_id)
                || f.get("redirect_uri") != Some(&q.redirect_uri)
                || Some(challenge) != q.code_challenge
            {
                return oauth_error("invalid_grant", "kod, klient, redirect_uri eller PKCE stämmer inte");
            }
            let resource = f.get("resource").or(q.resource.as_ref()).cloned().unwrap_or_else(|| s.mcp_url.clone());
            Grant {
                client_id: q.client_id,
                resource,
                scope: q.scope.unwrap_or_else(|| "maya".into()),
                expires: Instant::now(),
            }
        }
        Some("refresh_token") => {
            let Some(g) = f.get("refresh_token").and_then(|t| s.refresh.lock().unwrap().remove(t)) else {
                return oauth_error("invalid_grant", "okänd eller redan roterad refresh token");
            };
            if f.get("client_id").is_some_and(|c| *c != g.client_id) {
                return oauth_error("invalid_grant", "fel klient");
            }
            g
        }
        _ => return oauth_error("unsupported_grant_type", "bara authorization_code och refresh_token"),
    };
    let access = random_token();
    let refresh = random_token();
    s.access.lock().unwrap().insert(access.clone(), Grant { expires: Instant::now() + s.access_ttl, ..grant.clone() });
    s.refresh.lock().unwrap().insert(refresh.clone(), grant.clone());
    info!(client_id = %grant.client_id, resource = %grant.resource, scope = %grant.scope, "tokens utfärdade");
    Json(json!({
        "access_token": access,
        "token_type": "Bearer",
        "expires_in": s.access_ttl.as_secs(),
        "refresh_token": refresh,
        "scope": grant.scope,
    }))
    .into_response()
}

fn oauth_error(code: &str, desc: &str) -> Response {
    warn!(code, desc, "token-fel");
    (StatusCode::BAD_REQUEST, Json(json!({ "error": code, "error_description": desc }))).into_response()
}

// ---------- resource server-grind framför /mcp ----------

async fn bearer_gate(State(s): State<Arc<AppState>>, req: Request, next: Next) -> Response {
    let h = req.headers();
    let pick = |n: &str| h.get(n).and_then(|v| v.to_str().ok()).unwrap_or("-").to_owned();
    let (method, version, session, mcp_method, accept) =
        (req.method().clone(), pick("mcp-protocol-version"), pick("mcp-session-id"), pick("mcp-method"), pick("accept"));
    let bearer = pick("authorization").strip_prefix("Bearer ").map(str::to_owned);
    let (mut parts, body) = req.into_parts();
    let bytes = axum::body::to_bytes(body, 1 << 20).await.unwrap_or_default();
    let preview: String = String::from_utf8_lossy(&bytes).chars().take(400).collect();
    info!(%method, %version, %session, %mcp_method, %accept, token = bearer.is_some(), body = %preview, "/mcp");

    let grant = bearer.and_then(|t| s.access.lock().unwrap().get(&t).cloned());
    let grant = match grant {
        Some(g) if g.expires > Instant::now() && g.resource == s.mcp_url => g,
        other => {
            if let Some(g) = other {
                warn!(resource = %g.resource, expected = %s.mcp_url, expired = g.expires <= Instant::now(), "token avvisad");
            }
            let www = format!(
                r#"Bearer error="invalid_token", resource_metadata="{}/.well-known/oauth-protected-resource/mcp", scope="maya""#,
                s.base
            );
            return (StatusCode::UNAUTHORIZED, [(header::WWW_AUTHENTICATE, www)]).into_response();
        }
    };
    parts.extensions.insert(grant);
    let resp = next.run(Request::from_parts(parts, Body::from(bytes))).await;
    info!(status = %resp.status(), session_out = ?resp.headers().get("mcp-session-id"), content_type = ?resp.headers().get(header::CONTENT_TYPE), "/mcp svar");
    resp
}

// ---------- MCP-verktyg ----------

#[derive(Clone)]
struct Stub {
    tool_router: ToolRouter<Self>,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct EkoArgs {
    text: String,
}

#[tool_router]
impl Stub {
    #[tool(description = "Visar vilken klient, scope och protokollversion anropet kom med.")]
    async fn vem_ar_jag(&self, ctx: RequestContext<RoleServer>) -> String {
        let parts = ctx.extensions.get::<axum::http::request::Parts>();
        let grant = parts.and_then(|p| p.extensions.get::<Grant>());
        let hdr = |n: &str| parts.and_then(|p| p.headers.get(n)).and_then(|v| v.to_str().ok()).unwrap_or("-");
        format!(
            "client_id: {}\nscope: {}\nresource: {}\nMCP-Protocol-Version: {}\nMcp-Session-Id: {}",
            grant.map_or("-", |g| &g.client_id),
            grant.map_or("-", |g| &g.scope),
            grant.map_or("-", |g| &g.resource),
            hdr("mcp-protocol-version"),
            hdr("mcp-session-id"),
        )
    }

    #[tool(description = "Ekar tillbaka texten.")]
    fn eko(&self, Parameters(EkoArgs { text }): Parameters<EkoArgs>) -> String {
        text
    }
}

#[tool_handler]
impl ServerHandler for Stub {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("PROTOTYP av Maya: bara för att testa OAuth med CIMD.")
    }
}

// ---------- uppstart ----------

async fn log_all(req: Request, next: Next) -> Response {
    let (m, u, ua) = (req.method().clone(), req.uri().clone(), req.headers().get(header::USER_AGENT).cloned());
    let resp = next.run(req).await;
    info!(%m, %u, status = %resp.status(), ?ua, "http");
    resp
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "cimd_stub=info,rmcp=info".into()))
        .init();
    let env = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.into());
    let port = env("PORT", "8725");
    let base = env("BASE_URL", &format!("http://localhost:{port}")).trim_end_matches('/').to_owned();
    let sessions = env("SESSIONS", "0") == "1";
    let host = url::Url::parse(&base).unwrap().host_str().unwrap().to_owned();
    let s = Arc::new(AppState {
        mcp_url: format!("{base}/mcp"),
        base: base.clone(),
        access_ttl: Duration::from_secs(env("ACCESS_TTL", "120").parse().unwrap()),
        http: reqwest::Client::builder().timeout(Duration::from_secs(5)).build().unwrap(),
        pending: Default::default(),
        codes: Default::default(),
        access: Default::default(),
        refresh: Default::default(),
    });

    let config = StreamableHttpServerConfig::default()
        .with_legacy_session_mode(sessions)
        .with_json_response(!sessions)
        .with_allowed_hosts([host, "localhost".into(), "127.0.0.1".into()]);
    let mcp = StreamableHttpService::new(
        || Ok(Stub { tool_router: Stub::tool_router() }),
        Arc::new(LocalSessionManager::default()),
        config,
    );

    let app = Router::new()
        .route_service("/mcp", mcp)
        .route_layer(middleware::from_fn_with_state(s.clone(), bearer_gate))
        .route("/.well-known/oauth-protected-resource/mcp", get(protected_resource))
        .route("/.well-known/oauth-protected-resource", get(protected_resource))
        .route("/.well-known/oauth-authorization-server", get(authorization_server))
        .route("/authorize", get(authorize_get).post(authorize_post))
        .route("/token", axum::routing::post(token))
        .layer(middleware::from_fn(log_all))
        .with_state(s);

    info!(%base, sessions, "cimd-stub lyssnar på 127.0.0.1:{port}");
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}")).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
