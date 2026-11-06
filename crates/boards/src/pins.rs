//! Pins (ADR-0007): a card or decoration placed on a board. Pure types and validation; the op
//! batch (`ops`) and the read path (`resolve`) do the I/O.
//!
//! Coordinates are board units: `x`, `y` = the pin's centre as a fraction of the board's width and
//! height (0..=1); `scale` 0.1..=10; `rotation` in degrees, any finite value with |r| <= 360, stored
//! normalized to [-180, 180); `z_key` = fractional-index string, 1..=32 of `[0-9A-Za-z]`.

use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

pub const SCALE_MIN: f64 = 0.1;
pub const SCALE_MAX: f64 = 10.0;
pub const Z_KEY_MAX: usize = 32;

/// Why a transform was refused (safe to show).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransformError {
    #[error("{0} must be a number between 0 and 1")]
    Coordinate(&'static str),
    #[error("scale must be between 0.1 and 10")]
    Scale,
    #[error("rotation must be a number between -360 and 360")]
    Rotation,
    #[error("z_key must be 1 to 32 characters of 0-9, A-Z, a-z")]
    ZKey,
    #[error("set must change at least one field")]
    EmptyPatch,
}

/// Where and how a pin sits on the board. Construct through [`Transform::new`].
#[derive(Debug, Clone, PartialEq)]
pub struct Transform {
    pub x: f64,
    pub y: f64,
    pub scale: f64,
    pub rotation: f64,
    pub z_key: String,
}

fn coordinate(value: f64, name: &'static str) -> Result<f64, TransformError> {
    if value.is_finite() && (0.0..=1.0).contains(&value) {
        Ok(value)
    } else {
        Err(TransformError::Coordinate(name))
    }
}

fn scale(value: f64) -> Result<f64, TransformError> {
    if value.is_finite() && (SCALE_MIN..=SCALE_MAX).contains(&value) {
        Ok(value)
    } else {
        Err(TransformError::Scale)
    }
}

/// Degrees normalized to [-180, 180).
fn rotation(value: f64) -> Result<f64, TransformError> {
    if !value.is_finite() || value.abs() > 360.0 {
        return Err(TransformError::Rotation);
    }
    let normalized = (value + 180.0).rem_euclid(360.0) - 180.0;
    // rem_euclid can round up to exactly 360 for tiny negative inputs.
    Ok(if normalized >= 180.0 {
        -180.0
    } else {
        normalized
    })
}

fn z_key(value: &str) -> Result<String, TransformError> {
    let valid = !value.is_empty()
        && value.len() <= Z_KEY_MAX
        && value.bytes().all(|b| b.is_ascii_alphanumeric());
    if valid {
        Ok(value.to_owned())
    } else {
        Err(TransformError::ZKey)
    }
}

impl Transform {
    pub fn new(
        x: f64,
        y: f64,
        scale_value: f64,
        rotation_value: f64,
        z_key_value: &str,
    ) -> Result<Self, TransformError> {
        Ok(Self {
            x: coordinate(x, "x")?,
            y: coordinate(y, "y")?,
            scale: scale(scale_value)?,
            rotation: rotation(rotation_value)?,
            z_key: z_key(z_key_value)?,
        })
    }

    /// This transform with `patch` applied.
    pub fn patched(&self, patch: &TransformPatch) -> Self {
        Self {
            x: patch.x.unwrap_or(self.x),
            y: patch.y.unwrap_or(self.y),
            scale: patch.scale.unwrap_or(self.scale),
            rotation: patch.rotation.unwrap_or(self.rotation),
            z_key: patch.z_key.clone().unwrap_or_else(|| self.z_key.clone()),
        }
    }
}

/// The fields a `pin.update` changes. Construct through [`TransformPatch::new`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TransformPatch {
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub scale: Option<f64>,
    pub rotation: Option<f64>,
    pub z_key: Option<String>,
}

impl TransformPatch {
    pub fn new(
        x: Option<f64>,
        y: Option<f64>,
        scale_value: Option<f64>,
        rotation_value: Option<f64>,
        z_key_value: Option<&str>,
    ) -> Result<Self, TransformError> {
        let patch = Self {
            x: x.map(|v| coordinate(v, "x")).transpose()?,
            y: y.map(|v| coordinate(v, "y")).transpose()?,
            scale: scale_value.map(scale).transpose()?,
            rotation: rotation_value.map(rotation).transpose()?,
            z_key: z_key_value.map(z_key).transpose()?,
        };
        if patch == Self::default() {
            return Err(TransformError::EmptyPatch);
        }
        Ok(patch)
    }
}

