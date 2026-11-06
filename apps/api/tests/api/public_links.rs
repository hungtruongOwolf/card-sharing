//! Public board links (T-129, US-24 AC-24.1, AC-24.6): owner-only create/list/revoke, the raw
//! token once, hashes at rest, audit, and uniform 404s for collaborators and other owners.

use crate::board_ops::add_collaborator;
use crate::boards::new_board;
use crate::common::*;
use crate::media::without_request_id;
use crate::social_support::*;
use axum::body::Body;
use http::{Request, Response, StatusCode};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use time::Duration;
use uuid::Uuid;

pub async fn create_link(
    env: &TestEnv,
    user: &User,
    board: Uuid,
    expires_in: &str,
    key: Option<Uuid>,
) -> Response<Body> {
    let request = Request::post(format!("/v1/boards/{board}/public-links"))
        .header("authorization", format!("Bearer {}", user.token))
        .header("content-type", "application/json")
        .header(
            "idempotency-key",
            key.unwrap_or_else(Uuid::now_v7).to_string(),
        )
        .body(Body::from(json!({ "expires_in": expires_in }).to_string()))
        .unwrap();
    send(&env.router, request).await
}

/// Creates a link and returns the `link` object of the 201 body.
pub async fn created_link(env: &TestEnv, user: &User, board: Uuid, expires_in: &str) -> Value {
    let response = create_link(env, user, board, expires_in, None).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    json_body(response).await["link"].clone()
}

pub fn token_of(link: &Value) -> String {
    link["url"]
        .as_str()
        .unwrap()
        .rsplit('/')
        .next()
        .unwrap()
        .to_owned()
}

async fn listed(env: &TestEnv, user: &User, board: Uuid) -> Vec<Value> {
    let response = call(
        env,
        "GET",
        &format!("/v1/boards/{board}/public-links"),
        user,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await["items"]
        .as_array()
        .unwrap()
        .clone()
}

async fn audit_count(env: &TestEnv, action: &str, link: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM platform.audit_events WHERE action = $1 AND target = $2",
    )
    .bind(action)
    .bind(format!("public_link:{link}"))
    .fetch_one(env.identity.pool())
    .await
    .unwrap()
}

/// The 404 of a missing board for `user`, request id removed.
async fn missing_board_body(env: &TestEnv, user: &User) -> String {
    let response = create_link(env, user, Uuid::now_v7(), "7d", None).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    without_request_id(body_bytes(response).await)
}

#[tokio::test]
async fn owner_creates_a_link_once_and_only_its_hash_is_stored() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let owner = adult(&env).await;
    let board = new_board(&env, &owner).await;
    let link = created_link(&env, &owner, board, "7d").await;
    let url = link["url"].as_str().unwrap();
    assert!(url.starts_with("http://localhost:3000/b/"), "{url}");
    let token = token_of(&link);
    assert_eq!(token.len(), sharing::TOKEN_LEN);
    assert_eq!(link["board_id"], board.to_string());
    let created = time::OffsetDateTime::parse(
        link["created_at"].as_str().unwrap(),
        &time::format_description::well_known::Rfc3339,
    )
    .unwrap();
    let expires = time::OffsetDateTime::parse(
        link["expires_at"].as_str().unwrap(),
        &time::format_description::well_known::Rfc3339,
    )
    .unwrap();
    assert_eq!(expires - created, Duration::days(7));

    // Only the SHA-256 is stored.
    let id = link["id"].as_str().unwrap();
    let stored: Vec<u8> =
        sqlx::query_scalar("SELECT token_hash FROM sharing.public_links WHERE id = $1::uuid")
            .bind(id)
            .fetch_one(env.identity.pool())
            .await
            .unwrap();
    assert_eq!(stored, Sha256::digest(token.as_bytes()).to_vec());
    assert_eq!(audit_count(&env, "sharing.link_created", id).await, 1);

    // The list never carries the token or the URL.
    let items = listed(&env, &owner, board).await;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], id);
    let text = Value::Array(items).to_string();
    assert!(!text.contains(&token) && !text.contains("url"), "{text}");
}

