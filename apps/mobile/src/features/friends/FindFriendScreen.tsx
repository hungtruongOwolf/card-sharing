import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { StyleSheet, View } from 'react-native';

import { radius, space } from '@card-sharing/tokens';

import { t } from '../../i18n';
import { Button, Screen, Text, TextField, useTheme } from '../../ui';
import { FriendsError, type FriendsApi, type SearchResult } from './friendsApi';
import { newIdempotencyKey } from './idempotencyKey';
import { friendsErrorMessage, REQUESTS_QUERY_KEY, sessionEnded } from './shared';

export interface FindFriendScreenProps {
  api: FriendsApi;
  onBack: () => void;
}

const HANDLE = /^[a-z0-9._]{3,20}$/;

/** What the user typed as the server will read it: trimmed, one leading `@` dropped, lowercase. */
export const searchableHandle = (raw: string): string => raw.trim().replace(/^@/, '').toLowerCase();

/** A result with the idempotency key of its request: one key per found person, reused on retry. */
interface Found {
  readonly result: SearchResult;
  readonly key: string;
}

type Outcome = { kind: 'none' } | { kind: 'found'; found: Found } | { kind: 'sent'; name: string };

/**
 * Find a friend by exact handle (US-5a, T-44). Nothing is sent while typing (no autocomplete); a search
 * runs only on submit. "No one found" is the one answer for a missing handle and for every hidden account
 * (the server makes them identical; this screen never adds a difference). The result shows the display name
 * only and sends a request with the opaque `ref` (AC-5a.4: acceptance and caps apply). A request the server
 * silently drops looks exactly like a sent one.
 */
export function FindFriendScreen({ api, onBack }: FindFriendScreenProps) {
  const theme = useTheme();
  const queryClient = useQueryClient();
  const [input, setInput] = useState('');
  const [formatError, setFormatError] = useState(false);
  const [outcome, setOutcome] = useState<Outcome | null>(null);

  const search = useMutation({
    mutationFn: (handle: string) => api.searchHandle(handle),
    onSuccess: (result) =>
      setOutcome(
        result ? { kind: 'found', found: { result, key: newIdempotencyKey() } } : { kind: 'none' },
      ),
  });

  const send = useMutation({
    mutationFn: ({ result, key }: Found) => api.sendRequest({ searchRef: result.ref }, key),
    onSuccess: (_, { result }) => {
      setOutcome({ kind: 'sent', name: result.displayName });
      void queryClient.invalidateQueries({ queryKey: REQUESTS_QUERY_KEY });
    },
  });

  const submit = () => {
    const handle = searchableHandle(input);
    if (!HANDLE.test(handle)) {
      setFormatError(true);
      return;
    }
    setFormatError(false);
    setOutcome(null);
    send.reset();
    search.mutate(handle);
  };

  const errorText = (error: unknown) => {
    if (sessionEnded(error)) return null;
    if (error instanceof FriendsError && error.kind === 'rate_limited')
      return t('find.error.tryLater');
    if (error instanceof FriendsError && error.kind === 'not_found') return t('find.error.expired');
    return friendsErrorMessage(error);
  };

  let result = null;
  if (search.isError) {
    result = <Text tone="danger">{errorText(search.error)}</Text>;
  } else if (outcome?.kind === 'none') {
    result = <Text testID="find-none">{t('find.none')}</Text>;
  } else if (outcome?.kind === 'sent') {
    result = <Text testID="find-sent">{t('find.sent', { name: outcome.name })}</Text>;
  } else if (outcome?.kind === 'found') {
    const { found } = outcome;
    result = (
      <View
        testID="find-result"
        style={[
          styles.card,
          { backgroundColor: theme.color.surface.raised, borderColor: theme.hairline },
        ]}
      >
        <Text variant="label.md">{found.result.displayName}</Text>
        {send.isError ? <Text tone="danger">{errorText(send.error)}</Text> : null}
        <Button
          block
          label={t('find.send')}
          accessibilityLabel={t('find.send.a11y', { name: found.result.displayName })}
          loading={send.isPending}
          onPress={() => send.mutate(found)}
        />
      </View>
    );
  }

  return (
    <Screen title={t('find.title')} testID="find-friend">
      <Text tone="muted">{t('find.intro')}</Text>
      <TextField
        label={t('find.field.label')}
        placeholder={t('find.field.placeholder')}
        value={input}
        onChangeText={(text) => {
          setInput(text);
          setFormatError(false);
        }}
        error={formatError ? t('find.field.format') : undefined}
        autoCapitalize="none"
        autoCorrect={false}
        autoComplete="off"
        spellCheck={false}
        returnKeyType="search"
        onSubmitEditing={submit}
      />
      <Button
        block
        label={t('find.search')}
        disabled={input.trim().length === 0}
        loading={search.isPending}
        onPress={submit}
      />
      {result}
      <Button block variant="ghost" label={t('common.back')} onPress={onBack} />
    </Screen>
  );
}

const styles = StyleSheet.create({
  card: {
    gap: space[3],
    padding: space[4],
    borderRadius: radius.md,
    borderWidth: StyleSheet.hairlineWidth,
  },
});
