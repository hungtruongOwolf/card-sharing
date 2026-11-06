import { StyleSheet, Switch, View } from 'react-native';

import { space } from '@card-sharing/tokens';

import { t } from '../../i18n';
import { Screen, Text } from '../../ui';
import { useFeedback } from './FeedbackProvider';

/** Settings → Sounds and haptics (T-66): both on by default, persisted on the device. */
export function FeedbackSettingsScreen() {
  const { prefs, setPrefs, emit } = useFeedback();
  return (
    <Screen title={t('feedback.title')} testID="feedback-settings">
      <View style={styles.row}>
        <View style={styles.grow}>
          <Text>{t('feedback.sound')}</Text>
          <Text tone="muted">{t('feedback.sound.hint')}</Text>
        </View>
        <Switch
          accessibilityLabel={t('feedback.sound')}
          accessibilityHint={t('feedback.sound.hint')}
          value={prefs.sound}
          onValueChange={(sound) => setPrefs({ ...prefs, sound })}
        />
      </View>
      <View style={styles.row}>
        <View style={styles.grow}>
          <Text>{t('feedback.haptics')}</Text>
          <Text tone="muted">{t('feedback.haptics.hint')}</Text>
        </View>
        <Switch
          accessibilityLabel={t('feedback.haptics')}
          accessibilityHint={t('feedback.haptics.hint')}
          value={prefs.haptics}
          onValueChange={(haptics) => {
            setPrefs({ ...prefs, haptics });
            emit('toggle');
          }}
        />
      </View>
    </Screen>
  );
}

const styles = StyleSheet.create({
  row: { flexDirection: 'row', alignItems: 'center', gap: space[3] },
  grow: { flex: 1 },
});
