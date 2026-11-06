import { useState } from 'react';
import { StyleSheet, View } from 'react-native';

import { radius, space } from '@card-sharing/tokens';

import { t } from '../../i18n';
import { Button, Screen, Text, useTheme } from '../../ui';
import { REACTION_DISPLAY } from '../feed/reactions';
import type { Outbox } from './outbox';
import type { OutboxEntry } from './outboxStore';

export interface OutboxScreenProps {
  outbox: Outbox;
  entries: readonly OutboxEntry[];
  onBack: () => void;
}

function title(entry: OutboxEntry): string {
  if (entry.kind === 'send') return entry.job.caption ?? t('outbox.card.untitled');
  const glyph = entry.reaction ? REACTION_DISPLAY[entry.reaction]?.glyph : undefined;
  return glyph ? t('outbox.reaction', { glyph }) : t('outbox.reaction.removed');
}

/**
 * What is waiting to be sent (T-78): pending items go out by themselves (cancel is one tap away); items that the
 * server refused or that kept failing for 24 h say "Not sent" and can be retried or deleted. Nothing is dropped
 * silently.
 */
export function OutboxScreen({ outbox, entries, onBack }: OutboxScreenProps) {
  const theme = useTheme();
  const [busy, setBusy] = useState(false);
  const act = (op: () => Promise<void>) => {
    setBusy(true);
    op().finally(() => setBusy(false));
  };
  const pending = entries.filter((entry) => entry.state === 'pending').length;

  return (
    <Screen title={t('outbox.title')} testID="outbox">
      {entries.length === 0 ? <Text tone="muted">{t('outbox.empty')}</Text> : null}
      {pending > 0 ? (
        <Button
          block
          label={t('outbox.sendNow')}
          loading={busy}
          onPress={() => act(() => outbox.process())}
        />
      ) : null}
      {entries.map((entry) => {
        const name = title(entry);
        const failed = entry.state === 'failed';
        return (
          <View
            key={entry.id}
            testID={`outbox-${entry.id}`}
            style={[
              styles.row,
              { backgroundColor: theme.color.surface.raised, borderColor: theme.hairline },
            ]}
          >
            <View style={styles.text}>
              <Text variant="label.md" numberOfLines={1}>
                {name}
              </Text>
              <Text tone={failed ? 'danger' : 'muted'}>
                {failed ? t('outbox.state.failed') : t('outbox.state.pending')}
              </Text>
            </View>
            {failed ? (
              <Button
                variant="secondary"
                label={t('outbox.retry')}
                accessibilityLabel={t('outbox.retry.a11y', { name })}
                disabled={busy}
                onPress={() => act(() => outbox.retry(entry.id))}
              />
            ) : null}
            <Button
              variant="ghost"
              label={failed ? t('outbox.delete') : t('outbox.cancel')}
              accessibilityLabel={
                failed ? t('outbox.delete.a11y', { name }) : t('outbox.cancel.a11y', { name })
              }
              disabled={busy}
              onPress={() => act(() => outbox.cancel(entry.id))}
            />
          </View>
        );
      })}
      <Button block variant="ghost" label={t('common.back')} onPress={onBack} />
    </Screen>
  );
}

const styles = StyleSheet.create({
  row: {
    flexDirection: 'row',
    flexWrap: 'wrap',
    alignItems: 'center',
    gap: space[3],
    padding: space[4],
    borderRadius: radius.md,
    borderWidth: StyleSheet.hairlineWidth,
  },
  text: { flex: 1, minWidth: 140, gap: space[1] },
});