/// What a pin shows.
#[derive(Debug, Clone, PartialEq)]
pub enum PinContent {
    /// A card (logical reference into the cards module).
    Card(Uuid),
    /// A registry decoration `{id, version, params}` (validated by the caller).
    Decoration(Value),
}

/// A stored pin (tombstones included).
#[derive(Debug, Clone, PartialEq)]
pub struct Pin {
    pub id: Uuid,
    pub board_id: Uuid,
    pub created_by: Uuid,
    pub content: PinContent,
    pub transform: Transform,
    pub version: i64,
    pub changed_at_board_version: i64,
    pub deleted_at: Option<OffsetDateTime>,
}

#[derive(sqlx::FromRow)]
pub(crate) struct PinRow {
    id: Uuid,
    board_id: Uuid,
    created_by: Uuid,
    card_id: Option<Uuid>,
    decoration: Option<Value>,
    x: f64,
    y: f64,
    scale: f64,
    rotation: f64,
    z_key: String,
    version: i64,
    changed_at_board_version: i64,
    deleted_at: Option<OffsetDateTime>,
}

/// Columns of [`PinRow`] for queries aliasing `boards.pins` as `p`.
macro_rules! pin_columns {
    () => {
        "p.id, p.board_id, p.created_by, p.card_id, p.decoration, p.x, p.y, p.scale, p.rotation,
         p.z_key, p.version, p.changed_at_board_version, p.deleted_at"
    };
}
pub(crate) use pin_columns;

impl From<PinRow> for Pin {
    fn from(row: PinRow) -> Self {
        let content = match (row.card_id, row.decoration) {
            (Some(card), _) => PinContent::Card(card),
            (None, decoration) => PinContent::Decoration(decoration.unwrap_or(Value::Null)),
        };
        Self {
            id: row.id,
            board_id: row.board_id,
            created_by: row.created_by,
            content,
            transform: Transform {
                x: row.x,
                y: row.y,
                scale: row.scale,
                rotation: row.rotation,
                z_key: row.z_key,
            },
            version: row.version,
            changed_at_board_version: row.changed_at_board_version,
            deleted_at: row.deleted_at,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn valid_transforms_are_kept_and_rotation_is_normalized() {
        let t = Transform::new(0.0, 1.0, 0.1, 190.0, "a0V").unwrap();
        assert_eq!((t.x, t.y, t.scale), (0.0, 1.0, 0.1));
        assert!((t.rotation + 170.0).abs() < 1e-9);
        assert_eq!(
            Transform::new(0.5, 0.5, 10.0, -180.0, "z")
                .unwrap()
                .rotation,
            -180.0
        );
        assert_eq!(
            Transform::new(0.5, 0.5, 1.0, 180.0, "z").unwrap().rotation,
            -180.0
        );
        assert_eq!(
            Transform::new(0.5, 0.5, 1.0, -360.0, "z").unwrap().rotation,
            0.0
        );
    }

    #[test]
    fn out_of_range_and_non_finite_values_are_refused() {
        for (x, y, s, r, z) in [
            (1.01, 0.5, 1.0, 0.0, "a"),
            (0.5, -0.01, 1.0, 0.0, "a"),
            (f64::NAN, 0.5, 1.0, 0.0, "a"),
            (0.5, 0.5, 1e308, 0.0, "a"),
            (0.5, 0.5, 0.09, 0.0, "a"),
            (0.5, 0.5, f64::INFINITY, 0.0, "a"),
            (0.5, 0.5, 1.0, 361.0, "a"),
            (0.5, 0.5, 1.0, f64::NAN, "a"),
            (0.5, 0.5, 1.0, 0.0, ""),
            (0.5, 0.5, 1.0, 0.0, "a-b"),
            (0.5, 0.5, 1.0, 0.0, "\u{e9}"),
        ] {
            assert!(
                Transform::new(x, y, s, r, z).is_err(),
                "{x} {y} {s} {r} {z}"
            );
        }
        assert!(Transform::new(0.5, 0.5, 1.0, 0.0, &"a".repeat(33)).is_err());
    }

    #[test]
    fn patches_validate_each_field_and_must_change_something() {
        assert_eq!(
            TransformPatch::new(None, None, None, None, None),
            Err(TransformError::EmptyPatch)
        );
        assert!(TransformPatch::new(None, None, Some(1e308), None, None).is_err());
        let patch = TransformPatch::new(Some(0.25), None, None, Some(-190.0), None).unwrap();
        let base = Transform::new(0.5, 0.5, 2.0, 0.0, "m").unwrap();
        let moved = base.patched(&patch);
        assert_eq!((moved.x, moved.y, moved.scale), (0.25, 0.5, 2.0));
        assert!((moved.rotation - 170.0).abs() < 1e-9);
        assert_eq!(moved.z_key, "m");
    }
}
