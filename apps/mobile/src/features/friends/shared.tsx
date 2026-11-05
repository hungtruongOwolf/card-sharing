import type { ReactNode } from 'react';
import { StyleSheet, View } from 'react-native';

import { radius, space } from '@card-sharing/tokens';

import { t } from '../../i18n';
import { Button, Sheet, Text, useTheme } from '../../ui';
import { AuthError } from '../auth';
import { FriendsError, type Person } from './friendsApi';

export const FRIENDS_QUERY_KEY = ['friends'] as const;
export const REQUESTS_QUERY_KEY = ['friend-requests'] as const;
export const BLOCKS_QUERY_KEY = ['blocks'] as const;

/** The device is already signed out (the auth client cleared it); the stack moves to sign-in by itself. */
export const sessionEnded = (error: unknown): boolean =>
  error instanceof AuthError && (error.kind === 'session_ended' || error.kind === 'signed_out');

/** One message per error kind; the copy never says why the server refused beyond what it told us. */
export function friendsErrorMessage(error: unknown): string {
  if (error instanceof AuthError && error.status === 429) return t('friends.error.rateLimited');
  if (!(error instanceof FriendsError)) return t('friends.error.generic');
  switch (error.kind) {
    case 'friend_limit':
      return t('friends.error.friendLimit');
    case 'already_friends':
      return t('friends.error.alreadyFriends');
    case 'invite_expired':
      return t('friends.error.inviteExpired');
    case 'rate_limited':
      return t('friends.error.rateLimited');
    case 'not_found':
      return t('friends.error.gone');
    default:
      return t('friends.error.generic');
  }
}

/** A person's name with their handle under it, and the row's actions at the end. */
export function PersonRow({
  person,
  testID,
  badge,
  children,
}: {
  person: Person;
  testID: string;
  badge?: ReactNode;
  children?: ReactNode;
}) {
  const theme = useTheme();
  return (
    <View
      testID={testID}
      style={[
        styles.row,
        { backgroundColor: theme.color.surface.raised, borderColor: theme.hairline },
      ]}
    >
      <View style={styles.rowText}>
        <Text variant="label.md">{person.displayName}</Text>
        {person.handle ? (
          <Text tone="muted">{t('friends.handle', { handle: person.handle })}</Text>
        ) : null}
        {badge}
      </View>
      {children ? <View style={styles.actions}>{children}</View> : null}
    </View>
  );
}

export interface ConfirmSheetProps {
  visible: boolean;
  title: string;
  body: string;
  confirmLabel: string;
  busy: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

/** Confirm for silent, destructive actions (remove friend, block). Cancel is always one tap away. */
export function ConfirmSheet({
  visible,
  title,
  body,
  confirmLabel,
  busy,
  onConfirm,
  onCancel,
}: ConfirmSheetProps) {
  return (
    <Sheet
      visible={visible}
      title={title}
      onClose={() => {
        if (!busy) onCancel();
      }}
    >
      <Text>{body}</Text>
      <Button block variant="danger" label={confirmLabel} loading={busy} onPress={onConfirm} />
      <Button
        block
        variant="ghost"
        label={t('friends.confirm.cancel')}
        disabled={busy}
        onPress={onCancel}
      />
    </Sheet>
  );
}

/** Loading / error-with-retry / content, the same on every friends screen. */
export function QueryBody({
  pending,
  failed,
  loadingLabel,
  errorText,
  onRetry,
  children,
}: {
  pending: boolean;
  failed: boolean;
  loadingLabel: string;
  errorText: string;
  onRetry: () => void;
  children: () => ReactNode;
}) {
  const theme = useTheme();
  if (pending) {
    return (
      <View accessible accessibilityLabel={loadingLabel} style={styles.block}>
        <Text tone="muted">{loadingLabel}</Text>
        <View style={[styles.skeleton, { backgroundColor: theme.color.bg.paperAlt }]} />
      </View>
    );
  }
  if (failed) {
    return (
      <View style={styles.block}>
        <Text tone="muted">{errorText}</Text>
        <Button label={t('friends.retry')} onPress={onRetry} />
      </View>
    );
  }
  return <View style={styles.block}>{children()}</View>;
}

const styles = StyleSheet.create({
  block: { gap: space[3] },
  row: {
    flexDirection: 'row',
    flexWrap: 'wrap',
    alignItems: 'center',
    gap: space[3],
    padding: space[4],
    borderRadius: radius.md,
    borderWidth: StyleSheet.hairlineWidth,
  },
  rowText: { flex: 1, minWidth: 140, gap: space[1] },
  actions: { flexDirection: 'row', flexWrap: 'wrap', gap: space[2] },
  skeleton: { height: 64, borderRadius: radius.md },
});
