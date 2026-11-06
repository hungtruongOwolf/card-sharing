import { useQuery } from '@tanstack/react-query';
import { useRef, useState } from 'react';
import { StyleSheet, Switch, View } from 'react-native';

import { registry as shippedRegistry, type Registry } from '@card-sharing/effects-registry';
import { space } from '@card-sharing/tokens';

import { t } from '../../i18n';
import { Button, Screen, Text } from '../../ui';
import { AuthError } from '../auth';
import { BakeError } from '../cards/bakeError';
import type { LocalCard } from '../cards/localCard';
import type { PreparedCard } from '../cards/prepare';
import { useFeedback } from '../feedback';
import type { FriendsApi, Person } from '../friends/friendsApi';
import { FRIENDS_QUERY_KEY } from '../friends/shared';
import { CardsError, isRetryable, type CardsApi, type CardStatus, type SentCard } from './cardsApi';
import { newSendJob, runSend, SendError, withNewCardId, type SendJob } from './sendCard';

export interface SendScreenProps {
  card: LocalCard;
  cardsApi: CardsApi;
  friendsApi: FriendsApi;
  /** Bakes the card for upload; called only after the user taps Send. */
  prepare: (card: LocalCard, keepOriginal: boolean) => Promise<PreparedCard>;
  /** Drops the sent card from the drafts tray (best effort). */
  removeDraft: (id: string) => Promise<void>;
  registry?: Registry;
  /** `limits.card_recipients_max` from `/v1/config`; the server enforces its own copy. */
  recipientsMax?: number;
  /**
   * Offline outbox (T-78): a send that fails for a retryable reason (offline, server busy) is handed over and goes
   * out by itself later with the same idempotency key. Resolves false when the outbox cannot hold it.
   */
  queue?: (job: SendJob) => Promise<boolean>;
  /** How often the owner view of a just-sent card is refreshed while its content checks run. */
  statusPollMs?: number;
  onEdit: () => void;
  onOpenFriends: () => void;
  onDone: () => void;
}

const DEFAULT_RECIPIENTS_MAX = 20;
const DEFAULT_STATUS_POLL_MS = 3000;
/** About a minute of polling; after that the card keeps its neutral "on its way" state. */
const MAX_STATUS_POLLS = 20;

type Phase =
  | { step: 'compose' }
  | { step: 'sending'; job: SendJob | null }
  | { step: 'failed'; job: SendJob | null; error: unknown }
  | { step: 'sent'; sent: SentCard }
  | { step: 'queued' };

const sessionEnded = (error: unknown): boolean =>
  error instanceof AuthError && (error.kind === 'session_ended' || error.kind === 'signed_out');

function sendErrorMessage(error: unknown): string {
  if (
    error instanceof SendError ||
    (error instanceof CardsError && error.kind === 'unknown_style')
  ) {
    return t('send.error.staleStyle');
  }
  if (error instanceof CardsError) {
    switch (error.kind) {
      case 'rate_limited':
        return t('send.error.rateLimited');
      case 'not_allowed':
        return t('send.error.notAllowed');
      case 'key_reused':
        return t('send.error.changed');
      case 'invalid':
      case 'too_large':
      case 'unsupported':
      case 'not_found':
        return t('send.error.prepare');
    }
  }
  if (error instanceof BakeError) return t('send.error.prepare');
  return t('send.error.offline');
}

/**
 * Send a card (T-77, US-13): pick the audience (all friends by default, or some of them), choose whether to keep
 * the original (off by default, AC-13.4), then tap Send. Nothing is baked or uploaded before that tap. The send is
 * one job with one idempotency key: "Try again" repeats the exact same request, so a lost answer never makes a
 * second card. Afterwards the owner's card status is shown neutrally (on its way, delivered, waiting for review).
 */
