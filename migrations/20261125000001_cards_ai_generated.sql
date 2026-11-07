-- Sprint 9 (genai-effects.md "AI badge"): a card whose render is the output of a succeeded AI effect
-- carries the visible "AI" badge. Decided by the server when the card is stored (the render's
-- `genai.effect_jobs.output_media_id`), never taken from a client, and fixed for the card's life.

ALTER TABLE cards.cards ADD COLUMN ai_generated BOOLEAN NOT NULL DEFAULT false;
