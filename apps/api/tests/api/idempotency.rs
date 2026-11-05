//! T-14: Idempotency-Key store (SR-12), against real Postgres (TEST_DATABASE_URL; skipped locally
//! when unset, required in CI). Uses a test-only create route that counts its executions.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use crate::common::*;
use api::access::{
    ApiRouter, Caller, IDEMPOTENT_REPLAYED_HEADER, Idempotency, RoutePolicy, ValidJson,
};
use api::state::AppState;
use axum::Json;
use axum::body::Body;
use http::{Request, StatusCode};
use platform::idempotency::{Begin, IdempotencySettings, IdempotencyStore, StoredResponse};
use serde::Deserialize;
use serde_json::{Value, json};
use time::Duration;
use tokio::sync::Notify;
use utoipa::ToSchema;
use utoipa_axum::routes;
use uuid::Uuid;

/// Executions per account, so a replay is observable.
static EXECUTIONS: LazyLock<Mutex<HashMap<Uuid, u32>>> = LazyLock::new(Default::default);
/// Per-account gates that hold a request inside the handler (concurrency test).
static GATES: LazyLock<Mutex<HashMap<Uuid, Gate>>> = LazyLock::new(Default::default);

/// (entered the handler, may continue).
type Gate = (Arc<Notify>, Arc<Notify>);

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
struct CreateThing {
    name: String,
    #[serde(default)]
    hold: bool,
    #[serde(default)]
    fail: bool,
}

fn gates(account: Uuid) -> (Arc<Notify>, Arc<Notify>) {
    GATES
        .lock()
        .unwrap()
        .entry(account)
        .or_insert_with(|| (Arc::new(Notify::new()), Arc::new(Notify::new())))
        .clone()
}

fn executions(account: Uuid) -> u32 {
    EXECUTIONS
        .lock()
        .unwrap()
        .get(&account)
        .copied()
        .unwrap_or(0)
}

