import { useMutation, useQuery } from '@tanstack/react-query';
import { ActivityIndicator, StyleSheet, View } from 'react-native';

import { radius, space } from '@card-sharing/tokens';

import { t } from '../../i18n';
import { Button, Screen, Text, useTheme, useToast } from '../../ui';
import { PrivacyError, type PrivacyApi } from './privacyApi';
import { formatDateTime, privacyErrorMessage, sessionEnded } from './privacyMessages';

export const DELETION_QUERY_KEY = ['account-deletion'] as const;

export interface DeletionPendingScreenProps {
  api: PrivacyApi;
  /** Re-reads `/v1/me`: after a cancel the account is active again and the app opens as usual. */
  onAccountChanged: () => Promise<void>;
  signOut: () => Promise<void>;
}

/**
 * Shown instead of the app while the account waits out its 7-day deletion grace period (AC-29.2, T-153). The
 * server lets this session do only three things (read the deletion, cancel it, log out), so the screen offers
 * exactly those. The banner shows when the deletion becomes final; cancelling restores the account.
 */
export function DeletionPendingScreen({
  api,
  onAccountChanged,
  signOut,
}: DeletionPendingScreenProps) {
  const theme = useTheme();
  const toast = useToast();
  const deletion = useQuery({ queryKey: DELETION_QUERY_KEY, queryFn: () => api.getDeletion() });

  const cancel = useMutation({
    mutationFn: () => api.cancelDeletion(),
    onSuccess: async () => {
      toast.show({ message: t('deletionPending.cancelled'), variant: 'success' });
      await onAccountChanged();
    },
    onError: async (error) => {
      if (sessionEnded(error)) return;
      if (error instanceof PrivacyError && error.kind === 'not_found') {
        // Nothing pending any more (cancelled elsewhere, or the grace period ended): ask the server again.
        toast.show({ message: t('deletionPending.gone') });
        await onAccountChanged();
        return;
      }
      toast.show({ message: privacyErrorMessage(error), variant: 'error' });
    },
  });

  const leave = useMutation({ mutationFn: signOut });

  let banner;
  if (deletion.isPending) {
    banner = (
      <ActivityIndicator
        color={theme.color.primary.base}
        accessibilityLabel={t('deletionPending.loading')}
      />
    );
  } else if (deletion.isError || !deletion.data) {
    banner = <Text tone="muted">{t('deletionPending.banner.unknown')}</Text>;
  } else {
    banner = (
      <Text accessibilityRole="alert">
        {t('deletionPending.banner', { date: formatDateTime(deletion.data.graceUntil) })}
      </Text>
    );
  }

  return (
    <Screen title={t('deletionPending.title')} testID="deletion-pending">
      <View
        testID="deletion-pending-banner"
        style={[
          styles.banner,
          { backgroundColor: theme.color.surface.raised, borderColor: theme.hairline },
        ]}
      >
        {banner}
      </View>
      <Text tone="muted">{t('deletionPending.body')}</Text>
      <Button
        block
        label={t('deletionPending.cancel')}
        loading={cancel.isPending}
        disabled={leave.isPending}
        onPress={() => cancel.mutate()}
      />
      <Button
        block
        variant="ghost"
        label={t('deletionPending.signOut')}
        loading={leave.isPending}
        disabled={cancel.isPending}
        onPress={() => leave.mutate()}
      />
    </Screen>
  );
}

const styles = StyleSheet.create({
  banner: {
    padding: space[4],
    borderRadius: radius.md,
    borderWidth: StyleSheet.hairlineWidth,
    gap: space[2],
  },
});
