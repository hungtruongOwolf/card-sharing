import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { StyleSheet, View } from 'react-native';

import { space } from '@card-sharing/tokens';

import { t } from '../../i18n';
import { Button, Sheet, Text, useToast } from '../../ui';
import { newIdempotencyKey } from '../friends/idempotencyKey';
import type { Person } from '../friends/friendsApi';
import { InviteError, type BoardMember, type BoardMembersApi } from './boardMembersApi';

/** Members plus open invitations never exceed this (prd-mvp AC-19.1: max 8 collaborators; the server decides). */
export const MAX_COLLABORATORS = 8;

export const membersQueryKey = (boardId: string) => ['boards', boardId, 'members'] as const;
export const sentInvitationsQueryKey = (boardId: string) =>
  ['boards', boardId, 'invitations'] as const;

export interface BoardMembersSheetProps {
  visible: boolean;
  boardId: string;
  role: 'owner' | 'collaborator';
  api: BoardMembersApi;
  /** My current friends (only they can be invited, AC-19.1). */
  friends: () => Promise<readonly Person[]>;
  onClose: () => void;
  /** I left the board. */
  onLeft: () => void;
}

const memberName = (member: BoardMember) =>
  member.isMe ? t('boards.members.you') : (member.displayName ?? t('boards.members.someone'));

/**
 * Members and invitations of a shared board (T-108, US-19): the owner invites friends (acceptance required, AC-19.8),
 * cancels open invitations and removes collaborators (their pins go with them, AC-19.6); a collaborator can leave.
 * Removing and leaving ask for a second tap. Members hidden by the minor rule show their display name only.
 */
export function BoardMembersSheet({
  visible,
  boardId,
  role,
  api,
  friends,
  onClose,
  onLeft,
}: BoardMembersSheetProps) {
  const queryClient = useQueryClient();
  const toast = useToast();
  const [confirming, setConfirming] = useState<string | null>(null);
  const [inviting, setInviting] = useState(false);
  const owner = role === 'owner';
  const members = useQuery({
    queryKey: membersQueryKey(boardId),
    queryFn: () => api.members(boardId),
    enabled: visible,
  });
  const invitations = useQuery({
    queryKey: sentInvitationsQueryKey(boardId),
    queryFn: () => api.sentInvitations(boardId),
    enabled: visible && owner,
  });
  const friendList = useQuery({
    queryKey: ['friends', 'list', 'for-board'],
    queryFn: friends,
    enabled: visible && owner && inviting,
  });
  const refresh = () =>
    Promise.all([
      queryClient.invalidateQueries({ queryKey: membersQueryKey(boardId) }),
      queryClient.invalidateQueries({ queryKey: sentInvitationsQueryKey(boardId) }),
    ]);
  const failed = () => toast.show({ message: t('boards.members.error'), variant: 'error' });

  const invite = useMutation({
    mutationFn: (person: Person) => api.invite(boardId, person.id, newIdempotencyKey()),
    onSuccess: async (_v, person) => {
      toast.show({ message: t('boards.members.invited', { name: person.displayName }) });
      await refresh();
    },
    onError: (error) => {
      if (error instanceof InviteError) {
        toast.show({
          message: t(
            error.reason === 'collaborator_limit'
              ? 'boards.members.limit'
              : 'boards.members.alreadyInvited',
            { max: MAX_COLLABORATORS },
          ),
          variant: 'error',
        });
      } else failed();
    },
  });
  const cancel = useMutation({
    mutationFn: (invitationId: string) => api.cancelInvitation(invitationId),
    onSuccess: refresh,
    onError: failed,
  });
  const remove = useMutation({
    mutationFn: (member: BoardMember) => api.removeMember(boardId, member.id),
    onSuccess: async (_v, member) => {
      setConfirming(null);
      if (member.isMe) {
        toast.show({ message: t('boards.members.left') });
        onLeft();
        return;
      }
      await refresh();
    },
    onError: failed,
  });

  const memberIds = new Set((members.data ?? []).map((m) => m.id));
  const invitedIds = new Set((invitations.data ?? []).map((i) => i.inviteeId));
  const invitable = (friendList.data ?? []).filter(
    (f) => !memberIds.has(f.id) && !invitedIds.has(f.id),
  );
  const seats = (members.data?.length ?? 0) - 1 + (invitations.data?.length ?? 0);
  const full = seats >= MAX_COLLABORATORS;

  const destructive = (key: string, label: string, confirmLabel: string, onConfirm: () => void) =>
    confirming === key ? (
      <Button label={confirmLabel} variant="secondary" onPress={onConfirm} />
    ) : (
      <Button label={label} variant="ghost" onPress={() => setConfirming(key)} />
    );

  return (
    <Sheet
      visible={visible}
      onClose={onClose}
      title={t('boards.members.title')}
      testID="members-sheet"
    >
      {members.isPending ? <Text tone="muted">{t('boards.members.loading')}</Text> : null}
      {members.isError ? <Text tone="danger">{t('boards.members.error')}</Text> : null}
      <View style={styles.list}>
        {(members.data ?? []).map((member) => (
          <View key={member.id} style={styles.row}>
            <Text style={styles.name}>
              {t(member.role === 'owner' ? 'boards.members.owner' : 'boards.members.collaborator', {
                name: memberName(member),
              })}
            </Text>
            {owner && member.role === 'collaborator'
              ? destructive(
                  `remove:${member.id}`,
                  t('boards.members.remove'),
                  t('boards.members.removeConfirm', { name: memberName(member) }),
                  () => remove.mutate(member),
                )
              : null}
            {!owner && member.isMe
              ? destructive(
                  'leave',
                  t('boards.members.leave'),
                  t('boards.members.leaveConfirm'),
                  () => remove.mutate(member),
                )
              : null}
          </View>
        ))}
      </View>
      {owner ? (
        <>
          {(invitations.data ?? []).length > 0 ? (
            <Text variant="label.md">{t('boards.members.pending')}</Text>
          ) : null}
          {(invitations.data ?? []).map((invitation) => (
            <View key={invitation.id} style={styles.row}>
              <Text style={styles.name}>
                {invitation.inviteeName ?? t('boards.members.someone')}
              </Text>
              <Button
                label={t('boards.members.cancelInvite')}
                variant="ghost"
                onPress={() => cancel.mutate(invitation.id)}
              />
            </View>
          ))}
          {full ? (
            <Text tone="muted">{t('boards.members.limit', { max: MAX_COLLABORATORS })}</Text>
          ) : null}
          {!full && !inviting ? (
            <Button label={t('boards.members.invite')} onPress={() => setInviting(true)} />
          ) : null}
          {!full && inviting ? (
            <View style={styles.list}>
              <Text variant="label.md">{t('boards.members.inviteTitle')}</Text>
              {friendList.isPending ? (
                <Text tone="muted">{t('boards.members.loading')}</Text>
              ) : null}
              {friendList.isSuccess && invitable.length === 0 ? (
                <Text tone="muted">{t('boards.members.noFriends')}</Text>
              ) : null}
              {invitable.map((person) => (
                <Button
                  key={person.id}
                  variant="secondary"
                  label={t('boards.members.inviteOne', { name: person.displayName })}
                  loading={invite.isPending && invite.variables?.id === person.id}
                  onPress={() => invite.mutate(person)}
                />
              ))}
            </View>
          ) : null}
        </>
      ) : null}
    </Sheet>
  );
}

const styles = StyleSheet.create({
  list: { gap: space[2] },
  row: { flexDirection: 'row', alignItems: 'center', gap: space[2], flexWrap: 'wrap' },
  name: { flexShrink: 1 },
});
