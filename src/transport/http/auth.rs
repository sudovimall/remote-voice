use crate::{
    Error, Result,
    auth::{CurrentUser, model::AuthenticatedSession, session::now_epoch_seconds},
    config::settings::SessionSecureSetting,
    state::AppState,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use cookie::{Cookie, SameSite};
use serde::{Deserialize, Serialize};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/login", get(login_page))
        .route("/register", get(register_page))
        .route("/admin", get(admin_page))
        .route("/api/auth/login", post(login))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/register", post(register))
        .route("/api/auth/me", get(me))
        .route("/api/admin/invites", get(list_invites).post(create_invite))
        .route("/api/admin/users", get(list_users))
        .route("/api/admin/rooms", get(list_admin_rooms))
        .route("/api/admin/rooms/{room_id}/close", post(close_admin_room))
}

#[derive(Debug, Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Debug, Deserialize)]
struct RegisterRequest {
    code: String,
    username: String,
    password: String,
    display_name: String,
}

#[derive(Debug, Deserialize)]
struct CreateInviteRequest {
    #[serde(default = "default_invite_ttl_hours")]
    ttl_hours: u64,
}

#[derive(Debug, Serialize)]
struct AuthSessionResponse {
    user: CurrentUser,
    expires_at_epoch_seconds: i64,
}

#[derive(Debug, Serialize)]
struct MeResponse {
    auth_enabled: bool,
    user: Option<CurrentUser>,
}

#[derive(Debug, Serialize)]
struct CreatedInviteResponse {
    code: String,
    expires_at_epoch_seconds: i64,
    used_by_user_id: Option<i64>,
    used_at_epoch_seconds: Option<i64>,
    created_at_epoch_seconds: i64,
}

async fn login_page(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if current_user_from_headers(&state, &headers).is_ok() {
        return redirect_to("/").into_response();
    }
    Html(include_str!("../../../static/login.html")).into_response()
}

async fn register_page() -> Html<&'static str> {
    Html(include_str!("../../../static/register.html"))
}

async fn admin_page(State(state): State<AppState>, headers: HeaderMap) -> Response {
    match current_user_from_headers(&state, &headers) {
        Ok(user) if user.role == crate::auth::UserRole::Admin => {
            Html(include_str!("../../../static/admin.html")).into_response()
        }
        Ok(_) => Error::Forbidden.into_response(),
        Err(_) => login_redirect_response("/admin"),
    }
}

async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<LoginRequest>,
) -> Result<Response> {
    let service = auth_service(&state)?;
    let authenticated =
        service.login_at(&payload.username, &payload.password, now_epoch_seconds())?;
    session_response(&state, &headers, authenticated)
}

async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Result<Response> {
    if let (Some(service), Some(token)) = (state.auth.service(), session_token(&state, &headers)) {
        service.logout_at(&token, now_epoch_seconds())?;
    }

    let mut response = StatusCode::NO_CONTENT.into_response();
    if let Some(cookie_name) = session_cookie_name(&state) {
        response.headers_mut().insert(
            header::SET_COOKIE,
            expired_session_cookie(&cookie_name)
                .parse()
                .map_err(|error| Error::Internal(format!("清除 cookie 失败: {error}")))?,
        );
    }
    Ok(response)
}

async fn register(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<RegisterRequest>,
) -> Result<Response> {
    let service = auth_service(&state)?;
    let authenticated = service.register_with_invite_at(
        &payload.code,
        &payload.username,
        &payload.password,
        &payload.display_name,
        now_epoch_seconds(),
    )?;
    session_response(&state, &headers, authenticated)
}

async fn me(State(state): State<AppState>, headers: HeaderMap) -> Json<MeResponse> {
    let user = current_user_from_headers(&state, &headers).ok();
    Json(MeResponse {
        auth_enabled: state.auth.is_enabled(),
        user,
    })
}

async fn create_invite(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateInviteRequest>,
) -> Result<Json<CreatedInviteResponse>> {
    let user = api_user(&state, &headers)?;
    let service = auth_service(&state)?;
    let created = service.create_invite_at(&user, payload.ttl_hours, now_epoch_seconds())?;

    Ok(Json(CreatedInviteResponse {
        code: created.code,
        expires_at_epoch_seconds: created.invite.expires_at_epoch_seconds,
        used_by_user_id: created.invite.used_by_user_id,
        used_at_epoch_seconds: created.invite.used_at_epoch_seconds,
        created_at_epoch_seconds: created.invite.created_at_epoch_seconds,
    }))
}

