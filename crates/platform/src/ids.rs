//! Identifiers. Every server-generated primary key is a UUIDv7 (time-ordered, index friendly,
//! not guessable from neighbours). Client-generated ids (offline creation) must also be v7.

use uuid::Uuid;

/// A new UUIDv7.
pub fn new_id() -> Uuid {
    Uuid::now_v7()
}

/// True when `id` is a version-7 UUID. Use it to validate client-generated ids.
pub fn is_v7(id: &Uuid) -> bool {
    id.get_version_num() == 7
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_ids_are_v7_and_time_ordered() {
        let first = new_id();
        let second = new_id();
        assert!(is_v7(&first));
        assert!(first < second, "UUIDv7 values sort by creation time");
    }

    #[test]
    fn rejects_other_versions() {
        assert!(!is_v7(&Uuid::nil()));
        assert!(!is_v7(&Uuid::from_u128(
            0x0192_f0c1_7c1e_4a3b_9d2e_1234_5678_9abc
        )));
    }
}
