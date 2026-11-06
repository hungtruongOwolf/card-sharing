import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useEffect } from 'react';
import { StyleSheet, View } from 'react-native';

import { space } from '@card-sharing/tokens';

import { t } from '../../i18n';
import { Button, Screen, Text, useToast } from '../../ui';
import type { BoardMembersApi } from './boardMembersApi';
import type { BoardRepository } from './boardRepository';

export const BOARDS_QUERY_KEY = ['boards'] as const;

export interface BoardsScreenProps {
  repository: BoardRepository;
  onOpen: (id: string) => void;
  onOpenLab?: () => void;
  /** Invitations to shared boards (T-108); accept or decline here. */
  invitations?: Pick<BoardMembersApi, 'receivedInvitations' | 'accept' | 'decline'>;
}

export const RECEIVED_INVITATIONS_QUERY_KEY = ['boards', 'invitations', 'received'] as const;

/** My boards (US-18): open one or start a new personal board. */
export function BoardsScreen({ repository, onOpen, onOpenLab, invitations }: BoardsScreenProps) {
  const queryClient = useQueryClient();
  const toast = useToast();
  const query = useQuery({ queryKey: BOARDS_QUERY_KEY, queryFn: () => repository.list() });
  // Boards shared with me or made on another device (T-105); offline, the local list stays.
  useEffect(() => {
    if (!repository.refreshList) return;
    repository
      .refreshList()
      .then(() => queryClient.invalidateQueries({ queryKey: BOARDS_QUERY_KEY }))
      .catch(() => undefined);
  }, [repository, queryClient]);
  const received = useQuery({
    queryKey: RECEIVED_INVITATIONS_QUERY_KEY,
    queryFn: () => invitations!.receivedInvitations(),
    enabled: invitations !== undefined,
  });
  const gone = () => toast.show({ message: t('boards.invitations.gone'), variant: 'error' });
  // Joining needs my acceptance (AC-19.8); the server re-checks friendship, blocks and the cap.
  const accept = useMutation({
    mutationFn: (invitationId: string) => invitations!.accept(invitationId),
    onSuccess: async (boardId) => {
      await repository.refreshList?.().catch(() => undefined);
      await queryClient.invalidateQueries({ queryKey: BOARDS_QUERY_KEY });
      await queryClient.invalidateQueries({ queryKey: RECEIVED_INVITATIONS_QUERY_KEY });
      onOpen(boardId);
    },
    onError: async () => {
      gone();
      await queryClient.invalidateQueries({ queryKey: RECEIVED_INVITATIONS_QUERY_KEY });
    },
  });
  const decline = useMutation({
    mutationFn: (invitationId: string) => invitations!.decline(invitationId),
    onSettled: () => queryClient.invalidateQueries({ queryKey: RECEIVED_INVITATIONS_QUERY_KEY }),
  });
  const create = useMutation({
    mutationFn: () => repository.create(t('boards.defaultTitle')),
    onSuccess: async (board) => {
      await queryClient.invalidateQueries({ queryKey: BOARDS_QUERY_KEY });
      onOpen(board.id);
    },
    onError: () => toast.show({ message: t('boards.error.create'), variant: 'error' }),
  });

  return (
    <Screen title={t('boards.title')} testID="boards">
      <Button label={t('boards.new')} loading={create.isPending} onPress={() => create.mutate()} />
      {(received.data ?? []).length > 0 ? (
        <View style={styles.list} testID="board-invitations">
          <Text variant="label.md">{t('boards.invitations.title')}</Text>
          {(received.data ?? []).map((invitation) => (
            <View key={invitation.id} style={styles.invitation}>
              <Text>
                {t('boards.invitations.item', {
                  owner: invitation.ownerName ?? t('boards.invitations.someone'),
                  title: invitation.boardTitle,
                  count: invitation.memberCount,
                })}
              </Text>
              <View style={styles.row}>
                <Button
                  label={t('boards.invitations.accept')}
                  loading={accept.isPending && accept.variables === invitation.id}
                  onPress={() => accept.mutate(invitation.id)}
                />
                <Button
                  label={t('boards.invitations.decline')}
                  variant="ghost"
                  onPress={() => decline.mutate(invitation.id)}
                />
              </View>
            </View>
          ))}
        </View>
      ) : null}
      {query.isPending ? <Text tone="muted">{t('boards.loading')}</Text> : null}
      {query.data?.length === 0 ? <Text tone="muted">{t('boards.list.empty')}</Text> : null}
      <View style={styles.list}>
        {query.data?.map((board) => (
          <Button
            key={board.id}
            variant="secondary"
            label={t('boards.list.item', { title: board.title, count: board.pinCount })}
            onPress={() => onOpen(board.id)}
          />
        ))}
      </View>
      {onOpenLab ? (
        <Button label={t('boards.lab.open')} variant="ghost" onPress={onOpenLab} />
      ) : null}
    </Screen>
  );
}

const styles = StyleSheet.create({
  list: { gap: space[2] },
  invitation: { gap: space[1] },
  row: { flexDirection: 'row', flexWrap: 'wrap', gap: space[2] },
});