async fn list_invites(State(state): State<AppState>, headers: HeaderMap) -> Result<Response> {
    let user = api_user(&state, &headers)?;
    let service = auth_service(&state)?;
    Ok(Json(service.list_invites(&user)?).into_response())
}

async fn list_users(State(state): State<AppState>, headers: HeaderMap) -> Result<Response> {
    let user = api_user(&state, &headers)?;
    let service = auth_service(&state)?;
    Ok(Json(service.list_users(&user)?).into_response())
}

async fn list_admin_rooms(State(state): State<AppState>, headers: HeaderMap) -> Result<Response> {
    let user = api_user(&state, &headers)?;
    let rooms = state
        .services
        .authenticated_rooms
        .list_open_for_admin(&user)?;
    Ok(Json(rooms).into_response())
}

async fn close_admin_room(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(room_id): Path<String>,
) -> Result<Response> {
    let user = api_user(&state, &headers)?;
    state
        .services
        .authenticated_rooms
        .close_as_admin(&user, &room_id)?;

    if state.rooms.close_room(&room_id).is_ok() {
        let _ = state.signals.broadcast(
            &room_id,
            crate::transport::http::signaling::ServerSignal::RoomClosed {
                room_id: room_id.clone(),
            },
            None,
        );
        let _ = state.signals.clear_room(&room_id);
    }

    Ok(StatusCode::NO_CONTENT.into_response())
}

pub(super) fn page_user_or_redirect(
    state: &AppState,
    headers: &HeaderMap,
    next: &str,
) -> std::result::Result<Option<CurrentUser>, Response> {
    if !state.auth.is_enabled() {
        return Ok(None);
    }

    current_user_from_headers(state, headers)
        .map(Some)
        .map_err(|_| login_redirect_response(next))
}

pub(super) fn api_user(state: &AppState, headers: &HeaderMap) -> Result<CurrentUser> {
    if !state.auth.is_enabled() {
        return Err(Error::AuthDisabled);
    }
    current_user_from_headers(state, headers).map_err(|_| Error::Unauthenticated)
}

pub(crate) fn current_user_from_headers(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<CurrentUser> {
    let token = session_token(state, headers).ok_or(Error::Unauthenticated)?;
    let service = auth_service(state)?;
    service.current_user_from_token_at(&token, now_epoch_seconds())
}

fn auth_service(state: &AppState) -> Result<&std::sync::Arc<crate::auth::service::AuthService>> {
    state.auth.service().ok_or(Error::AuthDisabled)
}

fn session_response(
    state: &AppState,
    headers: &HeaderMap,
    authenticated: AuthenticatedSession,
) -> Result<Response> {
    let body = Json(AuthSessionResponse {
        user: authenticated.user,
        expires_at_epoch_seconds: authenticated.expires_at_epoch_seconds,
    });
    let mut response = body.into_response();
    if let Some(cookie_name) = session_cookie_name(state) {
        let value = session_cookie(
            &cookie_name,
            &authenticated.token,
            authenticated.expires_at_epoch_seconds,
            secure_cookie_for_request(state, headers),
        )
        .parse()
        .map_err(|error| Error::Internal(format!("设置 cookie 失败: {error}")))?;
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    Ok(response)
}

fn session_token(state: &AppState, headers: &HeaderMap) -> Option<String> {
    let cookie_name = session_cookie_name(state)?;
    let cookie_header = headers.get(header::COOKIE)?.to_str().ok()?;
    for cookie in Cookie::split_parse(cookie_header) {
        let Ok(cookie) = cookie else {
            continue;
        };
        if cookie.name() == cookie_name {
            return Some(cookie.value().to_string());
        }
    }
    None
}

fn session_cookie_name(state: &AppState) -> Option<String> {
    state
        .auth
        .service()
        .map(|service| service.session_settings().cookie_name.clone())
}

fn secure_cookie_for_request(state: &AppState, headers: &HeaderMap) -> bool {
    let setting = state
        .auth
        .service()
        .map(|service| service.session_settings().secure)
        .unwrap_or(SessionSecureSetting::Never);
    match setting {
        SessionSecureSetting::Always => true,
        SessionSecureSetting::Never => false,
        SessionSecureSetting::Auto => request_is_https(headers),
    }
}

fn request_is_https(headers: &HeaderMap) -> bool {
    headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("https"))
        || headers
            .get("forwarded")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| {
                value.split(',').any(|part| {
                    part.split(';').any(|parameter| {
                        let parameter = parameter.trim();
                        parameter.strip_prefix("proto=").is_some_and(|proto| {
                            proto.trim_matches('"').eq_ignore_ascii_case("https")
                        })
                    })
                })
            })
}

