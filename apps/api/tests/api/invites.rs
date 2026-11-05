//! T-37: personal invite links (US-4 AC-4.1, AC-4.3, SR-20) against real Postgres + Redis.

use crate::common::*;
use crate::social_support::*;
use http::StatusCode;
use social::{INVITE_TOKEN_LEN, InviteError};

async fn invite_url(env: &TestEnv, user: &User, method: &str, path: &str) -> String {
    let response = call(env, method, path, user, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    let keys: Vec<&String> = body.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["url"], "nothing but the link is returned");
    body["url"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn invite_link_is_stable_and_has_a_128_bit_token() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let user = adult(&env).await;
    let first = invite_url(&env, &user, "GET", "/v1/me/invite").await;
    let again = invite_url(&env, &user, "GET", "/v1/me/invite").await;
    assert_eq!(first, again, "the same link until reset");
    assert!(first.starts_with(&social_settings().invite_url_base));
    let token = token_of(&first);
    assert_eq!(token.len(), INVITE_TOKEN_LEN);
    // The token resolves to its owner; only its hash and an encrypted copy are stored.
    let resolved = env.state.social.resolve_invite(&token).await.unwrap();
    assert_eq!(resolved.owner_id, user.id);
    let stored: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM social.invites WHERE owner_id = $1 AND (token_enc = convert_to($2, 'UTF8') OR token_hash = convert_to($2, 'UTF8'))",
    )
    .bind(user.id)
    .bind(&token)
    .fetch_one(env.identity.pool())
    .await
    .unwrap();
    assert_eq!(stored, 0, "the raw token is never stored");
}

#[tokio::test]
async fn reset_expires_the_old_token_and_issues_a_new_one() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let user = adult(&env).await;
    let old = token_of(&invite_url(&env, &user, "GET", "/v1/me/invite").await);
    let new = token_of(&invite_url(&env, &user, "POST", "/v1/me/invite/reset").await);
    assert_ne!(old, new);
    assert!(matches!(
        env.state.social.resolve_invite(&old).await,
        Err(InviteError::Expired)
    ));
    assert_eq!(
        env.state
            .social
            .resolve_invite(&new)
            .await
            .unwrap()
            .owner_id,
        user.id
    );
    assert_eq!(
        token_of(&invite_url(&env, &user, "GET", "/v1/me/invite").await),
        new
    );
}

#[tokio::test]
async fn users_only_ever_see_and_reset_their_own_invite() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let alice = adult(&env).await;
    let bob = adult(&env).await;
    let alice_link = invite_url(&env, &alice, "GET", "/v1/me/invite").await;
    let bob_link = invite_url(&env, &bob, "GET", "/v1/me/invite").await;
    assert_ne!(alice_link, bob_link);
    // Bob's reset never touches Alice's invite.
    invite_url(&env, &bob, "POST", "/v1/me/invite/reset").await;
    assert_eq!(
        invite_url(&env, &alice, "GET", "/v1/me/invite").await,
        alice_link
    );
    // No parameter can name another account: a body naming one is ignored (Bob resets his own).
    let response = call(
        &env,
        "POST",
        "/v1/me/invite/reset",
        &bob,
        Some(&serde_json::json!({ "owner_id": alice.id })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        invite_url(&env, &alice, "GET", "/v1/me/invite").await,
        alice_link
    );
}

#[tokio::test]
async fn unknown_and_malformed_tokens_are_not_found() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    for token in [
        "",
        "short",
        "AAAAAAAAAAAAAAAAAAAAAA",
        "../../etc/passwd/aaaaa",
    ] {
        assert!(
            matches!(
                env.state.social.resolve_invite(token).await,
                Err(InviteError::NotFound)
            ),
            "{token}"
        );
    }
}
