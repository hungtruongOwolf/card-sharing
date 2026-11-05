//! Friend-graph queries shared by requests, friendships, blocks and search. Every write that
//! depends on counts or on the relation between two accounts runs inside one transaction that
//! first takes [`lock_pair`], so concurrent requests and accepts cannot exceed the caps.

use sqlx::PgConnection;
use uuid::Uuid;

/// Advisory-lock key of an account: the random tail of its UUIDv7 (the head is a timestamp).
fn lock_key(id: Uuid) -> i64 {
    let bytes = id.as_bytes();
    let mut tail = [0u8; 8];
    tail.copy_from_slice(&bytes[8..]);
    i64::from_be_bytes(tail)
}

/// Locks both accounts for the rest of the transaction, in a fixed order (no deadlocks).
pub(crate) async fn lock_pair(
    conn: &mut PgConnection,
    a: Uuid,
    b: Uuid,
) -> Result<(), sqlx::Error> {
    let (first, second) = if lock_key(a) <= lock_key(b) {
        (a, b)
    } else {
        (b, a)
    };
    for id in [first, second] {
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(lock_key(id))
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

/// Canonical friendship row order.
pub(crate) fn ordered(a: Uuid, b: Uuid) -> (Uuid, Uuid) {
    if a < b { (a, b) } else { (b, a) }
}

/// A block exists between the two accounts, in either direction.
pub(crate) async fn blocked_either_way(
    conn: &mut PgConnection,
    a: Uuid,
    b: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM social.blocks
                        WHERE (blocker_id = $1 AND blocked_id = $2)
                           OR (blocker_id = $2 AND blocked_id = $1))",
    )
    .bind(a)
    .bind(b)
    .fetch_one(conn)
    .await
}

pub(crate) async fn are_friends(
    conn: &mut PgConnection,
    a: Uuid,
    b: Uuid,
) -> Result<bool, sqlx::Error> {
    let (user_a, user_b) = ordered(a, b);
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM social.friendships WHERE user_a = $1 AND user_b = $2)",
    )
    .bind(user_a)
    .bind(user_b)
    .fetch_one(conn)
    .await
}

pub(crate) async fn friend_count(conn: &mut PgConnection, id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM social.friendships WHERE user_a = $1)
              + (SELECT count(*) FROM social.friendships WHERE user_b = $1)",
    )
    .bind(id)
    .fetch_one(conn)
    .await
}

/// Accounts that are friends with both `a` and `b`.
pub(crate) async fn mutual_friend_count(
    conn: &mut PgConnection,
    a: Uuid,
    b: Uuid,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "WITH fa AS (SELECT CASE WHEN user_a = $1 THEN user_b ELSE user_a END AS friend
                     FROM social.friendships WHERE user_a = $1 OR user_b = $1),
              fb AS (SELECT CASE WHEN user_a = $2 THEN user_b ELSE user_a END AS friend
                     FROM social.friendships WHERE user_a = $2 OR user_b = $2)
         SELECT count(*) FROM fa JOIN fb USING (friend)",
    )
    .bind(a)
    .bind(b)
    .fetch_one(conn)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn friendship_order_is_canonical() {
        let a = Uuid::now_v7();
        let b = Uuid::now_v7();
        assert_eq!(ordered(a, b), ordered(b, a));
        let (low, high) = ordered(a, b);
        assert!(low < high);
    }

    #[test]
    fn lock_keys_use_the_random_tail() {
        // Two ids made in the same millisecond share their head but not their tail.
        assert_ne!(lock_key(Uuid::now_v7()), lock_key(Uuid::now_v7()));
    }
}
