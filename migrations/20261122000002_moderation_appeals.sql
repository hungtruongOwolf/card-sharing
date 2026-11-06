-- moderation module: appeals of classifier holds join the queue (T-148, US-28 AC-28.3).
-- An appeal is a report with `source = 'appeal'` about the held card, filed by its owner, one per
-- hold. Moderators release (the card is delivered) or uphold (the card is blocked) the hold.
ALTER TABLE moderation.reports ADD COLUMN hold_id UUID NULL
    CHECK ((source = 'appeal') = (hold_id IS NOT NULL));
CREATE UNIQUE INDEX reports_appeal_hold ON moderation.reports (hold_id) WHERE source = 'appeal';
