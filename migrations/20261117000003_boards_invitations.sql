-- T-106: board invitations (data-model.md `boards`, ADR-0007, AC-19.1, AC-19.8).
-- Membership needs the invitee's acceptance. `declined` and `dropped` (a block between the invitee
-- and a member) look `pending` to the owner until `expires_at` and keep their cap slot until then.

CREATE TABLE boards.board_invitations (
    id            UUID PRIMARY KEY,
    board_id      UUID NOT NULL REFERENCES boards.boards (id) ON DELETE CASCADE,
    inviter_id    UUID NOT NULL,
    invitee_id    UUID NOT NULL REFERENCES identity.accounts (id) ON DELETE CASCADE,
    status        TEXT NOT NULL CHECK (status IN (
                      'pending', 'accepted', 'declined', 'cancelled', 'expired', 'dropped')),
    created_at    TIMESTAMPTZ NOT NULL,
    expires_at    TIMESTAMPTZ NOT NULL,
    responded_at  TIMESTAMPTZ NULL,
    CHECK (invitee_id <> inviter_id)
);
-- At most one invitation per board and invitee that still looks open to the owner.
CREATE UNIQUE INDEX board_invitations_open
    ON boards.board_invitations (board_id, invitee_id)
    WHERE status IN ('pending', 'declined', 'dropped');
CREATE INDEX board_invitations_invitee
    ON boards.board_invitations (invitee_id, status, created_at DESC);
CREATE INDEX board_invitations_board_open
    ON boards.board_invitations (board_id) WHERE status IN ('pending', 'declined', 'dropped');
CREATE INDEX board_invitations_expiry
    ON boards.board_invitations (expires_at) WHERE status IN ('pending', 'declined', 'dropped');
