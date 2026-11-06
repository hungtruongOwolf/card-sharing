//! T-144: staff sign-in with MFA (US-27, SR-32). Provider proof → login token → TOTP → staff
//! cookie + CSRF token; app bearer tokens never open staff routes; roles, CSRF, timeouts, replay
//! and the IP allow-list.

use crate::common::*;
use axum::Router;
use axum::body::Body;
use http::{Request, Response, StatusCode, header};
use identity::testing::random_nonce;
use identity::{Provider, StaffRole, VerifiedIdentity};
use platform::clock::Clock;
use serde_json::{Value, json};
use time::Duration;
use uuid::Uuid;

pub struct Staff {
    pub account: Uuid,
    pub subject: String,
    pub secret: Vec<u8>,
}

/// An active account with a staff role.
pub async fn staff_member(env: &TestEnv, role: StaffRole) -> Staff {
    let subject = format!("staff-{}", Uuid::now_v7().simple());
    let identity = VerifiedIdentity {
        provider: Provider::Apple,
        subject: subject.clone(),
        email: None,
    };
    let (account, _) = env.signed_in_as(&identity, Uuid::now_v7()).await;
    sqlx::query("UPDATE identity.accounts SET status = 'active' WHERE id = $1")
        .bind(account)
        .execute(env.identity.pool())
        .await
        .unwrap();
    let secret = env
        .state
        .staff
        .grant(account, role, &env.state.field_cipher)
        .await
        .unwrap();
    Staff {
        account,
        subject,
        secret,
    }
}

fn current_code(env: &TestEnv, staff: &Staff, offset_steps: i64) -> String {
    let step = identity::totp::step_at(env.clock.now().unix_timestamp()) + offset_steps;
    format!("{:06}", identity::totp::code(&staff.secret, step))
}

async fn login(env: &TestEnv, router: &Router, subject: &str) -> Response<Body> {
    let nonce = random_nonce();
    send(
        router,
        post_json(
            "/v1/admin/auth/login",
            None,
            &json!({
                "provider": "apple",
                "id_token": env.apple_id_token(subject, &nonce),
                "nonce": nonce,
            }),
        ),
    )
    .await
}

async fn mfa_token(env: &TestEnv, router: &Router, staff: &Staff) -> String {
    let response = login(env, router, &staff.subject).await;
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await["mfa_token"]
        .as_str()
        .unwrap()
        .to_owned()
}

async fn mfa(router: &Router, token: &str, code: &str) -> Response<Body> {
    send(
        router,
        post_json(
            "/v1/admin/auth/mfa",
            None,
            &json!({ "mfa_token": token, "code": code }),
        ),
    )
    .await
}

/// A signed-in staff session: the cookie value and the CSRF token.
pub struct StaffSession {
    pub cookie: String,
    pub csrf: String,
}

pub async fn signed_in_staff(env: &TestEnv, router: &Router, staff: &Staff) -> StaffSession {
    let token = mfa_token(env, router, staff).await;
    let response = mfa(router, &token, &current_code(env, staff, 0)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let set_cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    for attribute in ["HttpOnly", "Secure", "SameSite=Strict", "Path=/"] {
        assert!(set_cookie.contains(attribute), "{set_cookie}");
    }
    let cookie = set_cookie.split(';').next().unwrap().to_owned();
    assert!(cookie.starts_with("__Host-staff_session="), "{cookie}");
    let body = json_body(response).await;
    StaffSession {
        cookie,
        csrf: body["csrf_token"].as_str().unwrap().to_owned(),
    }
}

pub fn staff_request(
    method: &str,
    path: &str,
    session: &StaffSession,
    csrf: bool,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(header::COOKIE, &session.cookie);
    if csrf {
        builder = builder.header("x-csrf-token", &session.csrf);
    }
    builder.body(Body::empty()).unwrap()
}

#[tokio::test]
async fn a_moderator_signs_in_with_totp_and_reads_own_role() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let router = env.router.clone();
    let moderator = staff_member(&env, StaffRole::Moderator).await;
    let session = signed_in_staff(&env, &router, &moderator).await;
    let me = send(
        &router,
        staff_request("GET", "/v1/admin/me", &session, false),
    )
    .await;
    assert_eq!(me.status(), StatusCode::OK);
    let me = json_body(me).await;
    assert_eq!(me["account_id"], moderator.account.to_string());
    assert_eq!(me["role"], "moderator");

    // Admin-only route: 403 for a moderator.
    let staff = send(
        &router,
        staff_request("GET", "/v1/admin/staff", &session, false),
    )
    .await;
    assert_problem(staff, StatusCode::FORBIDDEN, "FORBIDDEN").await;
}

