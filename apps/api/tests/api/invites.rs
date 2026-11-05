//! T-37: personal invite links (US-4 AC-4.1, AC-4.3, SR-20) against real Postgres + Redis.

use crate::common::*;
use crate::social_support::*;
use http::StatusCode;
use platform::clock::Clock;
use serde_json::Value;
use social::{INVITE_TOKEN_LEN, InviteError};
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

async fn invite_body(env: &TestEnv, user: &User, method: &str, path: &str) -> Value {
    let response = call(env, method, path, user, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    let keys: Vec<&String> = body.as_object().unwrap().keys().collect();
    assert_eq!(
        keys,
        ["expires_at", "url"],
        "nothing but the link is returned"
    );
    body
}

async fn invite_url(env: &TestEnv, user: &User, method: &str, path: &str) -> String {
    invite_body(env, user, method, path).await["url"]
        .as_str()
        .unwrap()
        .to_owned()
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

async fn set_use_count(env: &TestEnv, owner: &User, count: i32) {
    sqlx::query(
        "UPDATE social.invites SET use_count = $2 WHERE owner_id = $1 AND revoked_at IS NULL",
    )
    .bind(owner.id)
    .bind(count)
    .execute(env.identity.pool())
    .await
    .unwrap();
}

#[tokio::test]
async fn a_minors_invite_expires_after_seven_days() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let teen = minor(&env).await;
    let body = invite_body(&env, &teen, "GET", "/v1/me/invite").await;
    let expires_at = OffsetDateTime::parse(body["expires_at"].as_str().unwrap(), &Rfc3339).unwrap();
    let issued = env.clock.now();
    assert!((expires_at - (issued + Duration::days(7))).abs() < Duration::seconds(1));
    let token = token_of(body["url"].as_str().unwrap());

    env.clock.advance(Duration::days(7) - Duration::seconds(5));
    assert!(env.state.social.resolve_invite(&token).await.is_ok());
    env.clock.advance(Duration::seconds(10));
    assert!(matches!(
        env.state.social.resolve_invite(&token).await,
        Err(InviteError::Expired)
    ));
    // The owner gets a fresh link instead of the dead one.
    let fresh = token_of(&invite_url(&env, &teen, "GET", "/v1/me/invite").await);
    assert_ne!(fresh, token);
    assert!(env.state.social.resolve_invite(&fresh).await.is_ok());
    assert!(matches!(
        env.state.social.resolve_invite(&token).await,
        Err(InviteError::Expired)
    ));
}

#[tokio::test]
async fn a_minors_invite_expires_at_the_use_cap() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let cap = i32::try_from(
        policy_service()
            .resolve(&Default::default())
            .invites
            .minor_max_uses,
    )
    .unwrap();
    let teen = minor(&env).await;
    let token = token_of(&invite_url(&env, &teen, "GET", "/v1/me/invite").await);
    set_use_count(&env, &teen, cap - 1).await;
    let resolved = env.state.social.resolve_invite(&token).await.unwrap();
    assert_eq!(resolved.max_uses, Some(cap));
    set_use_count(&env, &teen, cap).await;
    assert!(matches!(
        env.state.social.resolve_invite(&token).await,
        Err(InviteError::Expired)
    ));
}

#[tokio::test]
async fn adult_invites_have_no_expiry_or_use_cap() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let grown_up = adult(&env).await;
    let body = invite_body(&env, &grown_up, "GET", "/v1/me/invite").await;
    assert!(body["expires_at"].is_null());
    let token = token_of(body["url"].as_str().unwrap());
    set_use_count(&env, &grown_up, 10_000).await;
    env.clock.advance(Duration::days(400));
    let resolved = env.state.social.resolve_invite(&token).await.unwrap();
    assert_eq!(resolved.max_uses, None);
}

#[tokio::test]
async fn the_minor_rule_applies_to_an_invite_issued_before_the_owner_became_a_minor() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let owner = adult(&env).await;
    let token = token_of(&invite_url(&env, &owner, "GET", "/v1/me/invite").await);
    // A later OS signal says minor (it can only tighten): the invite now follows the minor rule.
    env.state
        .age
        .record_os_signal(
            owner.id,
            identity::OsAgeSource::AppleDeclaredAgeRange,
            identity::OsAgeSignal::Minor,
        )
        .await
        .unwrap();
    env.clock.advance(Duration::days(8));
    assert!(matches!(
        env.state.social.resolve_invite(&token).await,
        Err(InviteError::Expired)
    ));
}
