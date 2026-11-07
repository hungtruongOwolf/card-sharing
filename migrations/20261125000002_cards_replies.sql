-- ADR-0010: a reply to a card is a card. `reply_to_card_id` names the card it answers; the reply's
-- only recipient is that card's sender (enforced by the API on send). Cards are soft-deleted, so a
-- deleted original keeps the reference until the hard delete clears it (ON DELETE SET NULL); every
-- read resolves the reference per viewer and shows it only when the viewer may still read the
-- original.

ALTER TABLE cards.cards
    ADD COLUMN reply_to_card_id UUID NULL REFERENCES cards.cards (id) ON DELETE SET NULL;
-- The original sender lists the replies to one of their cards.
CREATE INDEX cards_reply_to ON cards.cards (reply_to_card_id) WHERE reply_to_card_id IS NOT NULL;
