//! Seeded randomized permission tests of boards against the real API (T-110, SR-5, SR-6, SR-7,
//! SR-21). Each world has random people (adults and minors), friendships, members, cards with
//! random recipients, a random op sequence by members and non-members, then random unfriends and
//! blocks. A model written from the spec predicts every op result and every pin each person may
//! see; the suite asserts that:
//! - only members can call ops, only permitted cards are pinned, only the pin's creator or the
//!   owner moves/removes a pin (SR-7);
//! - no card, caption or media URL ever reaches a viewer the spec does not entitle (SR-6, SR-21).
//!
//! Reproduce a failure with `BOARD_PROPERTY_SEED=<seed printed in the message>`;
//! `BOARD_PROPERTY_WORLDS` sets how many worlds run (default 4).

use std::collections::{HashMap, HashSet};

use crate::board_ops::{add_collaborator, post_ops};
use crate::boards::new_board;
use crate::cards_read::deliver_to;
use crate::common::*;
use crate::social_support::*;
use http::StatusCode;
use serde_json::json;
use uuid::Uuid;

/// SplitMix64 (no external crate needed).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn pair(a: Uuid, b: Uuid) -> (Uuid, Uuid) {
    if a < b { (a, b) } else { (b, a) }
}

struct ModelPin {
    card: Uuid,
    creator: Uuid,
    version: i64,
}

struct Model {
    people: Vec<User>,
    minors: HashSet<Uuid>,
    owner: Uuid,
    members: HashSet<Uuid>,
    friends: HashSet<(Uuid, Uuid)>,
    blocks: HashSet<(Uuid, Uuid)>,
    /// card -> (owner, recipients)
    cards: HashMap<Uuid, (Uuid, HashSet<Uuid>)>,
    pins: HashMap<Uuid, ModelPin>,
}

impl Model {
    fn friends_unblocked(&self, a: Uuid, b: Uuid) -> bool {
        self.friends.contains(&pair(a, b)) && !self.blocks.contains(&pair(a, b))
    }

    /// The card-read predicate: owner, or a recipient still friends with the owner, no block.
    fn may_read(&self, viewer: Uuid, card: Uuid) -> bool {
        self.cards.get(&card).is_some_and(|(owner, recipients)| {
            *owner == viewer
                || (recipients.contains(&viewer) && self.friends_unblocked(viewer, *owner))
        })
    }

    /// AC-19.9: an adult viewer, `person` a minor who is not their friend.
    fn restricted(&self, viewer: Uuid, person: Uuid) -> bool {
        !self.minors.contains(&viewer)
            && person != viewer
            && self.minors.contains(&person)
            && !self.friends_unblocked(viewer, person)
    }

    fn sees(&self, viewer: Uuid, pin: &ModelPin) -> bool {
        let owner = self.cards.get(&pin.card).map(|c| c.0);
        self.may_read(viewer, pin.card)
            && !self.restricted(viewer, pin.creator)
            && owner.is_some_and(|o| !self.restricted(viewer, o))
    }

    /// The pin-add rule (T-100).
    fn may_pin(&self, actor: Uuid, card: Uuid) -> bool {
        match self.cards.get(&card) {
            Some((owner, _)) if *owner == actor => true,
            Some(_) => actor == self.owner && self.may_read(actor, card),
            None => false,
        }
    }
}

