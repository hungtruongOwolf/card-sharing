//! Seeded property tests of the board permission rules (T-110, SR-5, SR-6, SR-7, SR-21).
//!
//! Random worlds (people, ages, friendships, blocks, card states, recipients, pinners) are checked
//! against an oracle written from the spec (api-mvp.md "Board read resolution", ADR-0007), not from
//! the implementation. No external crate: a SplitMix64 generator seeded from `BOARD_PROPERTY_SEED`
//! (or a fixed default) so any failure reproduces with the seed it prints.

use std::collections::HashSet;

use serde_json::json;
use uuid::Uuid;

use crate::ops::{CardCheck, CardFacts as PinFacts, pin_permission};
use crate::pins::{Pin, PinContent, Transform};
use crate::resolve::{CardFacts, Resolution, ViewerContext, resolve_pin};
use crate::service::Role;

/// SplitMix64: tiny, fast, good enough to explore permission truth tables.
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub(crate) fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub(crate) fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }

    pub(crate) fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(n.max(1)).unwrap_or(1)).unwrap_or(0)
    }
}

pub(crate) fn seed() -> u64 {
    std::env::var("BOARD_PROPERTY_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0x5EED_B0A2_D501_10AB)
}

/// A random world of people seen from one viewer.
struct World {
    people: Vec<Uuid>,
    minors: HashSet<Uuid>,
    /// Unordered pairs that are friends.
    friends: HashSet<(Uuid, Uuid)>,
    /// Unordered pairs with a block either way.
    blocks: HashSet<(Uuid, Uuid)>,
}

fn pair(a: Uuid, b: Uuid) -> (Uuid, Uuid) {
    if a < b { (a, b) } else { (b, a) }
}

impl World {
    fn random(rng: &mut Rng) -> Self {
        let people: Vec<Uuid> = (0..2 + rng.below(5)).map(|_| Uuid::now_v7()).collect();
        let mut world = Self {
            minors: people.iter().copied().filter(|_| rng.chance(35)).collect(),
            friends: HashSet::new(),
            blocks: HashSet::new(),
            people,
        };
        for (i, a) in world.people.iter().enumerate() {
            for b in &world.people[i + 1..] {
                if rng.chance(50) {
                    world.friends.insert(pair(*a, *b));
                }
                if rng.chance(15) {
                    world.blocks.insert(pair(*a, *b));
                }
            }
        }
        world
    }

    /// Friends with no block either way (the card-read predicate's notion).
    fn friends_unblocked(&self, a: Uuid, b: Uuid) -> bool {
        self.friends.contains(&pair(a, b)) && !self.blocks.contains(&pair(a, b))
    }

    fn pick(&self, rng: &mut Rng) -> Uuid {
        self.people[rng.below(self.people.len())]
    }

    fn context_for(&self, viewer: Uuid) -> ViewerContext {
        ViewerContext {
            viewer,
            viewer_is_minor: self.minors.contains(&viewer),
            minors: self.minors.clone(),
            friends: self
                .people
                .iter()
                .copied()
                .filter(|p| *p != viewer && self.friends_unblocked(viewer, *p))
                .collect(),
        }
    }
}

const STATUSES: [&str; 6] = [
    "active",
    "active",
    "active",
    "held_review",
    "pending_scan",
    "deleted",
];

fn pin(created_by: Uuid, card: Uuid, deleted: bool) -> Pin {
    Pin {
        id: Uuid::now_v7(),
        board_id: Uuid::nil(),
        created_by,
        content: PinContent::Card(card),
        transform: Transform {
            x: 0.5,
            y: 0.5,
            scale: 1.0,
            rotation: 0.0,
            z_key: "a".to_owned(),
        },
        version: 1,
        changed_at_board_version: 1,
        deleted_at: deleted.then(time::OffsetDateTime::now_utc),
    }
}

/// The spec: may `viewer` see the card behind a pin by `pinner`?
fn oracle_sees(
    world: &World,
    viewer: Uuid,
    pinner: Uuid,
    owner: Uuid,
    status: &str,
    delivered_to_viewer: bool,
) -> bool {
    let entitled = status == "active"
        && (viewer == owner || (delivered_to_viewer && world.friends_unblocked(viewer, owner)));
    let viewer_adult = !world.minors.contains(&viewer);
    let hides = |person: Uuid| {
        viewer_adult
            && person != viewer
            && world.minors.contains(&person)
            && !world.friends_unblocked(viewer, person)
    };
    entitled && !hides(pinner) && !hides(owner)
}

#[test]
fn no_viewer_sees_a_card_the_spec_does_not_entitle_them_to() {
    let seed = seed();
    let mut rng = Rng::new(seed);
    for case in 0..20_000 {
        let world = World::random(&mut rng);
        let viewer = world.pick(&mut rng);
        let owner = world.pick(&mut rng);
        let pinner = world.pick(&mut rng);
        let status = STATUSES[rng.below(STATUSES.len())];
        let delivered = viewer != owner && rng.chance(50);
        let tombstone = rng.chance(10);
        let card = Uuid::now_v7();
        // What the card-read SQL predicate reports for this viewer (cards::access).
        let entitled = (viewer == owner && status != "deleted")
            || (status == "active" && delivered && world.friends_unblocked(viewer, owner));
        let facts = CardFacts {
            owner_id: owner,
            active: status == "active",
            deleted: status == "deleted",
            entitled,
        };
        let got = resolve_pin(
            &pin(pinner, card, tombstone),
            Some(&facts),
            &world.context_for(viewer),
        );
        let expected = if tombstone || status == "deleted" {
            Resolution::Removed
        } else if oracle_sees(&world, viewer, pinner, owner, status, delivered) {
            Resolution::Card(card)
        } else {
            Resolution::PrivateTile
        };
        assert_eq!(
            got, expected,
            "seed {seed}, case {case}: viewer {viewer} pinner {pinner} owner {owner} status {status} delivered {delivered}"
        );
        // The invariant itself: a card is only ever shown to someone the spec entitles.
        if matches!(got, Resolution::Card(_)) {
            assert!(oracle_sees(
                &world, viewer, pinner, owner, status, delivered
            ));
        }
    }
}

#[test]
fn only_permitted_actors_may_pin_a_card() {
    let seed = seed();
    let mut rng = Rng::new(seed ^ 0xA11CE);
    for case in 0..20_000 {
        let role = if rng.chance(50) {
            Role::Owner
        } else {
            Role::Collaborator
        };
        let mine = rng.chance(50);
        let status = STATUSES[rng.below(STATUSES.len())];
        let received_visible = !mine && status == "active" && rng.chance(50);
        let exists = rng.chance(90);
        let facts = exists.then_some(PinFacts {
            mine,
            active: status == "active",
            held: status == "held_review",
            received_visible,
        });
        let got = pin_permission(role, facts);
        let expected = if !exists {
            CardCheck::Denied
        } else if mine && status == "active" {
            CardCheck::Allowed
        } else if mine && status == "held_review" {
            CardCheck::Held
        } else if !mine && role == Role::Owner && received_visible {
            CardCheck::Allowed
        } else {
            CardCheck::Denied
        };
        assert_eq!(
            got, expected,
            "seed {seed}, case {case}: {role:?} mine {mine} {status} received {received_visible}"
        );
    }
}

#[test]
fn decorations_never_depend_on_people() {
    let seed = seed();
    let mut rng = Rng::new(seed ^ 0xDEC0);
    for _ in 0..1_000 {
        let world = World::random(&mut rng);
        let viewer = world.pick(&mut rng);
        let mut decoration = pin(world.pick(&mut rng), Uuid::now_v7(), false);
        decoration.content = PinContent::Decoration(json!({ "id": "deco.washi", "version": 1 }));
        assert_eq!(
            resolve_pin(&decoration, None, &world.context_for(viewer)),
            Resolution::Decoration
        );
    }
}
