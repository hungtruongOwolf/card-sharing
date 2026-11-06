import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { ActivityIndicator, StyleSheet, Switch, View } from 'react-native';

import { space } from '@card-sharing/tokens';

import { t, type MessageKey } from '../../i18n';
import { Button, Screen, Text, useTheme, useToast } from '../../ui';
import { AuthError } from '../auth';
import {
  NOTIFICATION_TYPES,
  NotificationsError,
  type NotificationPrefs,
  type NotificationsApi,
  type NotificationTypes,
  type QuietHours,
} from './notificationsApi';
import { shiftClockTime } from './quietHours';
import { deviceTimeZone, NOTIFICATION_PREFS_QUERY_KEY } from './timeZone';

export interface NotificationSettingsScreenProps {
  api: NotificationsApi;
  onBack: () => void;
  /** Device time-zone seam for tests. */
  timeZone?: () => string | null;
}

const TYPE_COPY: Record<keyof NotificationTypes, { labelKey: MessageKey; hintKey: MessageKey }> = {
  cardNew: { labelKey: 'notifications.type.cardNew', hintKey: 'notifications.type.cardNew.hint' },
  reaction: {
    labelKey: 'notifications.type.reaction',
    hintKey: 'notifications.type.reaction.hint',
  },
  friend: { labelKey: 'notifications.type.friend', hintKey: 'notifications.type.friend.hint' },
  cardReview: {
    labelKey: 'notifications.type.cardReview',
    hintKey: 'notifications.type.cardReview.hint',
  },
};

const sessionEnded = (error: unknown) =>
  error instanceof AuthError && (error.kind === 'session_ended' || error.kind === 'signed_out');

const saveErrorMessage = (error: unknown) => {
  if (
    (error instanceof NotificationsError && error.kind === 'rate_limited') ||
    (error instanceof AuthError && error.status === 429)
  ) {
    return t('notifications.error.rateLimited');
  }
  if (error instanceof NotificationsError && error.kind === 'invalid') {
    return t('notifications.error.invalid');
  }
  return t('notifications.error.generic');
};

const samePrefs = (a: NotificationPrefs, b: NotificationPrefs) =>
  NOTIFICATION_TYPES.every((type) => a.types[type] === b.types[type]) &&
  a.quietHours.enabled === b.quietHours.enabled &&
  a.quietHours.start === b.quietHours.start &&
  a.quietHours.end === b.quietHours.end;

function SwitchRow({
  label,
  hint,
  value,
  disabled,
  onChange,
  testID,
}: {
  label: string;
  hint: string;
  value: boolean;
  disabled: boolean;
  onChange: (value: boolean) => void;
  testID: string;
}) {
  const theme = useTheme();
  return (
    <View style={styles.row}>
      <View style={styles.rowText}>
        <Text variant="label.md">{label}</Text>
        <Text tone="muted">{hint}</Text>
      </View>
      <Switch
        testID={testID}
        accessibilityLabel={label}
        accessibilityHint={hint}
        value={value}
        disabled={disabled}
        trackColor={{ true: theme.color.primary.base }}
        onValueChange={onChange}
      />
    </View>
  );
}

/** A quiet-hours time with −/+ buttons, exposed to screen readers as one adjustable control. */
function TimeStepper({
  label,
  value,
  disabled,
  onChange,
  testID,
}: {
  label: string;
  value: string;
  disabled: boolean;
  onChange: (value: string) => void;
  testID: string;
}) {
  const nudge = (steps: number) => {
    if (!disabled) onChange(shiftClockTime(value, steps));
  };
  return (
    <View
      testID={testID}
      style={styles.row}
      accessible
      accessibilityRole="adjustable"
      accessibilityLabel={label}
      accessibilityValue={{ text: value }}
      accessibilityState={{ disabled }}
      accessibilityActions={[{ name: 'increment' }, { name: 'decrement' }]}
      onAccessibilityAction={(event) => {
        if (event.nativeEvent.actionName === 'increment') nudge(1);
        if (event.nativeEvent.actionName === 'decrement') nudge(-1);
      }}
    >
      <Text style={styles.rowText} variant="label.md">
        {t('notifications.quiet.time.value', { label, time: value })}
      </Text>
      <Button
        label={t('notifications.quiet.time.minus')}
        accessibilityLabel={t('notifications.quiet.time.earlier', { label })}
        variant="secondary"
        disabled={disabled}
        onPress={() => nudge(-1)}
      />
      <Button
        label={t('notifications.quiet.time.plus')}
        accessibilityLabel={t('notifications.quiet.time.later', { label })}
        variant="secondary"
        disabled={disabled}
        onPress={() => nudge(1)}
      />
    </View>
  );
}

