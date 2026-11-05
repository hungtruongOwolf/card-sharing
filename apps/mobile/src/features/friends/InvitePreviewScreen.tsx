import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';

import { t } from '../../i18n';
import { Button, Screen, Text } from '../../ui';
import { FriendsError, isInviteToken, type FriendsApi } from './friendsApi';
import { newIdempotencyKey } from './idempotencyKey';
import { friendsErrorMessage, QueryBody, REQUESTS_QUERY_KEY } from './shared';

export interface InvitePreviewScreenProps {
  api: FriendsApi;
  /** The token from the opened link; validated before any request. */
  token: string;
  onDone: () => void;
}

/** Unknown, blocked, reset, expired and used-up invites all read the same: nothing to tell apart. */
const unusable = (error: unknown) =>
  error instanceof FriendsError && (error.kind === 'not_found' || error.kind === 'invite_expired');

/**
 * An opened invite link (US-5, T-39): shows the inviter's display name and sends a friend request that they
 * must accept (AC-5.1). One idempotency key per opened invite, so a retry after a lost response cannot send
 * twice. Opening my own link says "That's you!" and offers nothing to send.
 */
export function InvitePreviewScreen({ api, token, onDone }: InvitePreviewScreenProps) {
  const queryClient = useQueryClient();
  const [idempotencyKey] = useState(newIdempotencyKey);
  const valid = isInviteToken(token);
  const preview = useQuery({
    queryKey: ['invite-preview', token],
    queryFn: () => api.previewInvite(token),
    enabled: valid,
    retry: false,
  });
  const send = useMutation({
    mutationFn: () => api.sendRequest({ inviteToken: token }, idempotencyKey),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: REQUESTS_QUERY_KEY }),
  });

  let body;
  if (!valid || unusable(preview.error) || unusable(send.error)) {
    body = <Text testID="invite-unusable">{t('invite.preview.unusable')}</Text>;
  } else if (send.isSuccess) {
    body = (
      <Text testID="invite-sent">
        {t('invite.preview.sent', { name: preview.data?.displayName ?? '' })}
      </Text>
    );
  } else {
    body = (
      <QueryBody
        pending={preview.isPending}
        failed={preview.isError}
        loadingLabel={t('invite.preview.loading')}
        errorText={friendsErrorMessage(preview.error)}
        onRetry={() => void preview.refetch()}
      >
        {() =>
          preview.data?.isSelf ? (
            <Text testID="invite-self">{t('invite.preview.self')}</Text>
          ) : (
            <>
              <Text variant="label.md">
                {t('invite.preview.title', { name: preview.data?.displayName ?? '' })}
              </Text>
              <Text tone="muted">{t('invite.preview.body')}</Text>
              {send.isError ? <Text tone="danger">{friendsErrorMessage(send.error)}</Text> : null}
              <Button
                block
                label={t('invite.preview.add')}
                loading={send.isPending}
                onPress={() => send.mutate()}
              />
            </>
          )
        }
      </QueryBody>
    );
  }

  return (
    <Screen title={t('invite.preview.screenTitle')} testID="invite-preview">
      {body}
      <Button block variant="ghost" label={t('common.close')} onPress={onDone} />
    </Screen>
  );
}
