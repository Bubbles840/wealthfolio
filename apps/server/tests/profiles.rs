use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    middleware,
    routing::get,
    Extension, Json, Router,
};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tower::ServiceExt;
use wealthfolio_core::settings::SettingsServiceTrait;
use wealthfolio_server::{
    build_state,
    config::Config,
    profiles::{self, WebProfiles},
    AppState,
};

async fn send(
    router: &Router,
    path: &str,
    body: Value,
    cookie: Option<&str>,
    scope: Option<&str>,
) -> (StatusCode, Value, Option<String>) {
    let mut request = Request::builder()
        .uri(path)
        .method(if path == "/data" { "GET" } else { "POST" })
        .header("content-type", "application/json");
    if let Some(cookie) = cookie {
        request = request.header("cookie", cookie);
    }
    if let Some(scope) = scope {
        request = request.header("x-wf-profile-scope", scope);
    }
    let response = router
        .clone()
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let cookie = response
        .headers()
        .get("set-cookie")
        .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string());
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let body =
        serde_json::from_slice(&bytes).unwrap_or_else(|_| json!(String::from_utf8_lossy(&bytes)));
    (status, body, cookie)
}

fn test_config(path: &std::path::Path) -> Config {
    Config {
        listen_addr: "127.0.0.1:0".parse().unwrap(),
        db_path: path.join("app.db").to_string_lossy().into_owned(),
        cors_allow: vec!["*".into()],
        request_timeout: Duration::from_secs(30),
        static_dir: "dist".into(),
        addons_root: path.to_string_lossy().into_owned(),
        raw_secret_key: vec![7; 32],
        secrets_encryption_key: [7; 32],
        database_key: [9; 32],
        db_encryption_required: false,
        auth: None,
        oidc: None,
        mcp_enabled: false,
        mcp_audit_enabled: false,
        mcp_allowed_hosts: None,
    }
}

