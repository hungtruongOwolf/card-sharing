import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { StyleSheet, Switch, View } from 'react-native';

import { space } from '@card-sharing/tokens';

import { t } from '../../i18n';
import { Button, Screen, Text, useTheme, useToast } from '../../ui';
import {
  FriendsError,
  type BlockedPerson,
  type FriendsApi,
  type PrivacySettings,
} from './friendsApi';
import {
  BLOCKS_QUERY_KEY,
  friendsErrorMessage,
  PersonRow,
  QueryBody,
  sessionEnded,
} from './shared';

export const PRIVACY_QUERY_KEY = ['privacy-settings'] as const;

export interface PrivacyScreenProps {
  api: FriendsApi;
  onBack: () => void;
}

/**
 * Settings → Privacy (US-5a, T-45, T-47): "Findable by handle" and the accounts I blocked. The switch is not
 * offered when the server says the account cannot be findable (under the region's discovery age); the copy
 * gives no reason, so it reveals nothing about age. If the server refuses anyway (403), the screen re-reads
 * the settings. Unblocking is immediate and does not restore a friendship.
 */
export function PrivacyScreen({ api, onBack }: PrivacyScreenProps) {
  const theme = useTheme();
  const toast = useToast();
  const queryClient = useQueryClient();
  const settings = useQuery({ queryKey: PRIVACY_QUERY_KEY, queryFn: () => api.privacySettings() });
  const blocks = useQuery({ queryKey: BLOCKS_QUERY_KEY, queryFn: () => api.listBlocks() });

  const findable = useMutation({
    mutationFn: (value: boolean) => api.setFindable(value),
    onSuccess: (next: PrivacySettings) => queryClient.setQueryData(PRIVACY_QUERY_KEY, next),
    onError: (error) => {
      if (sessionEnded(error)) return;
      if (error instanceof FriendsError && error.kind === 'not_allowed') {
        void queryClient.invalidateQueries({ queryKey: PRIVACY_QUERY_KEY });
        toast.show({ message: t('privacy.findable.unavailable') });
        return;
      }
      toast.show({ message: friendsErrorMessage(error), variant: 'error' });
    },
  });

  const unblock = useMutation({
    mutationFn: (blocked: BlockedPerson) => api.unblock(blocked.person.id),
    onSuccess: (_, blocked) =>
      toast.show({
        message: t('privacy.unblocked', { name: blocked.person.displayName }),
        variant: 'success',
      }),
    onError: (error) => {
      if (!sessionEnded(error))
        toast.show({ message: friendsErrorMessage(error), variant: 'error' });
    },
    onSettled: () => void queryClient.invalidateQueries({ queryKey: BLOCKS_QUERY_KEY }),
  });

  return (
    <Screen title={t('privacy.title')} testID="privacy">
      <QueryBody
        pending={settings.isPending}
        failed={settings.isError}
        loadingLabel={t('privacy.loading')}
        errorText={t('privacy.error.load')}
        onRetry={() => void settings.refetch()}
      >
        {() =>
          settings.data?.canBeFindable ? (
            <View style={styles.row}>
              <View style={styles.rowText}>
                <Text variant="label.md">{t('privacy.findable')}</Text>
                <Text tone="muted">{t('privacy.findable.hint')}</Text>
              </View>
              <Switch
                testID="findable-switch"
                accessibilityLabel={t('privacy.findable')}
                accessibilityHint={t('privacy.findable.hint')}
                value={findable.isPending ? findable.variables : settings.data.findableByHandle}
                disabled={findable.isPending}
                trackColor={{ true: theme.color.primary.base }}
                onValueChange={(value) => findable.mutate(value)}
              />
            </View>
          ) : (
            <Text tone="muted" testID="findable-unavailable">
              {t('privacy.findable.unavailable')}
            </Text>
          )
        }
      </QueryBody>
      <Text variant="label.md">{t('privacy.blocked')}</Text>
      <Text tone="muted">{t('privacy.blocked.intro')}</Text>
      <QueryBody
        pending={blocks.isPending}
        failed={blocks.isError}
        loadingLabel={t('privacy.blocked.loading')}
        errorText={t('privacy.blocked.error.load')}
        onRetry={() => void blocks.refetch()}
      >
        {() =>
          blocks.data?.length ? (
            blocks.data.map((blocked) => (
              <PersonRow
                key={blocked.person.id}
                person={blocked.person}
                testID={`blocked-${blocked.person.id}`}
              >
                <Button
                  variant="secondary"
                  label={t('privacy.unblock')}
                  accessibilityLabel={t('privacy.unblock.a11y', {
                    name: blocked.person.displayName,
                  })}
                  loading={unblock.isPending && unblock.variables.person.id === blocked.person.id}
                  onPress={() => unblock.mutate(blocked)}
                />
              </PersonRow>
            ))
          ) : (
            <Text tone="muted">{t('privacy.blocked.empty')}</Text>
          )
        }
      </QueryBody>
      <Button block variant="ghost" label={t('common.back')} onPress={onBack} />
    </Screen>
  );
}

const styles = StyleSheet.create({
  row: { flexDirection: 'row', alignItems: 'center', gap: space[3] },
  rowText: { flex: 1, gap: space[1] },
});