#[tokio::test]
async fn an_admin_lists_staff() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let router = env.router.clone();
    let admin = staff_member(&env, StaffRole::Admin).await;
    let moderator = staff_member(&env, StaffRole::Moderator).await;
    let session = signed_in_staff(&env, &router, &admin).await;
    let response = send(
        &router,
        staff_request("GET", "/v1/admin/staff", &session, false),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let items = json_body(response).await["items"]
        .as_array()
        .unwrap()
        .clone();
    let entry = items
        .iter()
        .find(|m| m["account_id"] == moderator.account.to_string())
        .unwrap();
    assert_eq!(entry["role"], "moderator");
    // No secret ever leaves the server.
    assert!(!Value::Array(items).to_string().contains("totp"));
}

#[tokio::test]
async fn app_bearer_tokens_never_open_staff_routes() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let router = env.router.clone();
    let admin = staff_member(&env, StaffRole::Admin).await;
    // The admin's own app session is not a staff session.
    let device = identity::DeviceInfo {
        id: Uuid::now_v7(),
        platform: identity::Platform::Ios,
        app_version: "1.0.0".to_owned(),
    };
    let app = env
        .identity
        .start_session(admin.account, Some(&device))
        .await
        .unwrap();
    for path in ["/v1/admin/me", "/v1/admin/staff"] {
        let response = send(&router, get_request(path, Some(app.access_token.expose()))).await;
        assert_problem(response, StatusCode::UNAUTHORIZED, "UNAUTHENTICATED").await;
    }
    // Neither does the app token pasted into the staff cookie.
    let forged = StaffSession {
        cookie: format!("__Host-staff_session={}", app.access_token.expose()),
        csrf: String::new(),
    };
    let response = send(
        &router,
        staff_request("GET", "/v1/admin/me", &forged, false),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn state_changing_staff_requests_need_the_csrf_token() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let router = env.router.clone();
    let moderator = staff_member(&env, StaffRole::Moderator).await;
    let session = signed_in_staff(&env, &router, &moderator).await;
    let missing = send(
        &router,
        staff_request("POST", "/v1/admin/auth/logout", &session, false),
    )
    .await;
    assert_problem(missing, StatusCode::FORBIDDEN, "FORBIDDEN").await;
    let wrong = StaffSession {
        cookie: session.cookie.clone(),
        csrf: "not-the-token".to_owned(),
    };
    let wrong = send(
        &router,
        staff_request("POST", "/v1/admin/auth/logout", &wrong, true),
    )
    .await;
    assert_eq!(wrong.status(), StatusCode::FORBIDDEN);
    // With the token: signed out, and the cookie no longer works.
    let out = send(
        &router,
        staff_request("POST", "/v1/admin/auth/logout", &session, true),
    )
    .await;
    assert_eq!(out.status(), StatusCode::NO_CONTENT);
    let cleared = out
        .headers()
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(cleared.contains("Max-Age=0"), "{cleared}");
    let me = send(
        &router,
        staff_request("GET", "/v1/admin/me", &session, false),
    )
    .await;
    assert_eq!(me.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn login_refusals_are_one_401() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let router = env.router.clone();
    // An account that is not staff.
    let subject = format!("plain-{}", Uuid::now_v7().simple());
    let identity = VerifiedIdentity {
        provider: Provider::Apple,
        subject: subject.clone(),
        email: None,
    };
    env.signed_in_as(&identity, Uuid::now_v7()).await;
    let not_staff = body_bytes(login(&env, &router, &subject).await).await;
    // A subject with no account at all.
    let unknown = login(&env, &router, "nobody-at-all").await;
    assert_eq!(unknown.status(), StatusCode::UNAUTHORIZED);
    let strip = |b: axum::body::Bytes| crate::media::without_request_id(b);
    assert_eq!(strip(body_bytes(unknown).await), strip(not_staff));
    // A disabled staff member.
    let moderator = staff_member(&env, StaffRole::Moderator).await;
    assert!(env.state.staff.disable(moderator.account).await.unwrap());
    let disabled = login(&env, &router, &moderator.subject).await;
    assert_problem(disabled, StatusCode::UNAUTHORIZED, "UNAUTHENTICATED").await;
}

#[tokio::test]
async fn codes_are_single_use_and_attempts_are_limited() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let router = env.router.clone();
    let moderator = staff_member(&env, StaffRole::Moderator).await;
    // Wrong codes burn the challenge after five attempts, even before it expires.
    let token = mfa_token(&env, &router, &moderator).await;
    for _ in 0..5 {
        let response = mfa(&router, &token, "000000").await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    let late = mfa(&router, &token, &current_code(&env, &moderator, 0)).await;
    assert_eq!(late.status(), StatusCode::UNAUTHORIZED, "challenge used up");

    // A good code works once; the same code on a new challenge is a replay.
    signed_in_staff(&env, &router, &moderator).await;
    let token = mfa_token(&env, &router, &moderator).await;
    let replay = mfa(&router, &token, &current_code(&env, &moderator, 0)).await;
    assert_eq!(replay.status(), StatusCode::UNAUTHORIZED);
    // The next step's code (clock drift allowed) is fresh.
    let next = mfa(&router, &token, &current_code(&env, &moderator, 1)).await;
    assert_eq!(next.status(), StatusCode::OK);

    // A login challenge expires after 5 minutes.
    let token = mfa_token(&env, &router, &moderator).await;
    env.clock.advance(Duration::minutes(6));
    let expired = mfa(&router, &token, &current_code(&env, &moderator, 0)).await;
    assert_eq!(expired.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn staff_sessions_end_after_30_idle_minutes_and_12_hours() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let router = env.router.clone();
    let moderator = staff_member(&env, StaffRole::Moderator).await;
    let session = signed_in_staff(&env, &router, &moderator).await;
    // Activity every 25 minutes keeps it alive...
    for _ in 0..4 {
        env.clock.advance(Duration::minutes(25));
        let me = send(
            &router,
            staff_request("GET", "/v1/admin/me", &session, false),
        )
        .await;
        assert_eq!(me.status(), StatusCode::OK);
    }
    // ...31 idle minutes end it.
    env.clock.advance(Duration::minutes(31));
    let me = send(
        &router,
        staff_request("GET", "/v1/admin/me", &session, false),
    )
    .await;
    assert_eq!(me.status(), StatusCode::UNAUTHORIZED);

    // Absolute limit: 12 hours however active.
    env.clock.advance(Duration::minutes(1));
    let session = signed_in_staff(&env, &router, &moderator).await;
    for _ in 0..30 {
        env.clock.advance(Duration::minutes(25));
        send(
            &router,
            staff_request("GET", "/v1/admin/me", &session, false),
        )
        .await;
    }
    let me = send(
        &router,
        staff_request("GET", "/v1/admin/me", &session, false),
    )
    .await;
    assert_eq!(me.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_ip_allow_list_applies_to_staff_routes() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let mut config = test_config();
    config.staff_ip_allowlist = vec!["203.0.113.7".parse().unwrap()];
    config.client_ip_header = Some(http::HeaderName::from_static("x-test-client-ip"));
    let router = api::build(api::openapi::router(), env.state.clone(), &config);
    let moderator = staff_member(&env, StaffRole::Moderator).await;
    let session = signed_in_staff(&env, &env.router, &moderator).await;
    let mut request = staff_request("GET", "/v1/admin/me", &session, false);
    request
        .headers_mut()
        .insert("x-test-client-ip", "198.51.100.1".parse().unwrap());
    assert_problem(
        send(&router, request).await,
        StatusCode::FORBIDDEN,
        "FORBIDDEN",
    )
    .await;
    let mut request = staff_request("GET", "/v1/admin/me", &session, false);
    request
        .headers_mut()
        .insert("x-test-client-ip", "203.0.113.7".parse().unwrap());
    assert_eq!(send(&router, request).await.status(), StatusCode::OK);
}
