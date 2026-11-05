//! T-12: authorization-matrix harness (SR-5). Loads `tests/authz/matrix.yaml` and runs every
//! (route, role) cell against the real API with fresh fixture users. Fails on a registered route
//! without a row (coverage report), a row for an unknown route, or any cell mismatch.

use std::collections::{BTreeMap, BTreeSet};

use crate::common::*;
use axum::body::Body;
use http::{Method, Request, StatusCode};
use identity::testing::random_nonce;
use platform::clock::Clock;
use serde::Deserialize;
use serde_json::Value;
use time::{Date, Month};
use uuid::Uuid;

const MATRIX: &str = include_str!("authz/matrix.yaml");

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Matrix {
    version: u32,
    routes: Vec<Row>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Row {
    route: String,
    #[serde(default)]
    body: Option<Value>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    missing_path: Option<String>,
    expect: BTreeMap<Role, u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Role {
    Guest,
    OnboardingAge,
    OnboardingProfile,
    User,
    Suspended,
}

const ALL_ROLES: [Role; 5] = [
    Role::Guest,
    Role::OnboardingAge,
    Role::OnboardingProfile,
    Role::User,
    Role::Suspended,
];

struct Fixture {
    access_token: Option<String>,
    refresh_token: String,
    session_id: Uuid,
}

fn matrix() -> Matrix {
    let matrix: Matrix = serde_norway::from_str(MATRIX).unwrap();
    assert_eq!(matrix.version, 1);
    matrix
}

fn split_route(route: &str) -> (Method, &str) {
    let (method, path) = route.split_once(' ').expect("route is `METHOD /path`");
    (method.parse().unwrap(), path)
}

/// Every route the app serves: the OpenAPI operations plus undocumented policy entries.
fn registered_routes() -> BTreeSet<String> {
    let (_, _, policies) = api::openapi::router().finish();
    policies
        .iter()
        .map(|(method, path, _)| format!("{method} {path}"))
        .collect()
}

#[test]
fn matrix_covers_every_registered_route() {
    let registered = registered_routes();
    let rows: BTreeSet<String> = matrix().routes.iter().map(|r| r.route.clone()).collect();
    let missing: Vec<&String> = registered.difference(&rows).collect();
    let unknown: Vec<&String> = rows.difference(&registered).collect();
    if !missing.is_empty() || !unknown.is_empty() {
        panic!(
            "authorization matrix coverage report\n  routes without matrix rows: {missing:?}\n  rows for unknown routes: {unknown:?}\nAdd a row to apps/api/tests/authz/matrix.yaml for every new route."
        );
    }
    for row in matrix().routes {
        for role in ALL_ROLES {
            assert!(
                row.expect.contains_key(&role),
                "{} lacks a cell for {role:?}",
                row.route
            );
        }
        let denies_with_404 = row.expect.values().any(|status| *status == 404);
        assert!(
            !denies_with_404 || row.missing_path.is_some(),
            "{} expects 404 but has no missing_path to compare against",
            row.route
        );
    }
}

async fn fixture(env: &TestEnv, role: Role) -> Fixture {
    if role == Role::Guest {
        return Fixture {
            access_token: None,
            refresh_token: format!("unknown-{}", Uuid::now_v7()),
            session_id: Uuid::now_v7(),
        };
    }
    let (account, _) = env.signed_in_user().await;
    let status = match role {
        Role::OnboardingAge => "pending_age",
        Role::OnboardingProfile => "pending_profile",
        Role::User => "active",
        Role::Suspended => "suspended",
        Role::Guest => unreachable!(),
    };
    sqlx::query("UPDATE identity.accounts SET status = $2 WHERE id = $1")
        .bind(account)
        .bind(status)
        .execute(env.identity.pool())
        .await
        .unwrap();
    // A session started after the status change carries the matching scope.
    let session = env.identity.start_session(account, None).await.unwrap();
    Fixture {
        access_token: Some(session.access_token.expose().to_owned()),
        refresh_token: session.refresh_token.expose().to_owned(),
        session_id: session.session_id,
    }
}

/// Values for the body placeholders of one cell.
struct Placeholders {
    refresh_token: String,
    adult_dob: String,
    nonce: String,
    apple_id_token: String,
    google_id_token: String,
    handle: String,
    email: String,
    code_challenge: String,
}

impl Placeholders {
    /// Fresh per cell: a new provider subject and nonce, so sign-in cells never collide.
    fn new(env: &TestEnv, fixture: &Fixture, adult_dob: &str) -> Self {
        let nonce = random_nonce();
        let subject = format!("matrix-{}", Uuid::now_v7());
        Self {
            refresh_token: fixture.refresh_token.clone(),
            adult_dob: adult_dob.to_owned(),
            apple_id_token: env.apple_id_token(&subject, &nonce),
            google_id_token: env.google_id_token(&subject, &nonce),
            // The random tail of a UUIDv7 (the leading bits are a timestamp).
            handle: format!("mx{}", &Uuid::now_v7().simple().to_string()[20..]),
            email: format!("matrix-{}@example.com", Uuid::now_v7().simple()),
            code_challenge: base64::Engine::encode(
                &base64::engine::general_purpose::URL_SAFE_NO_PAD,
                Uuid::now_v7().as_bytes().repeat(2),
            ),
            nonce,
        }
    }
}

fn substitute(value: &Value, values: &Placeholders) -> Value {
    match value {
        Value::String(s) if s == "$REFRESH_TOKEN" => Value::String(values.refresh_token.clone()),
        Value::String(s) if s == "$ADULT_DOB" => Value::String(values.adult_dob.clone()),
        Value::String(s) if s == "$NONCE" => Value::String(values.nonce.clone()),
        Value::String(s) if s == "$APPLE_ID_TOKEN" => Value::String(values.apple_id_token.clone()),
        Value::String(s) if s == "$HANDLE" => Value::String(values.handle.clone()),
        Value::String(s) if s == "$EMAIL" => Value::String(values.email.clone()),
        Value::String(s) if s == "$CODE_CHALLENGE" => Value::String(values.code_challenge.clone()),
        Value::String(s) if s == "$GOOGLE_ID_TOKEN" => {
            Value::String(values.google_id_token.clone())
        }
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), substitute(v, values)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn request(
    method: &Method,
    path: &str,
    fixture: &Fixture,
    body: Option<&Value>,
    request_id: Uuid,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("x-request-id", request_id.to_string());
    if let Some(token) = &fixture.access_token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    match body {
        Some(body) => builder
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    }
}

#[tokio::test]
async fn every_matrix_cell_holds_against_the_real_api() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let today = env.clock.now().date();
    let adult_dob = Date::from_calendar_date(today.year() - 30, Month::January, 1)
        .unwrap()
        .to_string();

    let mut failures = Vec::new();
    for row in matrix().routes {
        let (method, template) = split_route(&row.route);
        for (role, expected) in &row.expect {
            let fixture = fixture(&env, *role).await;
            let path = row
                .path
                .as_deref()
                .unwrap_or(template)
                .replace("$SESSION_ID", &fixture.session_id.hyphenated().to_string());
            let path = path.as_str();
            let placeholders = Placeholders::new(&env, &fixture, &adult_dob);
            let body = row.body.as_ref().map(|b| substitute(b, &placeholders));
            let request_id = Uuid::now_v7();
            let response = send(
                &env.router,
                request(&method, path, &fixture, body.as_ref(), request_id),
            )
            .await;
            let status = response.status().as_u16();
            if status != *expected {
                failures.push(format!(
                    "{} as {role:?}: expected {expected}, got {status}",
                    row.route
                ));
                continue;
            }
            if status == StatusCode::NOT_FOUND.as_u16() {
                let denied = body_bytes(response).await;
                let missing_path = row.missing_path.as_deref().unwrap();
                let missing = send(
                    &env.router,
                    request(&method, missing_path, &fixture, body.as_ref(), request_id),
                )
                .await;
                if body_bytes(missing).await != denied {
                    failures.push(format!(
                        "{} as {role:?}: 404 body differs from a missing id (enumeration leak)",
                        row.route
                    ));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "authorization matrix failures:\n{}",
        failures.join("\n")
    );
}