#[tokio::test]
async fn never_and_24h_expiry_and_expired_links_drop_out_of_the_list() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let owner = adult(&env).await;
    let board = new_board(&env, &owner).await;
    let forever = created_link(&env, &owner, board, "never").await;
    assert!(forever["expires_at"].is_null());
    let day = created_link(&env, &owner, board, "24h").await;
    assert_eq!(listed(&env, &owner, board).await.len(), 2);

    env.clock.advance(Duration::hours(25));
    let owner = signed_in_again(&env, &owner).await;
    let items = listed(&env, &owner, board).await;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], forever["id"]);
    assert_ne!(items[0]["id"], day["id"]);
}

#[tokio::test]
async fn invalid_expiry_is_a_validation_error() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let owner = adult(&env).await;
    let board = new_board(&env, &owner).await;
    for bad in ["30d", "", "NEVER"] {
        let response = create_link(&env, &owner, board, bad, None).await;
        assert_problem(response, StatusCode::BAD_REQUEST, "VALIDATION_FAILED").await;
    }
    let request = Request::post(format!("/v1/boards/{board}/public-links"))
        .header("authorization", format!("Bearer {}", owner.token))
        .header("content-type", "application/json")
        .header("idempotency-key", Uuid::now_v7().to_string())
        .body(Body::from(
            json!({ "expires_in": "7d", "owner_id": owner.id }).to_string(),
        ))
        .unwrap();
    let response = send(&env.router, request).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST, "unknown fields");
}

#[tokio::test]
async fn a_replayed_create_returns_the_same_link() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let owner = adult(&env).await;
    let board = new_board(&env, &owner).await;
    let key = Uuid::now_v7();
    let first = json_body(create_link(&env, &owner, board, "7d", Some(key)).await).await;
    assert_eq!(first["link"]["url_already_shown"], false);
    let token = token_of(&first["link"]);
    let replayed = create_link(&env, &owner, board, "7d", Some(key)).await;
    assert_eq!(replayed.status(), StatusCode::CREATED);
    assert_eq!(
        replayed.headers().get("idempotent-replayed").unwrap(),
        "true"
    );
    let again = json_body(replayed).await;
    // #333: the same link, but its URL is not kept anywhere for the replay.
    assert_eq!(again["link"]["id"], first["link"]["id"]);
    assert_eq!(again["link"]["expires_at"], first["link"]["expires_at"]);
    assert!(again["link"].get("url").is_none(), "{again}");
    assert_eq!(again["link"]["url_already_shown"], true);
    assert_eq!(listed(&env, &owner, board).await.len(), 1);

    // No token material in the idempotency store (raw token, its URL path, or its hash).
    let stored: Vec<Vec<u8>> = sqlx::query_scalar(
        "SELECT response_body FROM platform.idempotency_keys
         WHERE key = $1 AND response_body IS NOT NULL",
    )
    .bind(key)
    .fetch_all(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(stored.len(), 1);
    let stored = String::from_utf8(stored[0].clone()).unwrap();
    let hash: String = Sha256::digest(token.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    for secret in [token.as_str(), "/b/", hash.as_str()] {
        assert!(!stored.contains(secret), "idempotency row holds {secret:?}");
    }
}

#[tokio::test]
async fn a_collaborator_cannot_create_or_list_links() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let owner = adult(&env).await;
    let collaborator = adult(&env).await;
    let board = new_board(&env, &owner).await;
    add_collaborator(&env, board, &collaborator).await;
    created_link(&env, &owner, board, "7d").await;

    let missing = missing_board_body(&env, &collaborator).await;
    let denied = create_link(&env, &collaborator, board, "7d", None).await;
    assert_eq!(denied.status(), StatusCode::NOT_FOUND);
    assert_eq!(without_request_id(body_bytes(denied).await), missing);

    let denied = call(
        &env,
        "GET",
        &format!("/v1/boards/{board}/public-links"),
        &collaborator,
        None,
    )
    .await;
    assert_problem(denied, StatusCode::NOT_FOUND, "NOT_FOUND").await;
    // A stranger, too.
    let stranger = adult(&env).await;
    let denied = create_link(&env, &stranger, board, "7d", None).await;
    assert_eq!(denied.status(), StatusCode::NOT_FOUND);
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM sharing.public_links WHERE board_id = $1")
            .bind(board)
            .fetch_one(env.identity.pool())
            .await
            .unwrap();
    assert_eq!(count, 1, "only the owner's link exists");
}