export function SendScreen({
  card,
  cardsApi,
  friendsApi,
  prepare,
  removeDraft,
  registry = shippedRegistry,
  recipientsMax = DEFAULT_RECIPIENTS_MAX,
  statusPollMs = DEFAULT_STATUS_POLL_MS,
  queue,
  onEdit,
  onOpenFriends,
  onDone,
}: SendScreenProps) {
  const feedback = useFeedback();
  const friends = useQuery({
    queryKey: FRIENDS_QUERY_KEY,
    queryFn: () => friendsApi.listFriends(),
  });
  const [mode, setMode] = useState<'all' | 'subset'>('all');
  const [picked, setPicked] = useState<readonly string[]>([]);
  const [keepOriginal, setKeepOriginal] = useState(false);
  const [phase, setPhase] = useState<Phase>({ step: 'compose' });
  const running = useRef(false);

  const run = async (start: SendJob | null) => {
    if (running.current) return;
    running.current = true;
    let job = start;
    setPhase({ step: 'sending', job });
    try {
      job ??= newSendJob(
        card,
        registry,
        mode === 'all' ? { mode: 'all' } : { mode: 'subset', recipientIds: picked },
        keepOriginal,
      );
      const result = await runSend(job, {
        api: cardsApi,
        prepare: (keep) => prepare(card, keep),
        onProgress: (next) => {
          job = next;
        },
      });
      feedback.emit('sendSuccess');
      setPhase({ step: 'sent', sent: result.sent });
      removeDraft(card.id).catch(() => undefined);
    } catch (error) {
      if (sessionEnded(error)) return;
      if (job && queue && isRetryable(error) && (await queue(job).catch(() => false))) {
        setPhase({ step: 'queued' });
        return;
      }
      feedback.emit('error');
      // A job that never got built (stale style) can be composed again; any other job is kept for the retry.
      setPhase({ step: 'failed', job, error });
    } finally {
      running.current = false;
    }
  };

  if (phase.step === 'queued') {
    return (
      <Screen title={t('send.title')} testID="send-queued">
        <Text accessibilityRole="alert">{t('send.queued')}</Text>
        <Text tone="muted">{t('send.queued.body')}</Text>
        <Button block label={t('send.done')} onPress={onDone} />
      </Screen>
    );
  }

  if (phase.step === 'sent') {
    return <SentStatus sent={phase.sent} api={cardsApi} pollMs={statusPollMs} onDone={onDone} />;
  }

  const busy = phase.step === 'sending';
  // A retryable failure keeps its job: the retry must be the same request, so the choices stay as they were.
  const retrying = phase.step === 'failed' && phase.job !== null && isRetryable(phase.error);
  const locked = retrying;
  const list = friends.data ?? [];
  const tooMany = mode === 'subset' && picked.length > recipientsMax;
  const canSend = !busy && list.length > 0 && (mode === 'all' || (picked.length > 0 && !tooMany));
  const toggle = (person: Person) =>
    setPicked((current) =>
      current.includes(person.id)
        ? current.filter((id) => id !== person.id)
        : [...current, person.id],
    );

  return (
    <Screen title={t('send.title')} testID="send">
      <Text tone="muted">{t('send.intro')}</Text>
      {friends.isPending ? (
        <Text tone="muted">{t('send.friends.loading')}</Text>
      ) : friends.isError ? (
        <View style={styles.block}>
          <Text tone="muted">{t('send.friends.error')}</Text>
          <Button label={t('send.retry')} onPress={() => void friends.refetch()} />
        </View>
      ) : list.length === 0 ? (
        <View style={styles.block} testID="send-no-friends">
          <Text>{t('send.friends.empty')}</Text>
          <Button label={t('send.friends.add')} onPress={onOpenFriends} />
        </View>
      ) : (
        <View style={styles.block}>
          <Text variant="label.md">{t('send.audience.title')}</Text>
          <View style={styles.row} accessibilityRole="radiogroup">
            <Button
              label={t('send.audience.all', { count: list.length })}
              variant={mode === 'all' ? 'primary' : 'secondary'}
              accessibilityRole="radio"
              accessibilityState={{ checked: mode === 'all', disabled: locked || busy }}
              disabled={locked || busy}
              onPress={() => setMode('all')}
            />
            <Button
              label={t('send.audience.some')}
              variant={mode === 'subset' ? 'primary' : 'secondary'}
              accessibilityRole="radio"
              accessibilityState={{ checked: mode === 'subset', disabled: locked || busy }}
              disabled={locked || busy}
              onPress={() => setMode('subset')}
            />
          </View>
          {mode === 'subset' ? (
            <View style={styles.row}>
              {list.map((person) => {
                const checked = picked.includes(person.id);
                return (
                  <Button
                    key={person.id}
                    testID={`send-friend-${person.id}`}
                    label={person.displayName}
                    accessibilityLabel={t('send.friend.a11y', { name: person.displayName })}
                    variant={checked ? 'primary' : 'secondary'}
                    accessibilityRole="checkbox"
                    accessibilityState={{ checked, disabled: locked || busy }}
                    disabled={locked || busy}
                    onPress={() => toggle(person)}
                  />
                );
              })}
            </View>
          ) : null}
          {mode === 'subset' ? (
            <Text tone={tooMany ? 'danger' : 'muted'}>
              {tooMany
                ? t('send.audience.tooMany', { max: recipientsMax })
                : t('send.audience.picked', { count: picked.length })}
            </Text>
          ) : null}
        </View>
      )}
      <View style={styles.toggle}>
        <View style={styles.grow}>
          <Text>{t('send.keepOriginal')}</Text>
          <Text tone="muted">{t('send.keepOriginal.hint')}</Text>
        </View>
        <Switch
          accessibilityLabel={t('send.keepOriginal')}
          accessibilityHint={t('send.keepOriginal.hint')}
          value={keepOriginal}
          disabled={locked || busy}
          onValueChange={setKeepOriginal}
        />
      </View>
      {phase.step === 'failed' ? (
        <View style={styles.block} testID="send-failed">
          <Text accessibilityRole="alert">{sendErrorMessage(phase.error)}</Text>
          {retrying ? (
            <Button block label={t('send.retry')} onPress={() => void run(phase.job)} />
          ) : null}
          {phase.job && phase.error instanceof CardsError && phase.error.kind === 'key_reused' ? (
            <Button
              block
              label={t('send.asNew')}
              onPress={() => void run(withNewCardId(phase.job!))}
            />
          ) : null}
        </View>
      ) : null}
      {!retrying ? (
        <Button
          block
          label={busy ? t('send.sending') : t('send.send')}
          loading={busy}
          disabled={!canSend}
          onPress={() => void run(null)}
        />
      ) : null}
      <Button block variant="ghost" label={t('send.edit')} disabled={busy} onPress={onEdit} />
    </Screen>
  );
}

