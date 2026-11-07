import { useState } from 'react';
import { StyleSheet, View } from 'react-native';

import { radius, space } from '@card-sharing/tokens';

import { isServerBusy } from '../../../data/serverBusy';
import { t } from '../../../i18n';
import { Button, Text, useTheme } from '../../../ui';
import { AuthError } from '../../auth';
import { DevSignInError, type DevSignIn, type SeedUser } from '../devSignIn';
import { FormMessage } from './FormMessage';

/**
 * "Sign in as demo user" (#306). Rendered by the sign-in screen only in development builds (`__DEV__`) and only when
 * the app wired a {@link DevSignIn}; a release build has neither. Lists the seed users the local server reports and
 * signs in as the chosen one; a server without the development sign-in answers 404 and the section says so.
 */
export function DevSignInSection({ devSignIn }: { devSignIn: DevSignIn }) {
  const theme = useTheme();
  const [users, setUsers] = useState<readonly SeedUser[] | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  async function load() {
    setMessage(null);
    setBusy('list');
    try {
      const list = await devSignIn.listSeedUsers();
      setUsers(list);
      if (list.length === 0) setMessage(t('devSignIn.empty'));
    } catch (error) {
      setMessage(
        error instanceof DevSignInError && error.kind === 'disabled'
          ? t('devSignIn.disabled')
          : error instanceof DevSignInError && error.kind === 'busy'
            ? t('error.serverBusy')
            : t('devSignIn.unreachable'),
      );
    } finally {
      setBusy(null);
    }
  }

  async function signIn(user: SeedUser) {
    setMessage(null);
    setBusy(user.handle);
    try {
      await devSignIn.signIn(user.handle);
    } catch (error) {
      setMessage(
        error instanceof AuthError && error.status === 404
          ? t('devSignIn.disabled')
          : isServerBusy(error)
            ? t('error.serverBusy')
            : t('devSignIn.failed'),
      );
    } finally {
      setBusy(null);
    }
  }

  return (
    <View testID="dev-sign-in" style={[styles.box, { borderColor: theme.color.warning.base }]}>
      <Text variant="label.md">{t('devSignIn.title')}</Text>
      <Text tone="muted">{t('devSignIn.body')}</Text>
      {users === null ? (
        <Button
          block
          variant="ghost"
          label={t('devSignIn.open')}
          loading={busy === 'list'}
          onPress={() => void load()}
        />
      ) : (
        users.map((user) => (
          <Button
            key={user.handle}
            block
            variant="secondary"
            label={
              user.minor
                ? t('devSignIn.userMinor', { name: user.displayName, handle: user.handle })
                : t('devSignIn.user', { name: user.displayName, handle: user.handle })
            }
            loading={busy === user.handle}
            disabled={busy !== null && busy !== user.handle}
            onPress={() => void signIn(user)}
          />
        ))
      )}
      <FormMessage message={message} />
    </View>
  );
}

const styles = StyleSheet.create({
  box: {
    gap: space[2],
    padding: space[3],
    borderRadius: radius.md,
    borderWidth: StyleSheet.hairlineWidth,
    borderStyle: 'dashed',
  },
});
