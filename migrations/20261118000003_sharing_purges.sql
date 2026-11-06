-- T-133: CDN purge of revoked and expired public links (AC-24.4, SR-16). `purged_at` is set once
-- the link's cached page and OG image were purged from the CDN and the board's public objects
-- were deleted (when no other live link needs them).

ALTER TABLE sharing.public_links ADD COLUMN purged_at TIMESTAMPTZ NULL;

-- The sweep finds links that ended (revoked or expired) and were not purged yet.
CREATE INDEX public_links_unpurged_expiry ON sharing.public_links (expires_at)
    WHERE purged_at IS NULL AND expires_at IS NOT NULL;
CREATE INDEX public_links_unpurged_revoked ON sharing.public_links (revoked_at)
    WHERE purged_at IS NULL AND revoked_at IS NOT NULL;
