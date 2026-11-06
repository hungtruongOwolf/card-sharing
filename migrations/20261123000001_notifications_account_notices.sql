-- notifications module: account notices in the activity tab (#331): a data export was requested
-- (SR-36: every device is told, so a stolen session cannot export silently), an export is ready,
-- a public link was disabled automatically (traffic anomaly). Ids only.
ALTER TABLE notifications.activity_items DROP CONSTRAINT activity_items_kind_check;
ALTER TABLE notifications.activity_items ADD CONSTRAINT activity_items_kind_check CHECK (kind IN (
    'card.new', 'reaction.new', 'friend.request', 'friend.accepted', 'card.held',
    'export.requested', 'export.ready', 'link.disabled'));
