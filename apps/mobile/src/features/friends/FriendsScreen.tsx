import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';

import { t } from '../../i18n';
import { Button, Screen, Text, useToast } from '../../ui';
import type { FriendsApi, Person } from './friendsApi';
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

export interface FriendsScreenProps {
  api: FriendsApi;
  onOpenRequests: () => void;
  onOpenInvite: () => void;
  onOpenFind: () => void;
  onBack: () => void;
}

type Pending = { action: 'remove' | 'block'; person: Person } | null;

/**
 * My friends (US-6, AC-26.2): remove or block a friend after a confirm. Both are silent to the other person
 * (the server sends nothing); the copy says so. A friend that is already gone refreshes the list.
 */
export function FriendsScreen({
  api,
  onOpenRequests,
  onOpenInvite,
  onOpenFind,
  onBack,
}: FriendsScreenProps) {
  const toast = useToast();
  const queryClient = useQueryClient();
  const [pending, setPending] = useState<Pending>(null);
  const friends = useQuery({ queryKey: FRIENDS_QUERY_KEY, queryFn: () => api.listFriends() });

  const act = useMutation({
    mutationFn: ({ action, person }: NonNullable<Pending>) =>
      action === 'remove' ? api.removeFriend(person.id) : api.block(person.id),
    onSuccess: (_, { action, person }) =>
      toast.show({
        message:
          action === 'remove'
            ? t('friends.removed', { name: person.displayName })
            : t('friends.blocked', { name: person.displayName }),
        variant: 'success',
      }),
    onError: (error) => {
      if (!sessionEnded(error))
        toast.show({ message: friendsErrorMessage(error), variant: 'error' });
    },
    onSettled: (_, __, { action }) => {
      setPending(null);
      void queryClient.invalidateQueries({ queryKey: FRIENDS_QUERY_KEY });
      if (action === 'block') {
        void queryClient.invalidateQueries({ queryKey: BLOCKS_QUERY_KEY });
        void queryClient.invalidateQueries({ queryKey: REQUESTS_QUERY_KEY });
      }
    },
  });

  const name = pending?.person.displayName ?? '';
  return (
    <Screen title={t('friends.title')} testID="friends">
      <Button block label={t('friends.invite.open')} onPress={onOpenInvite} />
      <Button block variant="secondary" label={t('friends.find.open')} onPress={onOpenFind} />
      <Button
        block
        variant="secondary"
        label={t('friends.requests.open')}
        onPress={onOpenRequests}
      />
      <QueryBody
        pending={friends.isPending}
        failed={friends.isError}
        loadingLabel={t('friends.loading')}
        errorText={t('friends.error.load')}
        onRetry={() => void friends.refetch()}
      >
        {() =>
          friends.data?.length ? (
            friends.data.map((person) => (
              <PersonRow key={person.id} person={person} testID={`friend-${person.id}`}>
                <Button
                  variant="secondary"
                  label={t('friends.remove')}
                  accessibilityLabel={t('friends.remove.a11y', { name: person.displayName })}
                  onPress={() => setPending({ action: 'remove', person })}
                />
                <Button
                  variant="ghost"
                  label={t('friends.block')}
                  accessibilityLabel={t('friends.block.a11y', { name: person.displayName })}
                  onPress={() => setPending({ action: 'block', person })}
                />
              </PersonRow>
            ))
          ) : (
            <Text tone="muted">{t('friends.empty')}</Text>
          )
        }
      </QueryBody>
      <Button block variant="ghost" label={t('common.back')} onPress={onBack} />
      <ConfirmSheet
        visible={pending !== null}
        title={
          pending?.action === 'block'
            ? t('friends.block.title', { name })
            : t('friends.remove.title', { name })
        }
        body={pending?.action === 'block' ? t('friends.block.body') : t('friends.remove.body')}
        confirmLabel={pending?.action === 'block' ? t('friends.block') : t('friends.remove')}
        busy={act.isPending}
        onConfirm={() => {
          if (pending) act.mutate(pending);
        }}
        onCancel={() => setPending(null)}
      />
    </Screen>
  );
}
