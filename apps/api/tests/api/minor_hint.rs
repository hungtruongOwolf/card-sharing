//! T-42: the "adult · no mutual friends" hint on a minor's incoming requests (US-5 AC-5.5, SR-20)
//! against real Postgres + Redis.

use crate::common::*;
use crate::social_support::*;
use http::StatusCode;
use serde_json::Value;

/// `sender` asks `recipient` through an invite; returns the recipient's incoming item.
async fn incoming_item(env: &TestEnv, sender: &User, recipient: &User) -> Value {
    let token = invite_token(env, recipient).await;
    assert_eq!(
        request_via_invite(env, sender, &token).await.status(),
        StatusCode::CREATED
    );
    let response = call(
        env,
        "GET",
        "/v1/friend-requests?direction=incoming",
        recipient,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let items = json_body(response).await["items"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(items.len(), 1);
    items[0].clone()
}

#[tokio::test]
async fn hint_truth_table() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    // (recipient is minor, sender is adult, mutual friend) -> hint expected
    let cases = [
        (true, true, false, true),
        (true, true, true, false),
        (true, false, false, false),
        (true, false, true, false),
        (false, true, false, false),
        (false, true, true, false),
        (false, false, false, false),
        (false, false, true, false),
    ];
    for (recipient_minor, sender_adult, mutual, expected) in cases {
        let recipient = if recipient_minor {
            minor(&env).await
        } else {
            adult(&env).await
        };
        let sender = if sender_adult {
            adult(&env).await
        } else {
            minor(&env).await
        };
        if mutual {
            let common = adult(&env).await;
            make_friends(&env, recipient.id, common.id).await;
            make_friends(&env, sender.id, common.id).await;
        }
        let item = incoming_item(&env, &sender, &recipient).await;
        let label = format!(
            "recipient_minor={recipient_minor} sender_adult={sender_adult} mutual={mutual}"
        );
        if expected {
            assert_eq!(item["hint"], "adult_no_mutual_friends", "{label}");
        } else {
            assert!(item.get("hint").is_none(), "{label}: {item}");
        }
    }
}

#[tokio::test]
async fn adults_never_see_a_hint_and_outgoing_lists_carry_none() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let teen = minor(&env).await;
    let grown_up = adult(&env).await;
    // A minor asking an adult: the adult learns nothing about the sender's age.
    let item = incoming_item(&env, &teen, &grown_up).await;
    assert!(item.get("hint").is_none());
    let keys: Vec<&String> = item.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["created_at", "expires_at", "id", "user"]);
    // The sender's own outgoing list has no hint either.
    let response = call(
        &env,
        "GET",
        "/v1/friend-requests?direction=outgoing",
        &teen,
        None,
    )
    .await;
    let items = json_body(response).await["items"]
        .as_array()
        .unwrap()
        .clone();
    assert!(items.iter().all(|item| item.get("hint").is_none()));
}
