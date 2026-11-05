import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { StyleSheet, View } from 'react-native';

import { radius, space } from '@card-sharing/tokens';

import { t } from '../../i18n';
import { Button, Screen, Text, useTheme, useToast } from '../../ui';
import { FriendsError, type FriendRequest, type FriendsApi } from './friendsApi';
import {
  BLOCKS_QUERY_KEY,
  ConfirmSheet,
  FRIENDS_QUERY_KEY,
  friendsErrorMessage,
  PersonRow,
  QueryBody,
  REQUESTS_QUERY_KEY,
  sessionEnded,
} from './shared';

export interface RequestsScreenProps {
  api: FriendsApi;
  onBack: () => void;
}

type Action = 'accept' | 'decline' | 'cancel' | 'block';
interface Pending {
  action: Action;
  request: FriendRequest;
}

const SUCCESS: Record<
  Action,
  'friends.accepted' | 'friends.declined' | 'friends.cancelled' | 'friends.blocked'
> = {
  accept: 'friends.accepted',
  decline: 'friends.declined',
  cancel: 'friends.cancelled',
  block: 'friends.blocked',
};

/** AC-5.5: the server marks a minor's request from an adult with no mutual friends; text, not colour only. */
function MinorHint() {
  const theme = useTheme();
  return (
    <View
      testID="hint-adult-no-mutual"
      accessible
      accessibilityLabel={t('friends.hint.adultNoMutual.a11y')}
      style={[styles.badge, { backgroundColor: theme.color.accent.mustard }]}
    >
      <Text variant="label.sm" style={{ color: theme.color.accent.onMustard }}>
        {t('friends.hint.adultNoMutual')}
      </Text>
    </View>
  );
}

/**
 * Friend requests (US-5): incoming ones are accepted, declined (silently) or blocked after a confirm;
 * outgoing ones are cancelled. Outgoing requests always look pending: the server never says that one was
 * declined or dropped. The 20-friend cap comes back as `FRIEND_LIMIT_REACHED` and the request stays.
 */
export function RequestsScreen({ api, onBack }: RequestsScreenProps) {
  const toast = useToast();
  const queryClient = useQueryClient();
  const [blocking, setBlocking] = useState<FriendRequest | null>(null);
  const incoming = useQuery({
    queryKey: [...REQUESTS_QUERY_KEY, 'incoming'],
    queryFn: () => api.listRequests('incoming'),
  });
  const outgoing = useQuery({
    queryKey: [...REQUESTS_QUERY_KEY, 'outgoing'],
    queryFn: () => api.listRequests('outgoing'),
  });

  const act = useMutation({
    mutationFn: async ({ action, request }: Pending) => {
      if (action === 'accept') await api.accept(request.id);
      else if (action === 'decline') await api.decline(request.id);
      else if (action === 'cancel') await api.cancel(request.id);
      else await api.block(request.person.id);
    },
    onSuccess: (_, { action, request }) =>
      toast.show({
        message: t(SUCCESS[action], { name: request.person.displayName }),
        variant: 'success',
      }),
    onError: (error) => {
      if (sessionEnded(error)) return;
      const gone = error instanceof FriendsError && error.kind === 'not_found';
      toast.show({
        message: gone ? t('friends.error.requestGone') : friendsErrorMessage(error),
        variant: gone ? 'info' : 'error',
      });
    },
    onSettled: (_, __, { action }) => {
      setBlocking(null);
      void queryClient.invalidateQueries({ queryKey: REQUESTS_QUERY_KEY });
      if (action === 'accept' || action === 'block') {
        void queryClient.invalidateQueries({ queryKey: FRIENDS_QUERY_KEY });
      }
      if (action === 'block') void queryClient.invalidateQueries({ queryKey: BLOCKS_QUERY_KEY });
    },
  });

  const busy = (action: Action, request: FriendRequest) =>
    act.isPending && act.variables.action === action && act.variables.request.id === request.id;
  const run = (action: Action, request: FriendRequest) => act.mutate({ action, request });

  return (
    <Screen title={t('friends.requests.title')} testID="friend-requests">
      <Text variant="label.md">{t('friends.requests.incoming')}</Text>
      <QueryBody
        pending={incoming.isPending}
        failed={incoming.isError}
        loadingLabel={t('friends.requests.loading')}
        errorText={t('friends.requests.error.load')}
        onRetry={() => void incoming.refetch()}
      >
        {() =>
          incoming.data?.length ? (
            incoming.data.map((request) => {
              const name = request.person.displayName;
              return (
                <PersonRow
                  key={request.id}
                  person={request.person}
                  testID={`incoming-${request.id}`}
                  badge={request.adultNoMutualFriends ? <MinorHint /> : null}
                >
                  <Button
                    label={t('friends.accept')}
                    accessibilityLabel={t('friends.accept.a11y', { name })}
                    loading={busy('accept', request)}
                    onPress={() => run('accept', request)}
                  />
                  <Button
                    variant="secondary"
                    label={t('friends.decline')}
                    accessibilityLabel={t('friends.decline.a11y', { name })}
                    loading={busy('decline', request)}
                    onPress={() => run('decline', request)}
                  />
                  <Button
                    variant="ghost"
                    label={t('friends.block')}
                    accessibilityLabel={t('friends.block.a11y', { name })}
                    onPress={() => setBlocking(request)}
                  />
                </PersonRow>
              );
            })
          ) : (
            <Text tone="muted">{t('friends.requests.incoming.empty')}</Text>
          )
        }
      </QueryBody>
      <Text variant="label.md">{t('friends.requests.outgoing')}</Text>
      <QueryBody
        pending={outgoing.isPending}
        failed={outgoing.isError}
        loadingLabel={t('friends.requests.loading')}
        errorText={t('friends.requests.error.load')}
        onRetry={() => void outgoing.refetch()}
      >
        {() =>
          outgoing.data?.length ? (
            outgoing.data.map((request) => (
              <PersonRow
                key={request.id}
                person={request.person}
                testID={`outgoing-${request.id}`}
                badge={<Text tone="muted">{t('friends.requests.pending')}</Text>}
              >
                <Button
                  variant="secondary"
                  label={t('friends.cancel')}
                  accessibilityLabel={t('friends.cancel.a11y', {
                    name: request.person.displayName,
                  })}
                  loading={busy('cancel', request)}
                  onPress={() => run('cancel', request)}
                />
              </PersonRow>
            ))
          ) : (
            <Text tone="muted">{t('friends.requests.outgoing.empty')}</Text>
          )
        }
      </QueryBody>
      <Button block variant="ghost" label={t('common.back')} onPress={onBack} />
      <ConfirmSheet
        visible={blocking !== null}
        title={t('friends.block.title', { name: blocking?.person.displayName ?? '' })}
        body={t('friends.block.body')}
        confirmLabel={t('friends.block')}
        busy={act.isPending && act.variables.action === 'block'}
        onConfirm={() => {
          if (blocking) run('block', blocking);
        }}
        onCancel={() => setBlocking(null)}
      />
    </Screen>
  );
}

const styles = StyleSheet.create({
  badge: {
    alignSelf: 'flex-start',
    paddingHorizontal: space[2],
    paddingVertical: space[1],
    borderRadius: radius.sm,
  },
});