#[utoipa::path(post, path = "/test/things", responses((status = CREATED, description = "created")))]
async fn create_thing(
    caller: Caller,
    body: ValidJson<CreateThing>,
) -> Result<(StatusCode, Json<Value>), StatusCode> {
    let account = caller.0.account_id;
    if body.0.hold {
        let (entered, release) = gates(account);
        entered.notify_one();
        release.notified().await;
    }
    let count = {
        let mut map = EXECUTIONS.lock().unwrap();
        let count = map.entry(account).or_insert(0);
        *count += 1;
        *count
    };
    if body.0.fail {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    Ok((
        StatusCode::CREATED,
        Json(json!({ "account_id": account, "execution": count, "name": body.0.name })),
    ))
}

fn routes() -> ApiRouter<AppState> {
    api::openapi::router().route(
        RoutePolicy::session().idempotency(Idempotency::Required),
        routes!(create_thing),
    )
}

async fn env() -> Option<TestEnv> {
    TestEnv::with_routes(routes()).await
}

/// An active account with a full session.
async fn active_user(env: &TestEnv) -> (Uuid, String) {
    let (account, _) = env.signed_in_user().await;
    sqlx::query("UPDATE identity.accounts SET status = 'active' WHERE id = $1")
        .bind(account)
        .execute(env.identity.pool())
        .await
        .unwrap();
    let session = env.identity.start_session(account, None).await.unwrap();
    (account, session.access_token.expose().to_owned())
}

fn create(token: &str, key: Option<&str>, body: &Value) -> Request<Body> {
    let mut builder = Request::post("/test/things")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"));
    if let Some(key) = key {
        builder = builder.header("idempotency-key", key);
    }
    builder.body(Body::from(body.to_string())).unwrap()
}

#[tokio::test]
async fn replay_returns_the_stored_response_without_running_again() {
    let Some(env) = env().await else {
        return;
    };
    let (account, token) = active_user(&env).await;
    let key = Uuid::now_v7().to_string();
    let body = json!({ "name": "first" });

    let first = send(&env.router, create(&token, Some(&key), &body)).await;
    assert_eq!(first.status(), StatusCode::CREATED);
    assert!(first.headers().get(IDEMPOTENT_REPLAYED_HEADER).is_none());
    let first_body = body_bytes(first).await;

    let replay = send(&env.router, create(&token, Some(&key), &body)).await;
    assert_eq!(replay.status(), StatusCode::CREATED);
    assert_eq!(
        replay.headers().get(IDEMPOTENT_REPLAYED_HEADER).unwrap(),
        "true"
    );
    assert_eq!(
        replay.headers().get("content-type").unwrap(),
        "application/json"
    );
    assert_eq!(body_bytes(replay).await, first_body);
    assert_eq!(executions(account), 1, "the handler ran once");
}

#[tokio::test]
async fn same_key_with_a_different_body_is_rejected() {
    let Some(env) = env().await else {
        return;
    };
    let (account, token) = active_user(&env).await;
    let key = Uuid::now_v7().to_string();
    let first = send(
        &env.router,
        create(&token, Some(&key), &json!({ "name": "a" })),
    )
    .await;
    assert_eq!(first.status(), StatusCode::CREATED);

    let reused = send(
        &env.router,
        create(&token, Some(&key), &json!({ "name": "b" })),
    )
    .await;
    assert_problem(
        reused,
        StatusCode::UNPROCESSABLE_ENTITY,
        "IDEMPOTENCY_KEY_REUSED",
    )
    .await;
    assert_eq!(executions(account), 1);
}

#[tokio::test]
async fn concurrent_submit_with_the_same_key_gets_409() {
    let Some(env) = env().await else {
        return;
    };
    let (account, token) = active_user(&env).await;
    let key = Uuid::now_v7().to_string();
    let body = json!({ "name": "slow", "hold": true });
    let (entered, release) = gates(account);

    let router = env.router.clone();
    let first = {
        let (token, key, body) = (token.clone(), key.clone(), body.clone());
        tokio::spawn(async move { send(&router, create(&token, Some(&key), &body)).await })
    };
    entered.notified().await;

    let second = send(&env.router, create(&token, Some(&key), &body)).await;
    assert_problem(second, StatusCode::CONFLICT, "IDEMPOTENCY_IN_PROGRESS").await;

    release.notify_one();
    let first = first.await.unwrap();
    assert_eq!(first.status(), StatusCode::CREATED);
    // Once finished, the same request replays.
    let third = send(&env.router, create(&token, Some(&key), &body)).await;
    assert_eq!(third.status(), StatusCode::CREATED);
    assert!(third.headers().get(IDEMPOTENT_REPLAYED_HEADER).is_some());
    assert_eq!(executions(account), 1);
}

#[tokio::test]
async fn another_account_reusing_the_key_gets_its_own_execution() {
    let Some(env) = env().await else {
        return;
    };
    let (alice, alice_token) = active_user(&env).await;
    let (bob, bob_token) = active_user(&env).await;
    let key = Uuid::now_v7().to_string();
    let body = json!({ "name": "same" });

    let a = send(&env.router, create(&alice_token, Some(&key), &body)).await;
    assert_eq!(a.status(), StatusCode::CREATED);
    let b = send(&env.router, create(&bob_token, Some(&key), &body)).await;
    assert_eq!(b.status(), StatusCode::CREATED);
    assert!(b.headers().get(IDEMPOTENT_REPLAYED_HEADER).is_none());
    let b = json_body(b).await;
    assert_eq!(b["account_id"], bob.to_string(), "never Alice's response");
    assert_eq!(executions(alice), 1);
    assert_eq!(executions(bob), 1);
}

#[tokio::test]
async fn missing_or_malformed_key_is_400_and_server_errors_free_the_key() {
    let Some(env) = env().await else {
        return;
    };
    let (account, token) = active_user(&env).await;
    let body = json!({ "name": "x" });
    let missing = send(&env.router, create(&token, None, &body)).await;
    assert_problem(missing, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
    let malformed = send(&env.router, create(&token, Some("not-a-uuid"), &body)).await;
    assert_problem(malformed, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
    assert_eq!(executions(account), 0);

    // A 5xx is not stored: the retry runs again.
    let key = Uuid::now_v7().to_string();
    let failing = json!({ "name": "x", "fail": true });
    let first = send(&env.router, create(&token, Some(&key), &failing)).await;
    assert_eq!(first.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let retry = send(&env.router, create(&token, Some(&key), &failing)).await;
    assert_eq!(retry.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(executions(account), 2);
}

#[tokio::test]
async fn unauthenticated_requests_never_reach_the_store() {
    let Some(env) = env().await else {
        return;
    };
    let request = Request::post("/test/things")
        .header("content-type", "application/json")
        .header("idempotency-key", Uuid::now_v7().to_string())
        .body(Body::from(json!({ "name": "x" }).to_string()))
        .unwrap();
    let response = send(&env.router, request).await;
    assert_problem(response, StatusCode::UNAUTHORIZED, "UNAUTHENTICATED").await;
}

fn store(env: &TestEnv) -> IdempotencyStore {
    IdempotencyStore::new(
        env.identity.pool().clone(),
        env.identity.clock(),
        IdempotencySettings::default(),
    )
}

fn stored(status: u16) -> StoredResponse {
    StoredResponse {
        status,
        content_type: Some("application/json".to_owned()),
        body: b"{}".to_vec(),
    }
}

#[tokio::test]
async fn crashed_request_frees_the_key_when_its_lease_ends() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let store = store(&env);
    let (user, key, hash) = (Uuid::now_v7(), Uuid::now_v7(), [7u8; 32]);

    let Begin::Execute(crashed) = store.begin(user, "POST /x", key, &hash).await.unwrap() else {
        panic!("first attempt must execute");
    };
    assert_eq!(
        store.begin(user, "POST /x", key, &hash).await.unwrap(),
        Begin::InProgress
    );
    // A different request with the key is refused even while the first one runs.
    assert_eq!(
        store.begin(user, "POST /x", key, &[8u8; 32]).await.unwrap(),
        Begin::KeyReused
    );

    env.clock
        .advance(IdempotencySettings::default().lease + Duration::seconds(1));
    let Begin::Execute(retry) = store.begin(user, "POST /x", key, &hash).await.unwrap() else {
        panic!("the lease ended: the retry must execute");
    };
    // The crashed attempt lost its claim and cannot overwrite the retry's result.
    assert!(!store.complete(&crashed, &stored(201)).await.unwrap());
    assert!(store.complete(&retry, &stored(201)).await.unwrap());
    assert!(matches!(
        store.begin(user, "POST /x", key, &hash).await.unwrap(),
        Begin::Replay(_)
    ));
}

#[tokio::test]
async fn stored_responses_expire_after_24_hours() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let store = store(&env);
    let (user, key, hash) = (Uuid::now_v7(), Uuid::now_v7(), [1u8; 32]);
    let Begin::Execute(claim) = store.begin(user, "POST /x", key, &hash).await.unwrap() else {
        panic!("first attempt must execute");
    };
    assert!(store.complete(&claim, &stored(201)).await.unwrap());

    env.clock.advance(Duration::hours(23));
    assert_eq!(
        store.begin(user, "POST /x", key, &hash).await.unwrap(),
        Begin::Replay(stored(201))
    );
    env.clock.advance(Duration::hours(2));
    // Expired: the key starts over, even with a different request.
    assert!(matches!(
        store.begin(user, "POST /x", key, &[2u8; 32]).await.unwrap(),
        Begin::Execute(_)
    ));
}