fn session_cookie(name: &str, value: &str, expires_at_epoch_seconds: i64, secure: bool) -> String {
    let max_age_seconds = (expires_at_epoch_seconds - now_epoch_seconds()).max(0);
    Cookie::build((name.to_string(), value.to_string()))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(secure)
        .max_age(cookie::time::Duration::seconds(max_age_seconds))
        .build()
        .to_string()
}

fn expired_session_cookie(name: &str) -> String {
    Cookie::build((name.to_string(), ""))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .max_age(cookie::time::Duration::seconds(0))
        .build()
        .to_string()
}

fn login_redirect_response(next: &str) -> Response {
    redirect_to(&format!("/login?next={}", percent_encode(next))).into_response()
}

fn redirect_to(location: &str) -> Response {
    (
        StatusCode::FOUND,
        [(header::LOCATION, location.to_string())],
    )
        .into_response()
}

fn percent_encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn default_invite_ttl_hours() -> u64 {
    24
}

#[cfg(test)]
mod tests {
    use crate::{
        auth::password::hash_password,
        auth::{AuthRuntime, service::AuthService},
        config::settings::{AuthSessionSettings, SessionSecureSetting},
        state::AppState,
        transport::http::router,
    };
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode, header},
    };
    use serde_json::json;
    use std::sync::Arc;
    use tower::ServiceExt;

    fn auth_state() -> AppState {
        auth_state_with_session(AuthSessionSettings::default())
    }

    fn auth_state_with_session(session: AuthSessionSettings) -> AppState {
        let store = Arc::new(
            crate::storage::sqlite::SqliteStore::open_in_memory().expect("打开内存数据库"),
        );
        let service = Arc::new(AuthService::new(store, session));
        let admin_hash = hash_password("secret").expect("生成密码 hash");
        service
            .sync_admin("admin", &admin_hash, "管理员", 10)
            .expect("同步管理员");
        AppState::new_with_auth(8, AuthRuntime::Enabled(service)).expect("创建认证状态")
    }

    async fn login_set_cookie(app: axum::Router, username: &str, password: &str) -> String {
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/login")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({ "username": username, "password": password }).to_string(),
                    ))
                    .expect("构造登录请求"),
            )
            .await
            .expect("登录响应");
        assert_eq!(response.status(), StatusCode::OK);
        response
            .headers()
            .get(header::SET_COOKIE)
            .expect("登录设置 cookie")
            .to_str()
            .expect("cookie 是 UTF-8")
            .to_string()
    }

    async fn login_cookie(app: axum::Router, username: &str, password: &str) -> String {
        login_set_cookie(app, username, password)
            .await
            .split(';')
            .next()
            .expect("cookie pair")
            .to_string()
    }

    #[tokio::test]
    async fn 认证开启时未登录页面跳转_login() {
        let app = router(auth_state());

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .body(Body::empty())
                    .expect("构造请求"),
            )
            .await
            .expect("页面响应");

        assert_eq!(response.status(), StatusCode::FOUND);
        assert_eq!(
            response.headers().get(header::LOCATION).expect("location"),
            "/login?next=%2F"
        );
    }

    #[tokio::test]
    async fn 认证开启时未登录_api_返回_401() {
        let app = router(auth_state());

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/rooms")
                    .body(Body::empty())
                    .expect("构造请求"),
            )
            .await
            .expect("API 响应");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body = to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("读取响应体");
        let error: serde_json::Value = serde_json::from_slice(&body).expect("错误是 JSON");
        assert_eq!(error["code"], "unauthenticated");
    }

    #[tokio::test]
    async fn 登录成功设置_cookie_且可访问受保护页面() {
        let app = router(auth_state());
        let cookie = login_cookie(app.clone(), "admin", "secret").await;

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .expect("构造请求"),
            )
            .await
            .expect("页面响应");

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn 登录_cookie_遵守_secure_配置() {
        let mut always_secure = AuthSessionSettings::default();
        always_secure.secure = SessionSecureSetting::Always;
        let app = router(auth_state_with_session(always_secure));

        let secure_cookie = login_set_cookie(app, "admin", "secret").await;

        assert!(secure_cookie.contains("Secure"));

        let mut never_secure = AuthSessionSettings::default();
        never_secure.secure = SessionSecureSetting::Never;
        let app = router(auth_state_with_session(never_secure));

        let insecure_cookie = login_set_cookie(app, "admin", "secret").await;

        assert!(!insecure_cookie.contains("Secure"));
    }

    #[tokio::test]
    async fn secure_auto_根据请求协议设置_cookie() {
        let mut auto_secure = AuthSessionSettings::default();
        auto_secure.secure = SessionSecureSetting::Auto;
        let app = router(auth_state_with_session(auto_secure));

        let https_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/login")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header("x-forwarded-proto", "https")
                    .body(Body::from(
                        json!({ "username": "admin", "password": "secret" }).to_string(),
                    ))
                    .expect("构造 HTTPS 登录请求"),
            )
            .await
            .expect("HTTPS 登录响应");
        assert_eq!(https_response.status(), StatusCode::OK);
        let https_cookie = https_response
            .headers()
            .get(header::SET_COOKIE)
            .expect("HTTPS 登录设置 cookie")
            .to_str()
            .expect("cookie 是 UTF-8");
        assert!(https_cookie.contains("Secure"));

        let http_response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/login")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({ "username": "admin", "password": "secret" }).to_string(),
                    ))
                    .expect("构造 HTTP 登录请求"),
            )
            .await
            .expect("HTTP 登录响应");
        assert_eq!(http_response.status(), StatusCode::OK);
        let http_cookie = http_response
            .headers()
            .get(header::SET_COOKIE)
            .expect("HTTP 登录设置 cookie")
            .to_str()
            .expect("cookie 是 UTF-8");
        assert!(!http_cookie.contains("Secure"));
    }

    #[tokio::test]
    async fn 普通用户访问管理_api_返回_403() {
        let app = router(auth_state());
        let admin_cookie = login_cookie(app.clone(), "admin", "secret").await;

        let invite_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/admin/invites")
                    .header(header::COOKIE, admin_cookie)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(json!({ "ttl_hours": 24 }).to_string()))
                    .expect("构造创建邀请码请求"),
            )
            .await
            .expect("邀请码响应");
        let invite_body = to_bytes(invite_response.into_body(), 1024 * 1024)
            .await
            .expect("读取邀请码响应");
        let invite: serde_json::Value = serde_json::from_slice(&invite_body).expect("邀请码 JSON");

        let register_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/register")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({
                            "code": invite["code"],
                            "username": "alice",
                            "password": "password",
                            "display_name": "Alice"
                        })
                        .to_string(),
                    ))
                    .expect("构造注册请求"),
            )
            .await
            .expect("注册响应");
        let user_cookie = register_response
            .headers()
            .get(header::SET_COOKIE)
            .expect("注册设置 cookie")
            .to_str()
            .expect("cookie")
            .split(';')
            .next()
            .expect("cookie pair")
            .to_string();

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/admin/invites")
                    .header(header::COOKIE, user_cookie)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(json!({ "ttl_hours": 24 }).to_string()))
                    .expect("构造普通用户创建邀请码请求"),
            )
            .await
            .expect("管理 API 响应");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn 管理员可以关闭持久房间() {
        let state = auth_state();
        let service = state.auth.service().expect("认证服务").clone();
        let app = router(state);
        let admin_cookie = login_cookie(app.clone(), "admin", "secret").await;
        let token = admin_cookie
            .strip_prefix("remote_voice_session=")
            .expect("cookie token");
        let admin = service
            .current_user_from_token_at(token, crate::auth::session::now_epoch_seconds())
            .expect("当前用户");
        service
            .store()
            .create_persistent_room(
                "ABC123",
                admin.id,
                crate::auth::session::now_epoch_seconds(),
            )
            .expect("创建持久房间");

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/admin/rooms/ABC123/close")
                    .header(header::COOKIE, admin_cookie)
                    .body(Body::empty())
                    .expect("构造关闭房间请求"),
            )
            .await
            .expect("关闭房间响应");

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert!(
            service
                .store()
                .find_open_persistent_room("ABC123")
                .expect("查询开放房间")
                .is_none()
        );
    }
}
