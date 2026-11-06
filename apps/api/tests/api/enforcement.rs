//! T-147: suspensions and bans apply everywhere (US-27, SR-2): every session and widget token
//! stops with 403 `ACCOUNT_SUSPENDED`, the account's cards and public links disappear for others
//! at once, sign-in is refused, and a temporary suspension ends on its own.

use crate::boards::new_board;
use crate::cards_read::delivered_card;
use crate::common::*;
use crate::public_links::{created_link, token_of};
use crate::social_support::*;
use axum::body::Body;
use http::{Request, StatusCode};
use identity::testing::random_nonce;
use identity::{Provider, VerifiedIdentity};
use platform::clock::Clock;
use serde_json::{Value, json};
use time::Duration;
use uuid::Uuid;

async fn widget_token(env: &TestEnv, user: &User) -> String {
    let response = call(env, "POST", "/v1/me/widget-token", user, None).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    json_body(response).await["token"]
        .as_str()
        .unwrap()
        .to_owned()
}

async fn feed_ids(env: &TestEnv, user: &User) -> Vec<Value> {
    let response = send(&env.router, get_request("/v1/feed", Some(&user.token))).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].clone())
        .collect()
}

fn public_page(token: &str) -> Request<Body> {
    let bytes = Uuid::now_v7().into_bytes();
    Request::get(format!("/v1/public/boards/{token}"))
        .header("x-service-credential", TEST_SERVICE_CREDENTIAL)
        .header(
            "x-client-ip",
            format!("10.{}.{}.{}", bytes[13], bytes[14], bytes[15]),
        )
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn a_suspended_account_is_refused_everywhere_and_its_content_disappears() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let delivered = delivered_card(&env).await;
    let owner = &delivered.owner;
    let widget = widget_token(&env, owner).await;
    assert!(
        feed_ids(&env, &delivered.friend)
            .await
            .contains(&json!(delivered.card.to_string()))
    );

    env.identity
        .suspend_account_until(
            owner.id,
            "moderation",
            Some(env.clock.now() + Duration::days(3)),
        )
        .await
        .unwrap();

    // The next request with any of its tokens: 403 ACCOUNT_SUSPENDED (neutral, not "signed out").
    let me = send(&env.router, get_request("/v1/me", Some(&owner.token))).await;
    assert_problem(me, StatusCode::FORBIDDEN, "ACCOUNT_SUSPENDED").await;
    let widget = send(&env.router, get_request("/v1/widget", Some(&widget))).await;
    assert_problem(widget, StatusCode::FORBIDDEN, "ACCOUNT_SUSPENDED").await;

    // Their card is gone for the friend at once.
    assert!(
        !feed_ids(&env, &delivered.friend)
            .await
            .contains(&json!(delivered.card.to_string()))
    );
    let card = send(
        &env.router,
        get_request(
            &format!("/v1/cards/{}", delivered.card),
            Some(&delivered.friend.token),
        ),
    )
    .await;
    assert_eq!(card.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn public_links_of_a_suspended_owner_stop_serving() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let owner = adult(&env).await;
    let board = new_board(&env, &owner).await;
    let link = created_link(&env, &owner, board, "7d").await;
    let token = token_of(&link);
    env.identity
        .suspend_account(owner.id, "moderation")
        .await
        .unwrap();
    let page = send(&env.router, public_page(&token)).await;
    assert_eq!(page.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn sign_in_is_refused_while_suspended_and_works_again_after_the_end() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let subject = format!("enf-{}", Uuid::now_v7().simple());
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
    env.identity
        .suspend_account_until(
            account,
            "moderation",
            Some(env.clock.now() + Duration::hours(1)),
        )
        .await
        .unwrap();

    let sign_in = |env: &TestEnv| {
        let nonce = random_nonce();
        post_json(
            "/v1/auth/apple",
            None,
            &json!({ "id_token": env.apple_id_token(&subject, &nonce), "nonce": nonce }),
        )
    };
    let refused = send(&env.router, sign_in(&env)).await;
    assert_problem(refused, StatusCode::FORBIDDEN, "ACCOUNT_SUSPENDED").await;

    // Not yet over: nothing is lifted.
    assert_eq!(env.identity.lift_expired_suspensions().await.unwrap(), 0);
    env.clock.advance(Duration::hours(2));
    assert!(env.identity.lift_expired_suspensions().await.unwrap() >= 1);
    let status: String = sqlx::query_scalar("SELECT status FROM identity.accounts WHERE id = $1")
        .bind(account)
        .fetch_one(env.identity.pool())
        .await
        .unwrap();
    assert_eq!(status, "active");
    let again = send(&env.router, sign_in(&env)).await;
    assert_eq!(again.status(), StatusCode::OK);
}

#[tokio::test]
async fn an_indefinite_suspension_and_a_ban_are_never_lifted() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let suspended = adult(&env).await;
    let banned = adult(&env).await;
    env.identity
        .suspend_account(suspended.id, "csam_match")
        .await
        .unwrap();
    env.identity.ban_account(banned.id).await.unwrap();
    env.clock.advance(Duration::days(400));
    env.identity.lift_expired_suspensions().await.unwrap();
    for (user, expected) in [(&suspended, "suspended"), (&banned, "banned")] {
        let status: String =
            sqlx::query_scalar("SELECT status FROM identity.accounts WHERE id = $1")
                .bind(user.id)
                .fetch_one(env.identity.pool())
                .await
                .unwrap();
        assert_eq!(status, expected);
    }
    // A banned account's old token answers the same neutral 403.
    let me = send(&env.router, get_request("/v1/me", Some(&banned.token))).await;
    assert_problem(me, StatusCode::FORBIDDEN, "ACCOUNT_SUSPENDED").await;
}
