//! HTTP dashboard & API – axum 0.8 (supports both Cookie and Bearer auth)

use crate::{
    cfg::HttpCfg,
    status::{AppStatus, EventBus, SharedStatus},
};
use axum::{
    Extension, Router,
    body::Body,
    extract::{Json, State, connect_info::ConnectInfo},
    http::{HeaderMap, Request, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Redirect, Response, Sse},
    routing::{get, post},
};
use chrono::{Duration, Utc};
use jsonwebtoken as jwt;
use jwt::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
};
use tokio::net::TcpListener;
use tokio_stream::{StreamExt, wrappers::BroadcastStream};
use tracing::info;

/*──────────────────── JWT helpers ────────────────────*/

#[derive(Serialize, Deserialize)]
struct Claims {
    sub: String,
    exp: usize,
}

fn sign_jwt(username: &str, cfg: &HttpCfg) -> String {
    let claims = Claims {
        sub: username.to_owned(),
        exp: (Utc::now() + Duration::seconds(cfg.token_ttl_sec as i64)).timestamp() as usize,
    };
    jwt::encode(
        &Header::new(Algorithm::HS512),
        &claims,
        &EncodingKey::from_secret(cfg.jwt_secret.as_bytes()),
    )
    .expect("signing should never fail")
}

fn verify_jwt(token: &str, cfg: &HttpCfg) -> Option<Claims> {
    jwt::decode::<Claims>(
        token,
        &DecodingKey::from_secret(cfg.jwt_secret.as_bytes()),
        &Validation::new(Algorithm::HS512),
    )
    .ok()
    .map(|d| d.claims)
}

/*───────────────── private-network check ─────────────────*/
fn is_private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            v4.is_loopback()
                || o[0] == 10
                || (o[0] == 172 && (16..=31).contains(&o[1]))
                || (o[0] == 192 && o[1] == 168)
        }
        IpAddr::V6(v6) => v6.is_loopback() || (v6.segments()[0] & 0xfe00) == 0xfc00,
    }
}

/*──────────────────── middlewares ────────────────────*/

/// `intranet_only` gate
async fn intranet_guard(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Extension(cfg): Extension<Arc<HttpCfg>>,
    req: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    if !cfg.intranet_only || is_private(addr.ip()) {
        Ok(next.run(req).await)
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}

/// Bearer/Cookie JWT auth; when `[http.auth]` is not configured the guard
/// is bypassed.
async fn auth_guard(
    Extension(cfg): Extension<Arc<HttpCfg>>,
    req: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let path = req.uri().path();
    if path == "/login" || path == "/api/login" {
        return Ok(next.run(req).await);
    }

    let Some(auth_cfg) = &cfg.auth else {
        return Ok(next.run(req).await);
    };

    /* 1) Authorization: Bearer ... */
    let mut token_opt = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(|s| s.to_owned());

    /* 2) Cookie: ddns_token=... */
    if token_opt.is_none()
        && let Some(raw) = req
            .headers()
            .get(header::COOKIE)
            .and_then(|v| v.to_str().ok())
    {
        for kv in raw.split(';') {
            let mut it = kv.trim().splitn(2, '=');
            if let (Some(k), Some(v)) = (it.next(), it.next())
                && k == "ddns_token"
            {
                token_opt = Some(v.to_owned());
                break;
            }
        }
    }

    let ok = token_opt
        .as_deref()
        .and_then(|t| verify_jwt(t, &cfg))
        .map(|c| c.sub == auth_cfg.username)
        .unwrap_or(false);

    if ok {
        Ok(next.run(req).await)
    } else {
        let wants_html = req
            .headers()
            .get(header::ACCEPT)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.starts_with("text/html"))
            .unwrap_or(false);

        if wants_html {
            Ok(Redirect::temporary("/login").into_response())
        } else {
            Err(StatusCode::UNAUTHORIZED)
        }
    }
}

/*──────────────────── router bootstrap ────────────────────*/

pub async fn run_http_server(
    status: SharedStatus,
    bus_tx: EventBus,
    cfg: HttpCfg,
) -> anyhow::Result<()> {
    let cfg = Arc::new(cfg);

    let app = Router::new()
        // API
        .route("/api/status", get(api_status))
        .route("/api/events", get(api_events))
        .route("/api/login", post(api_login))
        // pages
        .route("/login", get(page_login))
        .route("/", get(page_dashboard))
        // shared state
        .with_state(AppState { status, bus_tx })
        // middlewares (inside-out)
        .layer(middleware::from_fn(auth_guard))
        .layer(middleware::from_fn(intranet_guard))
        .layer(Extension(cfg.clone()));

    let listener = TcpListener::bind(&cfg.listen).await?;
    info!("dashboard listening at http://{}", cfg.listen);

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

/*──────── shared state ────────*/
#[derive(Clone)]
struct AppState {
    status: SharedStatus,
    bus_tx: EventBus,
}

/*──────── page handlers ────────*/
async fn page_dashboard() -> Html<&'static str> {
    Html(include_str!("dashboard.html"))
}
async fn page_login() -> Html<&'static str> {
    Html(include_str!("login.html"))
}

/*──────── API handlers ────────*/
async fn api_status(State(st): State<AppState>) -> Json<AppStatus> {
    Json(st.status.read().clone())
}

async fn api_events(
    State(st): State<AppState>,
) -> Sse<
    impl futures_core::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>,
> {
    let stream = BroadcastStream::new(st.bus_tx.subscribe()).filter_map(|msg| {
        msg.ok().and_then(|evt| {
            serde_json::to_string(&evt)
                .ok()
                .map(|j| Ok(axum::response::sse::Event::default().data(j)))
        })
    });
    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default())
}

/*──────── login ────────*/
#[derive(Deserialize)]
struct LoginReq {
    username: String,
    password: String,
}

#[derive(Serialize)]
struct LoginResp {
    token: String,
}

async fn api_login(
    Extension(cfg): Extension<Arc<HttpCfg>>,
    Json(p): Json<LoginReq>,
) -> Result<impl IntoResponse, StatusCode> {
    let Some(auth) = &cfg.auth else {
        return Err(StatusCode::BAD_REQUEST);
    };

    if p.username != auth.username || p.password != auth.password {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let token = sign_jwt(&auth.username, &cfg);

    let mut headers = HeaderMap::new();
    headers.insert(
        header::SET_COOKIE,
        format!(
            "ddns_token={}; Path=/; Max-Age={}; SameSite=Lax; HttpOnly",
            token, cfg.token_ttl_sec
        )
        .parse()
        .unwrap(),
    );

    Ok((headers, Json(LoginResp { token })))
}
