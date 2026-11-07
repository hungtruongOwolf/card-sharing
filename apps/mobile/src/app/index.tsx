import { useRouter, type Href } from 'expo-router';
import { useState } from 'react';
import { ScrollView, StyleSheet, View } from 'react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';

import { space } from '@card-sharing/tokens';

import { ActivityButton, useActivityApi } from '../features/activity';
import { deviceFeedCache, deviceSeenStore, FeedScreen, useSeenMarks } from '../features/feed';
import { useOutbox } from '../features/outbox';
import { useRemoteConfig } from '../features/remote-config';
import { useCardsApi } from '../features/sharing';
import { t, type MessageKey } from '../i18n';
import { Button, IconButton, Sheet, Text } from '../ui';

const CAPTURE_SIZE = 72;

/** Everything that is not the feed, one tap away in the More sheet. */
const MORE_LINKS: readonly (readonly [MessageKey, Href])[] = [
  ['boards.open', '/boards'],
  ['drafts.open.home', '/drafts'],
  ['friends.open', '/friends'],
  ['privacy.open', '/settings/privacy'],
  ['notifications.open', '/settings/notifications'],
  ['feedback.open', '/settings/feedback'],
  ['sessions.open', '/settings/sessions'],
  ['dataExport.open', '/settings/data-export'],
  ['deleteAccount.open', '/settings/delete-account'],
];
const LAB_LINKS: readonly (readonly [MessageKey, Href])[] = [
  ['holo.lab.open', '/holo-lab'],
  ['cutLab.open', '/cut-lab'],
  ['effectsLab.open', '/effects-lab'],
];

/**
 * Home (sprint 9, Stories style): the feed with friends' story rings, a top bar (activity, friends, more) and one
 * big gradient button to make a card, always under the thumb.
 */
export default function HomeScreen() {
  const router = useRouter();
  const insets = useSafeAreaInsets();
  const api = useCardsApi();
  const activityApi = useActivityApi();
  const remoteConfig = useRemoteConfig();
  const outbox = useOutbox();
  const waiting = outbox?.entries.length ?? 0;
  const seen = useSeenMarks(deviceSeenStore());
  const [moreOpen, setMoreOpen] = useState(false);
  const go = (href: Href) => {
    setMoreOpen(false);
    router.push(href);
  };

  const header = (
    <View style={styles.header}>
      <Text variant="display.md" accessibilityRole="header" style={styles.title}>
        {t('app.name')}
      </Text>
      <ActivityButton compact api={activityApi} onPress={() => router.push('/activity')} />
      <IconButton
        icon="people"
        variant="surface"
        accessibilityLabel={t('friends.open')}
        onPress={() => router.push('/friends')}
      />
      <IconButton
        icon="more"
        variant="surface"
        accessibilityLabel={t('home.more')}
        accessibilityHint={t('home.more.hint')}
        onPress={() => setMoreOpen(true)}
      />
    </View>
  );

  return (
    <View style={styles.root}>
      <FeedScreen
        api={api}
        cache={deviceFeedCache()}
        reactions={remoteConfig?.reactions}
        queueReaction={
          outbox ? (cardId, kind) => outbox.outbox.enqueueReaction(cardId, kind) : undefined
        }
        seen={seen}
        header={header}
        bottomInset={CAPTURE_SIZE + space[6]}
        onOpen={(cardId) => router.push({ pathname: '/card/[id]', params: { id: cardId } })}
        onCompose={() => router.push('/capture')}
      />
      <View
        pointerEvents="box-none"
        style={[styles.dock, { paddingBottom: Math.max(insets.bottom, space[4]) }]}
      >
        <IconButton
          icon="camera"
          variant="gradient"
          size={CAPTURE_SIZE}
          accessibilityLabel={t('home.capture')}
          onPress={() => router.push('/capture')}
          testID="home-capture"
        />
      </View>
      <Sheet visible={moreOpen} onClose={() => setMoreOpen(false)} title={t('home.more')}>
        <ScrollView contentContainerStyle={styles.links}>
          {waiting > 0 ? (
            <Button
              block
              variant="secondary"
              label={t('outbox.open', { count: waiting })}
              onPress={() => go('/outbox')}
            />
          ) : null}
          {MORE_LINKS.map(([label, href]) => (
            <Button
              key={label}
              block
              variant="secondary"
              label={t(label)}
              onPress={() => go(href)}
            />
          ))}
          <Text variant="title.md" accessibilityRole="header">
            {t('home.labs')}
          </Text>
          {LAB_LINKS.map(([label, href]) => (
            <Button key={label} block variant="ghost" label={t(label)} onPress={() => go(href)} />
          ))}
        </ScrollView>
      </Sheet>
    </View>
  );
}

const styles = StyleSheet.create({
  root: { flex: 1 },
  header: { flexDirection: 'row', alignItems: 'center', gap: space[2] },
  title: { flex: 1 },
  dock: { position: 'absolute', bottom: 0, start: 0, end: 0, alignItems: 'center' },
  links: { gap: space[2], paddingBottom: space[4] },
});
