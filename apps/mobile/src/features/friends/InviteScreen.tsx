import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { Share, StyleSheet, useWindowDimensions, View } from 'react-native';

import { space } from '@card-sharing/tokens';

import { t } from '../../i18n';
import { QrCode } from '../../qr/QrCode';
import { Button, Screen, Text, useToast } from '../../ui';
import type { FriendsApi, MyInvite } from './friendsApi';
import { ConfirmSheet, friendsErrorMessage, QueryBody, sessionEnded } from './shared';

export const INVITE_QUERY_KEY = ['my-invite'] as const;

const DAY = 24 * 3_600_000;
const MAX_QR_SIZE = 240;

export interface InviteScreenProps {
  api: FriendsApi;
  onBack: () => void;
  /** Opens the system share sheet; injectable for tests. */
  share?: (message: string) => Promise<unknown>;
  now?: () => number;
}

const systemShare = (message: string) => Share.share({ message });

/** "Works for 5 more days" / "less than a day" for a minor's invite (AC-4.4); relative, no date format. */
function expiryLabel(expiresAt: number, now: number): string {
  const left = expiresAt - now;
  if (left < DAY) return t('invite.expires.soon');
  return t('invite.expires.days', { count: Math.floor(left / DAY) });
}

/**
 * My invite (US-4): the link as a QR code and through the share sheet, and "Reset link", which makes the
 * old link answer "invite expired" (AC-4.3). The link is a bearer credential for adding me: it is shown and
 * shared only on purpose, never logged.
 */
export function InviteScreen({
  api,
  onBack,
  share = systemShare,
  now = Date.now,
}: InviteScreenProps) {
  const toast = useToast();
  const queryClient = useQueryClient();
  const { width } = useWindowDimensions();
  const [confirming, setConfirming] = useState(false);
  const invite = useQuery({ queryKey: INVITE_QUERY_KEY, queryFn: () => api.myInvite() });

  const reset = useMutation({
    mutationFn: () => api.resetInvite(),
    onSuccess: (next: MyInvite) => {
      queryClient.setQueryData(INVITE_QUERY_KEY, next);
      toast.show({ message: t('invite.reset.done'), variant: 'success' });
    },
    onError: (error) => {
      if (!sessionEnded(error))
        toast.show({ message: friendsErrorMessage(error), variant: 'error' });
    },
    onSettled: () => setConfirming(false),
  });

  const qrSize = Math.min(width - space[8] * 2, MAX_QR_SIZE);
  return (
    <Screen title={t('invite.title')} testID="invite">
      <Text tone="muted">{t('invite.intro')}</Text>
      <QueryBody
        pending={invite.isPending}
        failed={invite.isError}
        loadingLabel={t('invite.loading')}
        errorText={t('invite.error.load')}
        onRetry={() => void invite.refetch()}
      >
        {() =>
          invite.data ? (
            <View style={styles.block}>
              <View style={styles.center}>
                <QrCode
                  value={invite.data.url}
                  size={qrSize}
                  accessibilityLabel={t('invite.qr.a11y')}
                />
              </View>
              <Text selectable testID="invite-url">
                {invite.data.url}
              </Text>
              {invite.data.expiresAt !== null ? (
                <Text tone="muted">{expiryLabel(invite.data.expiresAt, now())}</Text>
              ) : null}
              <Button
                block
                label={t('invite.share')}
                onPress={() => {
                  const message = t('invite.share.message', { url: invite.data.url });
                  share(message).catch(() =>
                    toast.show({ message: t('friends.error.generic'), variant: 'error' }),
                  );
                }}
              />
              <Button
                block
                variant="secondary"
                label={t('invite.reset')}
                onPress={() => setConfirming(true)}
              />
            </View>
          ) : null
        }
      </QueryBody>
      <Button block variant="ghost" label={t('common.back')} onPress={onBack} />
      <ConfirmSheet
        visible={confirming}
        title={t('invite.reset.title')}
        body={t('invite.reset.body')}
        confirmLabel={t('invite.reset')}
        busy={reset.isPending}
        onConfirm={() => reset.mutate()}
        onCancel={() => setConfirming(false)}
      />
    </Screen>
  );
}

const styles = StyleSheet.create({
  block: { gap: space[3] },
  center: { alignItems: 'center' },
});