const STATUS_KEY: Record<CardStatus, 'pending' | 'active' | 'held' | 'stopped'> = {
  pending_scan: 'pending',
  active: 'active',
  held_review: 'held',
  blocked: 'stopped',
  failed: 'stopped',
};

/** The owner's view of a just-sent card, refreshed while the content checks run. Copy stays neutral. */
function SentStatus({
  sent,
  api,
  pollMs,
  onDone,
}: {
  sent: SentCard;
  api: CardsApi;
  pollMs: number;
  onDone: () => void;
}) {
  const card = useQuery({
    queryKey: ['card', sent.id],
    queryFn: () => api.getCard(sent.id),
    enabled: sent.status === 'pending_scan',
    refetchInterval: (query) =>
      (query.state.data?.status ?? 'pending_scan') === 'pending_scan' &&
      query.state.dataUpdateCount < MAX_STATUS_POLLS
        ? pollMs
        : false,
    retry: false,
  });
  const status = STATUS_KEY[card.data?.status ?? sent.status];
  const count = sent.recipientCount;
  return (
    <Screen title={t('send.title')} testID={`send-status-${status}`}>
      {status === 'pending' ? (
        <>
          <Text accessibilityRole="alert">{t('send.status.pending', { count })}</Text>
          <Text tone="muted">{t('send.status.pending.body')}</Text>
        </>
      ) : status === 'active' ? (
        <Text accessibilityRole="alert">{t('send.status.active', { count })}</Text>
      ) : status === 'held' ? (
        <>
          <Text accessibilityRole="alert">{t('send.status.held')}</Text>
          <Text tone="muted">{t('send.status.held.body')}</Text>
          <Button
            block
            variant="secondary"
            label={t('send.appeal')}
            accessibilityHint={t('send.appeal.soon')}
            disabled
          />
          <Text tone="muted">{t('send.appeal.soon')}</Text>
        </>
      ) : (
        <Text accessibilityRole="alert">{t('send.status.stopped')}</Text>
      )}
      <Button block label={t('send.done')} onPress={onDone} />
    </Screen>
  );
}

const styles = StyleSheet.create({
  block: { gap: space[3] },
  row: { flexDirection: 'row', flexWrap: 'wrap', gap: space[2] },
  toggle: { flexDirection: 'row', alignItems: 'center', gap: space[3] },
  grow: { flex: 1 },
});