#[tokio::test]
async fn browsers_databases_credentials_and_stale_scopes_are_isolated() {
    let dir = tempfile::tempdir().unwrap();
    let config = test_config(dir.path());
    let a = build_state(&config).await.unwrap();
    let root = WebProfiles::new(a.clone(), &config).unwrap();
    let b = root
        .registry
        .create("Second", "clay-fluff-animated")
        .unwrap();
    let b_state = root.runtime(b.id).await.unwrap();
    a.settings_service
        .set_setting_value("profile-test", "A")
        .await
        .unwrap();
    b_state
        .settings_service
        .set_setting_value("profile-test", "B")
        .await
        .unwrap();
    a.secret_store.set_secret("provider", "A secret").unwrap();
    b_state
        .secret_store
        .set_secret("provider", "B secret")
        .unwrap();
    assert_ne!(a.db_path, b_state.db_path);
    assert_eq!(
        a.secret_store.get_secret("provider").unwrap().as_deref(),
        Some("A secret")
    );
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let router = Router::new()
        .route(
            "/data",
            get(|Extension(state): Extension<Arc<AppState>>| async move {
                Json(json!(state
                    .settings_service
                    .get_setting_value("profile-test")
                    .unwrap()))
            }),
        )
        .route(
            "/delayed",
            axum::routing::post({
                let entered = entered.clone();
                let release = release.clone();
                move |Extension(state): Extension<Arc<AppState>>| {
                    let entered = entered.clone();
                    let release = release.clone();
                    async move {
                        entered.notify_one();
                        release.notified().await;
                        state
                            .settings_service
                            .set_setting_value("profile-test", "A completed")
                            .await
                            .unwrap();
                        Json(json!("private A response"))
                    }
                }
            }),
        )
        .layer(middleware::from_fn_with_state(
            root.clone(),
            profiles::admit,
        ))
        .merge(profiles::router(root.clone()))
        .layer(middleware::from_fn_with_state(
            a.clone(),
            wealthfolio_server::auth::require_jwt,
        ))
        .with_state(a);
    let (_, _, cookie_a) = send(
        &router,
        "/profiles/get_profile_state",
        json!({}),
        None,
        None,
    )
    .await;
    let (_, _, cookie_b) = send(
        &router,
        "/profiles/get_profile_state",
        json!({}),
        None,
        None,
    )
    .await;
    assert_ne!(cookie_a, cookie_b);
    let a_id = root.registry.default_id().unwrap();
    let (_, grant_a, _) = send(
        &router,
        "/profiles/unlock_profile",
        json!({"profileId":a_id}),
        cookie_a.as_deref(),
        None,
    )
    .await;
    let (_, grant_b, _) = send(
        &router,
        "/profiles/unlock_profile",
        json!({"profileId":b.id}),
        cookie_b.as_deref(),
        None,
    )
    .await;
    let scope_a = grant_a["scopeId"].as_str().unwrap();
    let scope_b = grant_b["scopeId"].as_str().unwrap();
    assert_eq!(
        send(
            &router,
            "/data",
            Value::Null,
            cookie_a.as_deref(),
            Some(scope_a)
        )
        .await
        .1,
        json!("A")
    );
    assert_eq!(
        send(
            &router,
            "/data",
            Value::Null,
            cookie_b.as_deref(),
            Some(scope_b)
        )
        .await
        .1,
        json!("B")
    );
    assert_eq!(
        send(
            &router,
            "/data",
            Value::Null,
            cookie_b.as_deref(),
            Some(scope_a)
        )
        .await
        .0,
        StatusCode::LOCKED
    );
    let delayed = tokio::spawn({
        let router = router.clone();
        let cookie = cookie_a.clone();
        let scope = scope_a.to_string();
        async move {
            send(
                &router,
                "/delayed",
                json!({}),
                cookie.as_deref(),
                Some(&scope),
            )
            .await
        }
    });
    entered.notified().await;
    send(
        &router,
        "/profiles/lock_profile",
        json!({}),
        cookie_a.as_deref(),
        None,
    )
    .await;
    release.notify_one();
    assert_eq!(delayed.await.unwrap().0, StatusCode::LOCKED);
    assert_eq!(
        root.runtime(a_id)
            .await
            .unwrap()
            .settings_service
            .get_setting_value("profile-test")
            .unwrap()
            .as_deref(),
        Some("A completed")
    );
    assert_eq!(
        b_state
            .settings_service
            .get_setting_value("profile-test")
            .unwrap()
            .as_deref(),
        Some("B")
    );
    assert_eq!(
        send(
            &router,
            "/data",
            Value::Null,
            cookie_a.as_deref(),
            Some(scope_a)
        )
        .await
        .0,
        StatusCode::LOCKED
    );
    assert_eq!(
        send(
            &router,
            "/data",
            Value::Null,
            cookie_b.as_deref(),
            Some(scope_b)
        )
        .await
        .0,
        StatusCode::OK
    );
    root.registry
        .set_password(b.id, None, Some("correct passphrase"))
        .unwrap();
    assert_eq!(
        send(
            &router,
            "/data",
            Value::Null,
            cookie_b.as_deref(),
            Some(scope_b)
        )
        .await
        .0,
        StatusCode::LOCKED
    );
    // MCP selects a database before validating its PAT and owns a separate
    // protocol session manager. A browser password lock does not revoke a PAT.
    use wealthfolio_server::mcp::auth::{generate_token, hash_token, token_prefix};
    use wealthfolio_storage_sqlite::agent::NewPersonalAccessToken;
    let a_state = root.runtime(a_id).await.unwrap();
    let mut tokens = Vec::new();
    for state in [&a_state, &b_state] {
        let token = generate_token();
        state
            .pat_repository
            .create(NewPersonalAccessToken {
                name: "test".into(),
                token_prefix: token_prefix(&token).unwrap().into(),
                token_hash: hash_token(&token),
                scopes_json: "[\"accounts:read\"]".into(),
                expires_at: None,
            })
            .await
            .unwrap();
        tokens.push(token);
    }
    let mcp = Router::new()
        .route("/mcp", axum::routing::any(profiles::mcp))
        .with_state(root.clone());
    let request = |token: &str, profile: Option<uuid::Uuid>, session: Option<&str>| {
        let mut request = Request::builder()
            .uri("/mcp")
            .header("host", "localhost")
            .method("POST")
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .header("authorization", format!("Bearer {token}"));
        if let Some(profile) = profile {
            request = request.header("x-wf-profile-id", profile.to_string());
        }
        if let Some(session) = session {
            request = request.header("mcp-session-id", session);
        }
        request.body(Body::from(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}).to_string())).unwrap()
    };
    assert_eq!(
        mcp.clone()
            .oneshot(request(&tokens[0], Some(b.id), None))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        mcp.clone()
            .oneshot(request(&tokens[1], None, None))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let response = mcp
        .clone()
        .oneshot(request(&tokens[0], None, None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mcp_session = response
        .headers()
        .get("mcp-session-id")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    drop(response);
    assert_eq!(
        mcp.clone()
            .oneshot(request(&tokens[1], Some(b.id), Some(&mcp_session)))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        mcp.oneshot(request(&tokens[1], Some(b.id), None))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn legacy_automatic_access_cannot_bypass_explicit_lock_or_malformed_scope() {
    let dir = tempfile::tempdir().unwrap();
    let config = test_config(dir.path());
    let state = build_state(&config).await.unwrap();
    let root = WebProfiles::new(state.clone(), &config).unwrap();
    let router = Router::new()
        .route("/data", get(|| async { Json(json!("data")) }))
        .layer(middleware::from_fn_with_state(
            root.clone(),
            profiles::admit,
        ))
        .merge(profiles::router(root.clone()))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            wealthfolio_server::auth::require_jwt,
        ))
        .with_state(state);
    let (status, _, cookie) = send(&router, "/data", Value::Null, None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        send(
            &router,
            "/data",
            Value::Null,
            cookie.as_deref(),
            Some("malformed")
        )
        .await
        .0,
        StatusCode::LOCKED
    );
    send(
        &router,
        "/profiles/lock_profile",
        json!({}),
        cookie.as_deref(),
        None,
    )
    .await;
    assert_eq!(
        send(&router, "/data", Value::Null, cookie.as_deref(), None)
            .await
            .0,
        StatusCode::LOCKED
    );
    assert!(send(
        &router,
        "/profiles/get_profile_state",
        json!({}),
        cookie.as_deref(),
        None
    )
    .await
    .1["session"]
        .is_null());
}

#[tokio::test]
async fn delete_profile_closes_runtime_and_supports_empty_installation() {
    let dir = tempfile::tempdir().unwrap();
    let config = test_config(dir.path());
    let router = wealthfolio_server::api::app_router_from_config(&config)
        .await
        .unwrap();
    let (_, initial, cookie) = send(
        &router,
        "/api/v1/profiles/get_profile_state",
        json!({}),
        None,
        None,
    )
    .await;
    let id = initial["profiles"][0]["id"].as_str().unwrap();
    let name = initial["profiles"][0]["name"].as_str().unwrap();
    let scope = initial["session"]["scopeId"].as_str().unwrap();
    let (status, backup, _) = send(
        &router,
        "/api/v1/utilities/database/backup",
        json!({}),
        cookie.as_deref(),
        Some(scope),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{backup}");
    let db_path = dir.path().join("profiles").join(id).join("app.db");
    let (status, _, _) = send(
        &router,
        "/api/v1/profiles/delete_profile",
        json!({"profileId":id,"confirmation":"wrong"}),
        cookie.as_deref(),
        Some(scope),
    )
    .await;
    assert_eq!(status, StatusCode::LOCKED);
    assert!(db_path.exists());
    let (status, body, _) = send(
        &router,
        "/api/v1/profiles/delete_profile",
        json!({"profileId":id,"confirmation":name}),
        cookie.as_deref(),
        Some(scope),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!db_path.exists());
    let (_, state, _) = send(
        &router,
        "/api/v1/profiles/get_profile_state",
        json!({}),
        cookie.as_deref(),
        None,
    )
    .await;
    assert_eq!(state["profiles"], json!([]));
    assert!(state["session"].is_null());
    drop(router);
    let reopened = WebProfiles::open(&config).await.unwrap();
    assert!(reopened.registry.list().unwrap().is_empty());
    assert!(reopened.registry.default_id().is_err());
    let new = reopened
        .registry
        .create("New", wealthfolio_core::profiles::PROFILE_AVATARS[0])
        .unwrap();
    assert!(reopened.runtime(new.id).await.is_ok());
}