#[tokio::test]
async fn a_minor_owner_is_refused_by_policy() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let teen = minor(&env).await;
    let board = new_board(&env, &teen).await;
    let response = create_link(&env, &teen, board, "7d", None).await;
    assert_problem(response, StatusCode::FORBIDDEN, "POLICY_DISALLOWED").await;
    // 17 is still under the default public_links.min_age (18).
    let seventeen = user_aged(&env, 17).await;
    let board = new_board(&env, &seventeen).await;
    let response = create_link(&env, &seventeen, board, "never", None).await;
    assert_problem(response, StatusCode::FORBIDDEN, "POLICY_DISALLOWED").await;
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM sharing.public_links WHERE owner_id = ANY($1)")
            .bind(vec![teen.id, seventeen.id])
            .fetch_one(env.identity.pool())
            .await
            .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn owner_revokes_and_others_get_the_missing_404() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let owner = adult(&env).await;
    let other = adult(&env).await;
    let board = new_board(&env, &owner).await;
    let link = created_link(&env, &owner, board, "7d").await;
    let id = link["id"].as_str().unwrap();

    let missing = call(
        &env,
        "DELETE",
        &format!("/v1/public-links/{}", Uuid::now_v7()),
        &other,
        None,
    )
    .await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    let missing = without_request_id(body_bytes(missing).await);
    let denied = call(
        &env,
        "DELETE",
        &format!("/v1/public-links/{id}"),
        &other,
        None,
    )
    .await;
    assert_eq!(denied.status(), StatusCode::NOT_FOUND);
    assert_eq!(without_request_id(body_bytes(denied).await), missing);
    assert_eq!(listed(&env, &owner, board).await.len(), 1, "still live");

    let revoked = call(
        &env,
        "DELETE",
        &format!("/v1/public-links/{id}"),
        &owner,
        None,
    )
    .await;
    assert_eq!(revoked.status(), StatusCode::NO_CONTENT);
    assert!(listed(&env, &owner, board).await.is_empty());
    let (reason, at): (Option<String>, Option<time::OffsetDateTime>) = sqlx::query_as(
        "SELECT revoked_reason, revoked_at FROM sharing.public_links WHERE id = $1::uuid",
    )
    .bind(id)
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(reason.as_deref(), Some("owner"));
    assert!(at.is_some());
    assert_eq!(audit_count(&env, "sharing.link_revoked", id).await, 1);

    // Revoking again is a success without a second audit entry.
    let again = call(
        &env,
        "DELETE",
        &format!("/v1/public-links/{id}"),
        &owner,
        None,
    )
    .await;
    assert_eq!(again.status(), StatusCode::NO_CONTENT);
    assert_eq!(audit_count(&env, "sharing.link_revoked", id).await, 1);
    // A malformed id is just a missing one.
    let malformed = call(&env, "DELETE", "/v1/public-links/not-a-uuid", &owner, None).await;
    assert_problem(malformed, StatusCode::NOT_FOUND, "NOT_FOUND").await;
}

#[tokio::test]
async fn a_deleted_board_has_no_links_to_list_or_create() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let owner = adult(&env).await;
    let board = new_board(&env, &owner).await;
    created_link(&env, &owner, board, "7d").await;
    let deleted = call(&env, "DELETE", &format!("/v1/boards/{board}"), &owner, None).await;
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
    let response = create_link(&env, &owner, board, "7d", None).await;
    assert_problem(response, StatusCode::NOT_FOUND, "NOT_FOUND").await;
    let response = call(
        &env,
        "GET",
        &format!("/v1/boards/{board}/public-links"),
        &owner,
        None,
    )
    .await;
    assert_problem(response, StatusCode::NOT_FOUND, "NOT_FOUND").await;
}
