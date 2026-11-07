-- identity module: magic links for re-authentication (sprint 10 phase 2, ADR-0012).
-- Owner: crates/identity (magic_link.rs). Spec: context/wiki/architecture/data-model.md.
-- A `reauth` link is sent only to the address of the account's own email identity, on request of a signed-in
-- device session; it is bound to that account and session and redeems to a 5-minute re-auth token of that
-- session only. `sign_in` links (the default) never carry an account or session. Rows are purged after 24 hours.

ALTER TABLE identity.magic_links
    ADD COLUMN purpose TEXT NOT NULL DEFAULT 'sign_in' CHECK (purpose IN ('sign_in', 'reauth')),
    ADD COLUMN account_id UUID NULL,
    ADD COLUMN session_id UUID NULL,
    ADD CONSTRAINT magic_links_reauth_binding CHECK (
        (purpose = 'reauth') = (account_id IS NOT NULL AND session_id IS NOT NULL)
    );
