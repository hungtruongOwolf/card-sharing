import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useEffect, useState } from 'react';
import { Share, StyleSheet, View } from 'react-native';

import { space } from '@card-sharing/tokens';

import { t } from '../../i18n';
import { Button, Sheet, Text, useToast } from '../../ui';
import { newIdempotencyKey } from '../friends/idempotencyKey';
import type { PublicLinkPrefs } from './publicLinkPrefs';
import {
  LINK_EXPIRIES,
  LinksDisallowedError,
  type CreatedLink,
  type LinkExpiry,
  type PublicLinksApi,
} from './publicLinksApi';

export const publicLinksQueryKey = (boardId: string) =>
  ['boards', boardId, 'public-links'] as const;

const EXPIRY_KEYS = {
  '24h': 'boards.links.expiry.24h',
  '7d': 'boards.links.expiry.7d',
  never: 'boards.links.expiry.never',
} as const;

export interface PublicLinksSheetProps {
  visible: boolean;
  boardId: string;
  api: PublicLinksApi;
  prefs: PublicLinkPrefs;
  onClose: () => void;
  /** The server refused links for this account (AC-24.6): the board hides the action. */
  onDisallowed: () => void;
  /** Share sheet seam (tests). */
  share?: (url: string) => Promise<unknown>;
}

const shareUrl = (url: string) => Share.share({ message: url, url });

/**
 * Manage a board's public links (T-134, US-24): a one-time explanation of link previews first (AC-24.7), then create
 * a link with an expiry (24 h, 7 days, never), share the URL, which the server shows only once, and revoke live links
 * (the page answers 404 within a minute, AC-24.4). Owner only; the board shows the action only to its owner.
 */
export function PublicLinksSheet({
  visible,
  boardId,
  api,
  prefs,
  onClose,
  onDisallowed,
  share = shareUrl,
}: PublicLinksSheetProps) {
  const queryClient = useQueryClient();
  const toast = useToast();
  const [expiry, setExpiry] = useState<LinkExpiry>('7d');
  const [created, setCreated] = useState<CreatedLink | null>(null);
  const [disclosed, setDisclosed] = useState<boolean | null>(null);

  useEffect(() => {
    if (!visible) return;
    let cancelled = false;
    void prefs.disclosureSeen().then((seen) => !cancelled && setDisclosed(seen));
    return () => {
      cancelled = true;
    };
  }, [visible, prefs]);

  const links = useQuery({
    queryKey: publicLinksQueryKey(boardId),
    queryFn: () => api.list(boardId),
    enabled: visible && disclosed === true,
  });
  const refresh = () => queryClient.invalidateQueries({ queryKey: publicLinksQueryKey(boardId) });
  const failed = () => toast.show({ message: t('boards.links.error'), variant: 'error' });

  const create = useMutation({
    mutationFn: (expiresIn: LinkExpiry) => api.create(boardId, expiresIn, newIdempotencyKey()),
    onSuccess: async (link) => {
      setCreated(link);
      await refresh();
    },
    onError: async (error) => {
      if (error instanceof LinksDisallowedError) {
        await prefs.markDisallowed().catch(() => undefined);
        toast.show({ message: t('boards.links.disallowed'), variant: 'error' });
        onDisallowed();
        onClose();
      } else failed();
    },
  });
  const revoke = useMutation({
    mutationFn: (linkId: string) => api.revoke(linkId),
    onSuccess: async (_v, linkId) => {
      if (created?.id === linkId) setCreated(null);
      toast.show({ message: t('boards.links.revoked') });
      await refresh();
    },
    onError: failed,
  });

  const close = () => {
    // The URL is shown once: closing the sheet forgets it.
    setCreated(null);
    onClose();
  };

  return (
    <Sheet visible={visible} onClose={close} title={t('boards.links.title')} testID="public-links">
      {disclosed === false ? (
        <View style={styles.block} testID="public-links-disclosure">
          <Text>{t('boards.links.disclosure')}</Text>
          <Button
            label={t('boards.links.disclosure.confirm')}
            onPress={async () => {
              await prefs.markDisclosureSeen().catch(() => undefined);
              setDisclosed(true);
            }}
          />
        </View>
      ) : null}
      {disclosed === true ? (
        <View style={styles.block}>
          <Text tone="muted">{t('boards.links.body')}</Text>
          <View style={styles.row} accessibilityRole="radiogroup">
            {LINK_EXPIRIES.map((option) => (
              <Button
                key={option}
                label={t(EXPIRY_KEYS[option])}
                variant={option === expiry ? 'primary' : 'secondary'}
                accessibilityRole="radio"
                accessibilityState={{ checked: option === expiry }}
                onPress={() => setExpiry(option)}
              />
            ))}
          </View>
          <Button
            label={t('boards.links.create')}
            loading={create.isPending}
            onPress={() => create.mutate(expiry)}
          />
          {created ? (
            <View style={styles.block} testID="public-links-created">
              <Text selectable>{created.url}</Text>
              <Text tone="muted">{t('boards.links.once')}</Text>
              <Button
                label={t('boards.links.share')}
                variant="secondary"
                onPress={() => void share(created.url).catch(() => undefined)}
              />
            </View>
          ) : null}
          <Text variant="label.md">{t('boards.links.live')}</Text>
          {links.isError ? <Text tone="danger">{t('boards.links.error')}</Text> : null}
          {links.data?.length === 0 ? <Text tone="muted">{t('boards.links.none')}</Text> : null}
          {(links.data ?? []).map((link) => {
            const expires =
              link.expiresAt === null
                ? t('boards.links.neverExpires')
                : t('boards.links.expires', { date: new Date(link.expiresAt).toLocaleString() });
            return (
              <View key={link.id} style={styles.row}>
                <Text style={styles.grow}>{expires}</Text>
                <Button
                  label={t('boards.links.revoke')}
                  accessibilityLabel={t('boards.links.revoke.a11y', { expires })}
                  variant="ghost"
                  onPress={() => revoke.mutate(link.id)}
                />
              </View>
            );
          })}
        </View>
      ) : null}
    </Sheet>
  );
}

const styles = StyleSheet.create({
  block: { gap: space[3] },
  row: { flexDirection: 'row', flexWrap: 'wrap', alignItems: 'center', gap: space[2] },
  grow: { flexGrow: 1, flexShrink: 1 },
});