async fn build_world(env: &TestEnv, rng: &mut Rng) -> (Model, Uuid) {
    let count = 4 + rng.below(2);
    let mut people = Vec::new();
    let mut minors = HashSet::new();
    for _ in 0..count {
        let person = if rng.chance(30) {
            let p = minor(env).await;
            minors.insert(p.id);
            p
        } else {
            adult(env).await
        };
        people.push(person);
    }
    let mut friends = HashSet::new();
    for i in 0..count {
        for j in i + 1..count {
            if rng.chance(60) {
                make_friends(env, people[i].id, people[j].id).await;
                friends.insert(pair(people[i].id, people[j].id));
            }
        }
    }
    let owner = people[0].clone();
    let board = new_board(env, &owner).await;
    let mut members = HashSet::from([owner.id]);
    for person in &people[1..] {
        if rng.chance(65) {
            add_collaborator(env, board, person).await;
            members.insert(person.id);
        }
    }
    let mut model = Model {
        people,
        minors,
        owner: owner.id,
        members,
        friends,
        blocks: HashSet::new(),
        cards: HashMap::new(),
        pins: HashMap::new(),
    };
    // Up to three cards, each from someone with friends, to a random non-empty subset of them.
    for _ in 0..3 {
        let sender = model.people[rng.below(count)].clone();
        let their_friends: Vec<User> = model
            .people
            .iter()
            .filter(|p| model.friends.contains(&pair(sender.id, p.id)))
            .cloned()
            .collect();
        if their_friends.is_empty() {
            continue;
        }
        let mut recipients: Vec<&User> = their_friends.iter().filter(|_| rng.chance(60)).collect();
        if recipients.is_empty() {
            recipients.push(&their_friends[rng.below(their_friends.len())]);
        }
        let card = deliver_to(env, &sender, &recipients).await;
        model
            .cards
            .insert(card, (sender.id, recipients.iter().map(|u| u.id).collect()));
    }
    (model, board)
}

/// Runs random ops and checks every result against the model.
async fn random_ops(env: &TestEnv, rng: &mut Rng, model: &mut Model, board: Uuid, seed: u64) {
    let cards: Vec<Uuid> = model.cards.keys().copied().collect();
    for step in 0..14 {
        let actor = model.people[rng.below(model.people.len())].clone();
        let pins: Vec<Uuid> = model.pins.keys().copied().collect();
        let op_id = Uuid::now_v7();
        let (op, kind, target) = if pins.is_empty() || rng.chance(50) {
            let card = if cards.is_empty() || rng.chance(10) {
                Uuid::now_v7()
            } else {
                cards[rng.below(cards.len())]
            };
            let pin = Uuid::now_v7();
            let op = json!({ "type": "pin.add", "op_id": op_id, "pin": {
                "id": pin, "card_id": card, "x": 0.5, "y": 0.5, "scale": 1, "rotation": 0, "z_key": "m" } });
            (op, "add", (pin, card))
        } else {
            let pin = pins[rng.below(pins.len())];
            let current = model.pins[&pin].version;
            let base = if rng.chance(80) { current } else { current + 1 };
            let op = if rng.chance(50) {
                json!({ "type": "pin.update", "op_id": op_id, "pin_id": pin, "base_version": base, "set": { "x": 0.25 } })
            } else {
                json!({ "type": "pin.remove", "op_id": op_id, "pin_id": pin, "base_version": base })
            };
            let kind = if op["type"] == "pin.update" {
                "update"
            } else {
                "remove"
            };
            (op, kind, (pin, Uuid::nil()))
        };
        let context = format!("seed {seed}, step {step}: {kind} by {} ({op})", actor.id);
        let response = post_ops(env, &actor, board, json!([op.clone()])).await;
        if !model.members.contains(&actor.id) {
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{context}");
            continue;
        }
        assert_eq!(response.status(), StatusCode::OK, "{context}");
        let result = json_body(response).await["results"][0].clone();
        let (pin, card) = target;
        match kind {
            "add" => {
                if model.may_pin(actor.id, card) {
                    assert_eq!(result["status"], "applied", "{context}: {result}");
                    model.pins.insert(
                        pin,
                        ModelPin {
                            card,
                            creator: actor.id,
                            version: 1,
                        },
                    );
                } else {
                    assert_eq!(result["code"], "NOT_FOUND", "{context}: {result}");
                }
            }
            _ => {
                let entry = &model.pins[&pin];
                let allowed = actor.id == model.owner || actor.id == entry.creator;
                let base = op["base_version"].as_i64().unwrap();
                if !allowed {
                    assert_eq!(result["code"], "PIN_NOT_YOURS", "{context}: {result}");
                    assert!(result.get("current").is_none(), "{context}");
                } else if base != entry.version {
                    assert_eq!(result["status"], "conflict", "{context}: {result}");
                } else {
                    assert_eq!(result["status"], "applied", "{context}: {result}");
                    if kind == "remove" {
                        model.pins.remove(&pin);
                    } else if let Some(entry) = model.pins.get_mut(&pin) {
                        entry.version += 1;
                    }
                }
            }
        }
    }
}

