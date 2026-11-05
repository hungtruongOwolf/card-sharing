import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { ActivityIndicator, StyleSheet, View } from 'react-native';

import { radius, space } from '@card-sharing/tokens';

import { getLocale, t } from '../../i18n';
import { Button, Screen, Sheet, Text, useTheme, useToast } from '../../ui';
import { AuthError } from '../auth';
import { countryName, lastSeenLabel } from './lastSeen';
import { SessionsError, type DeviceSession, type SessionsApi } from './sessionsApi';

export const SESSIONS_QUERY_KEY = ['sessions'] as const;

export interface SessionsScreenProps {
  api: SessionsApi;
  /** Auth client's `signOutEverywhere`: revokes every session, then clears this device. */
  signOutEverywhere: () => Promise<void>;
  onBack: () => void;
  now?: () => number;
}

/** The device is already signed out (the auth client cleared it); the stack moves to sign-in by itself. */
const sessionEnded = (error: unknown) =>
  error instanceof AuthError && (error.kind === 'session_ended' || error.kind === 'signed_out');

const errorMessage = (error: unknown) =>
  (error instanceof SessionsError && error.kind === 'rate_limited') ||
  (error instanceof AuthError && error.status === 429)
    ? t('sessions.error.rateLimited')
    : t('sessions.error.generic');

function DeviceRow({
  device,
  now,
  revoking,
  onRevoke,
}: {
  device: DeviceSession;
  now: number;
  revoking: boolean;
  onRevoke: (device: DeviceSession) => void;
}) {
  const theme = useTheme();
  const name = device.clientLabel ?? t('sessions.device.unknown');
  return (
    <View
      testID={`session-${device.id}`}
      style={[
        styles.row,
        { backgroundColor: theme.color.surface.raised, borderColor: theme.hairline },
      ]}
    >
      <View style={styles.rowText}>
        <Text variant="label.md">{name}</Text>
        {device.isCurrent ? (
          <Text variant="label.sm" tone="primary">
            {t('sessions.device.current')}
          </Text>
        ) : null}
        {device.country ? (
          <Text tone="muted">
            {t('sessions.device.country', { country: countryName(device.country, getLocale()) })}
          </Text>
        ) : null}
        <Text tone="muted">{lastSeenLabel(device.lastSeenAt, now)}</Text>
      </View>
      {device.isCurrent ? null : (
        <Button
          label={t('sessions.revoke')}
          accessibilityLabel={t('sessions.revoke.a11y', { device: name })}
          variant="secondary"
          loading={revoking}
          onPress={() => onRevoke(device)}
        />
      )}
    </View>
  );
}

/**
 * Settings → signed-in devices (AC-1.4, follow-up of T-24): lists the account's device sessions, revokes one,
 * and logs out everywhere after a confirm. The server decides what is revocable (a foreign or stale id is
 * 404); this screen only hides the revoke button on the current device, which logs out instead.
 */
export function SessionsScreen({
  api,
  signOutEverywhere,
  onBack,
  now = Date.now,
}: SessionsScreenProps) {
  const theme = useTheme();
  const toast = useToast();
  const queryClient = useQueryClient();
  const [confirming, setConfirming] = useState(false);
  const sessions = useQuery({ queryKey: SESSIONS_QUERY_KEY, queryFn: () => api.list() });

  const revoke = useMutation({
    mutationFn: (device: DeviceSession) => api.revoke(device.id),
    onSuccess: () => toast.show({ message: t('sessions.revoked'), variant: 'success' }),
    onError: (error) => {
      if (sessionEnded(error)) return;
      if (error instanceof SessionsError && error.kind === 'not_found') {
        toast.show({ message: t('sessions.alreadyGone') });
        return;
      }
      toast.show({ message: errorMessage(error), variant: 'error' });
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: SESSIONS_QUERY_KEY }),
  });

  const everywhere = useMutation({
    mutationFn: signOutEverywhere,
    onError: (error) => {
      setConfirming(false);
      if (!sessionEnded(error)) toast.show({ message: errorMessage(error), variant: 'error' });
    },
  });

  let body;
  if (sessions.isPending) {
    body = (
      <ActivityIndicator
        color={theme.color.primary.base}
        accessibilityLabel={t('sessions.loading')}
      />
    );
  } else if (sessions.isError) {
    body = (
      <View style={styles.block}>
        <Text tone="muted">{t('sessions.error.load')}</Text>
        <Button label={t('sessions.retry')} onPress={() => void sessions.refetch()} />
      </View>
    );
  } else {
    const at = now();
    body = (
      <View style={styles.block}>
        {sessions.data.map((device) => (
          <DeviceRow
            key={device.id}
            device={device}
            now={at}
            revoking={revoke.isPending && revoke.variables.id === device.id}
            onRevoke={(d) => revoke.mutate(d)}
          />
        ))}
      </View>
    );
  }

  return (
    <Screen title={t('sessions.title')} testID="settings-sessions">
      <Text tone="muted">{t('sessions.intro')}</Text>
      {body}
      <Button
        block
        variant="danger"
        label={t('sessions.everywhere')}
        onPress={() => setConfirming(true)}
      />
      <Button block variant="ghost" label={t('common.back')} onPress={onBack} />
      <Sheet
        visible={confirming}
        onClose={() => {
          if (!everywhere.isPending) setConfirming(false);
        }}
        title={t('sessions.everywhere.title')}
      >
        <Text>{t('sessions.everywhere.body')}</Text>
        <Button
          block
          variant="danger"
          label={t('sessions.everywhere.confirm')}
          loading={everywhere.isPending}
          onPress={() => everywhere.mutate()}
        />
        <Button
          block
          variant="ghost"
          label={t('sessions.everywhere.cancel')}
          disabled={everywhere.isPending}
          onPress={() => setConfirming(false)}
        />
      </Sheet>
    </Screen>
  );
}

const styles = StyleSheet.create({
  block: { gap: space[3] },
  row: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: space[3],
    padding: space[4],
    borderRadius: radius.md,
    borderWidth: StyleSheet.hairlineWidth,
  },
  rowText: { flex: 1, gap: space[1] },
});
