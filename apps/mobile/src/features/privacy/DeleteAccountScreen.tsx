import { useMutation } from '@tanstack/react-query';
import { StyleSheet, View } from 'react-native';

import { space } from '@card-sharing/tokens';

import { t, type MessageKey } from '../../i18n';
import { Button, Screen, Text, useToast } from '../../ui';
import type { SignInProvider } from '../onboarding/signInProviders';
import type { PendingDeletion, PrivacyApi } from './privacyApi';
import { privacyErrorMessage, sessionEnded } from './privacyMessages';
import { useReauth } from './useReauth';

/** What deletion does, in the order the server does it (AC-29.1, AC-29.2, AC-29.3). */
const CONSEQUENCES: readonly MessageKey[] = [
  'deleteAccount.consequence.logout',
  'deleteAccount.consequence.hidden',
  'deleteAccount.consequence.boards',
  'deleteAccount.consequence.grace',
  'deleteAccount.consequence.final',
];

export interface DeleteAccountScreenProps {
  api: PrivacyApi;
  /** The native sign-in sheets, used to confirm it's the account owner (re-auth). */
  providers: readonly SignInProvider[];
  /**
   * The server accepted the deletion and logged out every device, this one included. The caller clears the
   * device (sign-out also clears the widget, AC-16.2) and tells the user how to cancel.
   */
  onDeleted: (deletion: PendingDeletion) => void | Promise<void>;
  /** "Download your data first" (US-30). */
  onOpenExport: () => void;
  onBack: () => void;
}

/**
 * Settings → Delete account (US-29, T-153). Consequences first, then a re-auth with the provider the user
 * signed in with, then `POST /v1/me/deletion` with the fresh `X-Reauth` token. Nothing is decided here: the
 * server checks the re-auth token, hides the content, revokes every session and starts the 7-day grace.
 */
export function DeleteAccountScreen({
  api,
  providers,
  onDeleted,
  onOpenExport,
  onBack,
}: DeleteAccountScreenProps) {
  const toast = useToast();
  const reauth = useReauth(api, providers);

  const remove = useMutation({
    mutationFn: async () => {
      const grant = await reauth.request({
        title: t('deleteAccount.reauth.title'),
        body: t('deleteAccount.reauth.body'),
      });
      if (!grant) return null;
      return api.requestDeletion(grant);
    },
    onSuccess: async (deletion) => {
      if (deletion) await onDeleted(deletion);
    },
    onError: (error) => {
      if (!sessionEnded(error))
        toast.show({ message: privacyErrorMessage(error), variant: 'error' });
    },
  });

  return (
    <Screen title={t('deleteAccount.title')} testID="delete-account">
      <Text>{t('deleteAccount.intro')}</Text>
      <View style={styles.list} accessibilityRole="list">
        {CONSEQUENCES.map((key) => (
          <Text key={key} tone="muted">
            {`• ${t(key)}`}
          </Text>
        ))}
      </View>
      <Button
        block
        variant="secondary"
        label={t('deleteAccount.exportFirst')}
        disabled={remove.isPending}
        onPress={onOpenExport}
      />
      <Button
        block
        variant="danger"
        label={t('deleteAccount.confirm')}
        loading={remove.isPending}
        onPress={() => remove.mutate()}
      />
      <Button
        block
        variant="ghost"
        label={t('common.back')}
        disabled={remove.isPending}
        onPress={onBack}
      />
      {reauth.sheet}
    </Screen>
  );
}

const styles = StyleSheet.create({
  list: { gap: space[2] },
});