/// Every person reads the board; nothing the model forbids may appear.
async fn check_reads(env: &TestEnv, model: &Model, board: Uuid, seed: u64, phase: &str) {
    for viewer in &model.people {
        let response = call(env, "GET", &format!("/v1/boards/{board}"), viewer, None).await;
        if !model.members.contains(&viewer.id) {
            assert_eq!(
                response.status(),
                StatusCode::NOT_FOUND,
                "seed {seed} {phase}"
            );
            continue;
        }
        assert_eq!(response.status(), StatusCode::OK, "seed {seed} {phase}");
        let body = json_body(response).await;
        let text = body.to_string();
        let items = body["pins"].as_array().unwrap();
        assert_eq!(items.len(), model.pins.len(), "seed {seed} {phase}: {body}");
        let mut visible_cards = HashSet::new();
        for item in items {
            let id = Uuid::parse_str(item["id"].as_str().unwrap()).unwrap();
            let pin = &model.pins[&id];
            let context = format!("seed {seed} {phase}: viewer {} pin {id}", viewer.id);
            if model.sees(viewer.id, pin) {
                assert_eq!(item["kind"], "card", "{context}: {item}");
                assert_eq!(item["card"]["id"], pin.card.to_string(), "{context}");
                visible_cards.insert(pin.card);
            } else {
                assert_eq!(item["kind"], "private_tile", "{context}: {item}");
                assert!(item.get("card").is_none(), "{context}: {item}");
            }
        }
        // A card no pin entitles this viewer to never appears anywhere in the response.
        for card in model.cards.keys().filter(|c| !visible_cards.contains(c)) {
            assert!(
                !text.contains(&card.to_string()),
                "seed {seed} {phase}: card {card} leaked to {}: {text}",
                viewer.id
            );
        }
        let urls = text.matches("render_url").count();
        assert!(
            urls <= visible_cards.len() * 2,
            "seed {seed} {phase}: {text}"
        );
        if visible_cards.is_empty() {
            assert!(
                !text.contains("zoomies"),
                "seed {seed} {phase}: caption leaked"
            );
            assert!(
                !text.contains("/v1/media-files/"),
                "seed {seed} {phase}: media leaked"
            );
        }
    }
}

/// Random unfriends and blocks (direct writes, as if done elsewhere).
async fn mutate_relations(env: &TestEnv, rng: &mut Rng, model: &mut Model) {
    let ids: Vec<Uuid> = model.people.iter().map(|p| p.id).collect();
    for _ in 0..2 {
        let a = ids[rng.below(ids.len())];
        let b = ids[rng.below(ids.len())];
        if a == b {
            continue;
        }
        if rng.chance(50) && model.friends.remove(&pair(a, b)) {
            sqlx::query("DELETE FROM social.friendships WHERE user_a = $1 AND user_b = $2")
                .bind(pair(a, b).0)
                .bind(pair(a, b).1)
                .execute(env.identity.pool())
                .await
                .unwrap();
        } else if model.blocks.insert(pair(a, b)) {
            block(env, a, b).await;
        }
    }
}

#[tokio::test]
async fn random_worlds_never_leak_cards_and_only_allowed_actors_mutate_pins() {
    let Some(env) = TestEnv::new().await else {
        return;
    };
    let base: u64 = std::env::var("BOARD_PROPERTY_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0x0B0A_2D5E_ED00_0110);
    let worlds: u64 = std::env::var("BOARD_PROPERTY_WORLDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(4);
    for n in 0..worlds {
        let seed = base.wrapping_add(n);
        let mut rng = Rng(seed);
        let (mut model, board) = build_world(&env, &mut rng).await;
        random_ops(&env, &mut rng, &mut model, board, seed).await;
        check_reads(&env, &model, board, seed, "after ops").await;
        mutate_relations(&env, &mut rng, &mut model).await;
        check_reads(&env, &model, board, seed, "after unfriend/block").await;
    }
}
