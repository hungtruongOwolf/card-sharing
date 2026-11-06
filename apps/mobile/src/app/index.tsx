import { useRouter } from 'expo-router';
import { useState } from 'react';
import { StyleSheet, useWindowDimensions, View } from 'react-native';

import { space } from '@card-sharing/tokens';

import { ActivityButton, useActivityApi } from '../features/activity';
import { CAPTION_MAX_GRAPHEMES as CAPTION_MAX, checkCaption } from '../features/captions';
import { HoloCard } from '../features/holo-card';
import { useOutbox } from '../features/outbox';
import { t } from '../i18n';
import { Button, Screen, Sheet, Text, TextField, useToast } from '../ui';

const CARD_ASPECT = 1.4;
const MAX_CARD_WIDTH = 340;
const DEMO_FRIEND_COUNT = 3;

/** Toolchain proof (Skia + Reanimated + Gesture Handler) and a live sample of the T-33 UI kit. */
export default function HomeScreen() {
  const { width } = useWindowDimensions();
  const toast = useToast();
  const router = useRouter();
  const [sheetOpen, setSheetOpen] = useState(false);
  const activityApi = useActivityApi();
  const waiting = useOutbox()?.entries.length ?? 0;
  const [caption, setCaption] = useState('');
  const cardWidth = Math.min(width - space[8] * 2, MAX_CARD_WIDTH);
  // Graphemes, not UTF-16 code units: an emoji counts as one character (T-69).
  const over = checkCaption(caption).length - CAPTION_MAX;

  return (
    <Screen title={t('app.name')} centered>
      <HoloCard width={cardWidth} height={cardWidth * CARD_ASPECT} />
      <Text tone="muted">{t('home.hint')}</Text>
      <Button label={t('home.capture')} onPress={() => router.push('/capture')} />
      <Button label={t('feed.open')} variant="secondary" onPress={() => router.push('/feed')} />
      <Button label={t('boards.open')} variant="secondary" onPress={() => router.push('/boards')} />
      <ActivityButton api={activityApi} onPress={() => router.push('/activity')} />
      {waiting > 0 ? (
        <Button
          label={t('outbox.open', { count: waiting })}
          variant="secondary"
          onPress={() => router.push('/outbox')}
        />
      ) : null}
      <Button
        label={t('drafts.open.home')}
        variant="secondary"
        onPress={() => router.push('/drafts')}
      />
      <Button label={t('holo.lab.open')} variant="ghost" onPress={() => router.push('/holo-lab')} />
      <Button label={t('cutLab.open')} variant="ghost" onPress={() => router.push('/cut-lab')} />
      <Button label={t('friends.open')} variant="ghost" onPress={() => router.push('/friends')} />
      <Button
        label={t('privacy.open')}
        variant="ghost"
        onPress={() => router.push('/settings/privacy')}
      />
      <Button
        label={t('feedback.open')}
        variant="ghost"
        onPress={() => router.push('/settings/feedback')}
      />
      <Button
        label={t('notifications.open')}
        variant="ghost"
        onPress={() => router.push('/settings/notifications')}
      />
      <Button
        label={t('sessions.open')}
        variant="ghost"
        onPress={() => router.push('/settings/sessions')}
      />
      <View style={styles.actions}>
        <Button
          label={t('home.editCaption')}
          variant="secondary"
          onPress={() => setSheetOpen(true)}
        />
        <Button
          label={t('home.send')}
          onPress={() =>
            toast.show({
              message: t('home.sent', { count: DEMO_FRIEND_COUNT }),
              variant: 'success',
            })
          }
        />
      </View>
      <Sheet visible={sheetOpen} onClose={() => setSheetOpen(false)} title={t('home.editCaption')}>
        <TextField
          label={t('home.caption.label')}
          placeholder={t('home.caption.placeholder')}
          value={caption}
          onChangeText={setCaption}
          hint={t('home.caption.hint', { max: CAPTION_MAX })}
          error={over > 0 ? t('home.caption.tooLong', { over }) : undefined}
        />
        <Button
          block
          label={t('home.caption.save')}
          disabled={over > 0}
          onPress={() => {
            setSheetOpen(false);
            toast.show({
              message: t('home.caption.saved'),
              action: { label: t('common.undo'), onPress: () => setCaption('') },
            });
          }}
        />
      </Sheet>
    </Screen>
  );
}

const styles = StyleSheet.create({
  actions: { flexDirection: 'row', flexWrap: 'wrap', justifyContent: 'center', gap: space[3] },
});