/**
 * Settings → Notifications (T-87, US-31 AC-31.1): one switch per alert type and quiet hours (default 22:00–08:00,
 * applied in the device's time zone, which is sent with every save). Changes stay a draft until one `PUT` saves
 * them (the route allows 30 writes an hour). A type turned off still shows in the activity tab (AC-31.3). The
 * server enforces every rule; this screen only edits the caller's own preferences.
 */
export function NotificationSettingsScreen({
  api,
  onBack,
  timeZone = deviceTimeZone,
}: NotificationSettingsScreenProps) {
  const theme = useTheme();
  const toast = useToast();
  const queryClient = useQueryClient();
  const prefs = useQuery({ queryKey: NOTIFICATION_PREFS_QUERY_KEY, queryFn: () => api.get() });
  const [draft, setDraft] = useState<NotificationPrefs | null>(null);

  const save = useMutation({
    mutationFn: (next: NotificationPrefs) => api.put(next),
    onSuccess: (stored) => {
      queryClient.setQueryData(NOTIFICATION_PREFS_QUERY_KEY, stored);
      setDraft(null);
      toast.show({ message: t('notifications.saved'), variant: 'success' });
    },
    onError: (error) => {
      if (!sessionEnded(error)) toast.show({ message: saveErrorMessage(error), variant: 'error' });
    },
  });

  let body;
  if (prefs.isPending) {
    body = (
      <ActivityIndicator
        color={theme.color.primary.base}
        accessibilityLabel={t('notifications.loading')}
      />
    );
  } else if (prefs.isError) {
    body = (
      <View style={styles.block}>
        <Text tone="muted">{t('notifications.error.load')}</Text>
        <Button label={t('notifications.retry')} onPress={() => void prefs.refetch()} />
      </View>
    );
  } else {
    const shown = draft ?? prefs.data;
    const busy = save.isPending;
    const setTypes = (types: NotificationTypes) => setDraft({ ...shown, types });
    const setQuiet = (quietHours: QuietHours) => setDraft({ ...shown, quietHours });
    const quiet = shown.quietHours;
    const emptyWindow = quiet.enabled && quiet.start === quiet.end;
    const dirty = draft !== null && !samePrefs(draft, prefs.data);
    const zone = timeZone() ?? prefs.data.timeZone;
    body = (
      <View style={styles.block}>
        <Text variant="label.md" accessibilityRole="header">
          {t('notifications.types.title')}
        </Text>
        {NOTIFICATION_TYPES.map((type) => (
          <SwitchRow
            key={type}
            testID={`notification-type-${type}`}
            label={t(TYPE_COPY[type].labelKey)}
            hint={t(TYPE_COPY[type].hintKey)}
            value={shown.types[type]}
            disabled={busy}
            onChange={(value) => setTypes({ ...shown.types, [type]: value })}
          />
        ))}
        <Text tone="muted">{t('notifications.types.activityNote')}</Text>
        <Text variant="label.md" accessibilityRole="header">
          {t('notifications.quiet.title')}
        </Text>
        <SwitchRow
          testID="quiet-hours-switch"
          label={t('notifications.quiet.enabled')}
          hint={t('notifications.quiet.enabled.hint')}
          value={quiet.enabled}
          disabled={busy}
          onChange={(enabled) => setQuiet({ ...quiet, enabled })}
        />
        {quiet.enabled ? (
          <>
            <TimeStepper
              testID="quiet-start"
              label={t('notifications.quiet.start')}
              value={quiet.start}
              disabled={busy}
              onChange={(start) => setQuiet({ ...quiet, start })}
            />
            <TimeStepper
              testID="quiet-end"
              label={t('notifications.quiet.end')}
              value={quiet.end}
              disabled={busy}
              onChange={(end) => setQuiet({ ...quiet, end })}
            />
            {emptyWindow ? (
              <Text tone="danger" accessibilityRole="alert" testID="quiet-empty">
                {t('notifications.quiet.empty')}
              </Text>
            ) : null}
            <Text tone="muted" testID="quiet-zone">
              {zone
                ? t('notifications.quiet.zone', { zone })
                : t('notifications.quiet.zone.unknown')}
            </Text>
          </>
        ) : null}
        <Button
          block
          label={t('notifications.save')}
          disabled={!dirty || emptyWindow}
          loading={busy}
          onPress={() => save.mutate({ ...shown, timeZone: zone })}
        />
      </View>
    );
  }

  return (
    <Screen title={t('notifications.title')} testID="settings-notifications">
      <Text tone="muted">{t('notifications.intro')}</Text>
      {body}
      <Button block variant="ghost" label={t('common.back')} onPress={onBack} />
    </Screen>
  );
}

const styles = StyleSheet.create({
  block: { gap: space[3] },
  row: { flexDirection: 'row', alignItems: 'center', gap: space[3] },
  rowText: { flex: 1, gap: space[1] },
});
