//! T-3: `api seed` creates two adults, one minor, friendships, delivered cards and a shared board
//! through the production migrations and routes, and running it again changes nothing.

use crate::common::*;
use crate::media_gate::fake_gates;
use http::StatusCode;
use serde_json::Value;
use uuid::Uuid;

async fn get(env: &TestEnv, path: &str, token: &str) -> Value {
    let response = send(&env.router, get_request(path, Some(token))).await;
    assert_eq!(response.status(), StatusCode::OK, "GET {path}");
    json_body(response).await
}

#[tokio::test]
async fn seed_creates_demo_data_and_is_idempotent() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let first = api::seed::run(&env.router, &env.state, &fake_gates())
        .await
        .unwrap();
    let second = api::seed::run(&env.router, &env.state, &fake_gates())
        .await
        .unwrap();
    assert_eq!(second.cards_created, 0, "a second run creates no cards");
    assert_eq!(first.board_id, second.board_id);
    let ids = |r: &api::seed::SeedReport| r.users.iter().map(|u| u.id).collect::<Vec<Uuid>>();
    assert_eq!(ids(&first), ids(&second));

    let token = |i: usize| second.users[i].session.access_token.expose().to_owned();
    let (alice, bob, mia) = (token(0), token(1), token(2));

    // Profiles are complete: every seed user is active with the persona's handle.
    for (i, persona) in api::seed::PERSONAS.iter().enumerate() {
        let me = get(&env, "/v1/me", &token(i)).await;
        assert_eq!(me["account"]["status"], "active", "{}", persona.key);
        assert_eq!(me["profile"]["handle"], persona.handle);
    }
    let minors: Vec<bool> = second.users.iter().map(|u| u.persona.age < 18).collect();
    assert_eq!(minors, vec![false, false, true], "two adults and one minor");

    // Friendships: Alice with Bob and with Mia; Bob and Mia are not friends.
    let friends = |body: Value| -> Vec<String> {
        body["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| {
                f["user"]["handle"]
                    .as_str()
                    .or(f["handle"].as_str())
                    .unwrap()
                    .to_owned()
            })
            .collect()
    };
    let mut alice_friends = friends(get(&env, "/v1/friends", &alice).await);
    alice_friends.sort();
    assert_eq!(alice_friends, vec!["bob.demo", "mia.demo"]);
    assert_eq!(
        friends(get(&env, "/v1/friends", &bob).await),
        vec!["alice.demo"]
    );

    // Delivered cards: Bob got two from Alice, Mia one, Alice one from Bob.
    let feed_len = |body: Value| body["items"].as_array().unwrap().len();
    assert_eq!(feed_len(get(&env, "/v1/feed", &bob).await), 2);
    assert_eq!(feed_len(get(&env, "/v1/feed", &mia).await), 1);
    assert_eq!(feed_len(get(&env, "/v1/feed", &alice).await), 1);

    // The shared board: Bob is a member and sees the three pins.
    let board = get(&env, &format!("/v1/boards/{}", second.board_id), &bob).await;
    assert_eq!(board["pins"].as_array().unwrap().len(), 3, "{board}");
    // Mia was never invited: the board is hidden from her like a missing one.
    let response = send(
        &env.router,
        get_request(&format!("/v1/boards/{}", second.board_id), Some(&mia)),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
