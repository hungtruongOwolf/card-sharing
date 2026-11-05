-- social module: minor invite rules (US-4 AC-4.4, SR-20). Owner: crates/social (invites.rs).
-- When the owner is a minor, an invite gets an expiry and a use cap from region policy
-- (config/policy/regions.yaml `invites`). use_count is incremented inside the friend-request
-- transaction; at the cap or past the expiry the token answers INVITE_EXPIRED.

ALTER TABLE social.invites
    ADD COLUMN expires_at TIMESTAMPTZ NULL,
    ADD COLUMN max_uses   INT NULL CHECK (max_uses IS NULL OR max_uses > 0),
    ADD COLUMN use_count  INT NOT NULL DEFAULT 0 CHECK (use_count >= 0);
