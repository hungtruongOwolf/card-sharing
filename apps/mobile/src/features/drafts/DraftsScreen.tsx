import { useCallback, useEffect, useState } from 'react';
import { Image, StyleSheet, View } from 'react-native';

import { radius, space } from '@card-sharing/tokens';

import { t } from '../../i18n';
import { Button, Screen, Sheet, Text, useToast } from '../../ui';
import type { LocalCard } from '../cards/localCard';
import type { DraftStore } from './draftStore';
import {
  draftAgeDays,
  isLowStorage,
  STALE_DRAFT_DAYS,
  staleDrafts,
  unknownStorageInfo,
  type StorageInfo,
} from './housekeeping';

export interface DraftsScreenProps {
  store: DraftStore;
  storageInfo?: StorageInfo;
  /** Clock seam for tests. */
  now?: () => Date;
  onOpen: (card: LocalCard) => void;
  /** Send a draft (T-77); without it the tray has no Send button. */
  onSend?: (card: LocalCard) => void;
  onNew: () => void;
}

type Loaded = { status: 'loading' } | { status: 'error' } | { status: 'ready'; cards: LocalCard[] };

const THUMB = 64;

/**
 * Drafts tray (T-68, US-9): unsent cards on this device, newest first. Works offline (no network at all). Offers
 * to clean up drafts older than 30 days and warns when free space is low (prd-mvp edge cases).
 */
export function DraftsScreen({
  store,
  storageInfo = unknownStorageInfo,
  now = () => new Date(),
  onOpen,
  onSend,
  onNew,
}: DraftsScreenProps) {
  const toast = useToast();
  const [loaded, setLoaded] = useState<Loaded>({ status: 'loading' });
  const [lowStorage, setLowStorage] = useState(false);
  const [pending, setPending] = useState<
    { kind: 'delete'; card: LocalCard } | { kind: 'cleanup'; cards: LocalCard[] } | null
  >(null);
  const [busy, setBusy] = useState(false);

  const reload = useCallback(() => {
    store.list().then(
      (cards) => setLoaded({ status: 'ready', cards }),
      () => setLoaded({ status: 'error' }),
    );
  }, [store]);

  useEffect(() => {
    reload();
    let cancelled = false;
    storageInfo.freeBytes().then(
      (free) => {
        if (!cancelled) setLowStorage(isLowStorage(free));
      },
      () => undefined,
    );
    return () => {
      cancelled = true;
    };
  }, [reload, storageInfo]);

  const confirm = async () => {
    if (!pending) return;
    const ids = pending.kind === 'delete' ? [pending.card.id] : pending.cards.map((c) => c.id);
    setBusy(true);
    try {
      for (const id of ids) await store.remove(id);
      toast.show({ message: t('drafts.deleted', { count: ids.length }) });
    } catch {
      toast.show({ message: t('drafts.error'), variant: 'error' });
    } finally {
      setBusy(false);
      setPending(null);
      reload();
    }
  };

  if (loaded.status === 'loading') {
    return (
      <Screen testID="drafts-loading" centered>
        <Text tone="muted">{t('drafts.loading')}</Text>
      </Screen>
    );
  }
  if (loaded.status === 'error') {
    return (
      <Screen title={t('drafts.title')} testID="drafts-error">
        <Text tone="muted">{t('drafts.error')}</Text>
        <Button block label={t('drafts.retry')} onPress={reload} />
      </Screen>
    );
  }

  const today = now();
  const stale = staleDrafts(loaded.cards, today);
  return (
    <Screen title={t('drafts.title')} testID="drafts">
      <Text tone="muted">{t('drafts.privacy')}</Text>
      {lowStorage ? (
        <Text accessibilityRole="alert" testID="drafts-low-storage">
          {t('drafts.lowStorage')}
        </Text>
      ) : null}
      {stale.length > 0 ? (
        <View style={styles.banner} testID="drafts-cleanup">
          <Text>{t('drafts.cleanup.body', { count: stale.length, days: STALE_DRAFT_DAYS })}</Text>
          <Button
            variant="secondary"
            label={t('drafts.cleanup.action')}
            onPress={() => setPending({ kind: 'cleanup', cards: stale })}
          />
        </View>
      ) : null}
      {loaded.cards.length === 0 ? <Text tone="muted">{t('drafts.empty')}</Text> : null}
      {loaded.cards.map((card) => {
        const title = card.draft.caption || t('drafts.untitled');
        const days = draftAgeDays(card, today);
        return (
          <View key={card.id} style={styles.row}>
            <Image
              source={{ uri: card.photo.uri }}
              style={styles.thumb}
              resizeMode="cover"
              accessibilityIgnoresInvertColors
            />
            <View style={styles.grow}>
              <Text numberOfLines={1}>{title}</Text>
              <Text tone="muted">
                {days === 0 ? t('drafts.editedToday') : t('drafts.edited', { days })}
              </Text>
            </View>
            <Button
              label={t('drafts.open')}
              accessibilityLabel={t('drafts.open.a11y', { title })}
              onPress={() => onOpen(card)}
            />
            {onSend ? (
              <Button
                variant="secondary"
                label={t('drafts.send')}
                accessibilityLabel={t('drafts.send.a11y', { title })}
                onPress={() => onSend(card)}
              />
            ) : null}
            <Button
              variant="ghost"
              label={t('drafts.delete')}
              accessibilityLabel={t('drafts.delete.a11y', { title })}
              onPress={() => setPending({ kind: 'delete', card })}
            />
          </View>
        );
      })}
      <Button block variant="secondary" label={t('drafts.new')} onPress={onNew} />
      <Sheet
        visible={pending !== null}
        title={t('drafts.confirm.title', {
          count: pending?.kind === 'cleanup' ? pending.cards.length : 1,
        })}
        onClose={() => {
          if (!busy) setPending(null);
        }}
      >
        <Text>{t('drafts.confirm.body')}</Text>
        <Button
          block
          variant="danger"
          label={t('drafts.confirm.delete')}
          loading={busy}
          onPress={() => void confirm()}
        />
        <Button
          block
          variant="ghost"
          label={t('drafts.confirm.cancel')}
          disabled={busy}
          onPress={() => setPending(null)}
        />
      </Sheet>
    </Screen>
  );
}

const styles = StyleSheet.create({
  row: { flexDirection: 'row', flexWrap: 'wrap', alignItems: 'center', gap: space[3] },
  grow: { flex: 1, minWidth: 120 },
  thumb: { width: THUMB, height: THUMB, borderRadius: radius.sm },
  banner: { gap: space[2] },
});
