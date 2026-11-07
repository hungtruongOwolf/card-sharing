import { StyleSheet, View, type StyleProp, type ViewStyle } from 'react-native';

import { linearGradient, onMedia, radius, space } from '@card-sharing/tokens';

import { t } from '../../i18n';
import { Text } from '../../ui';

export interface AiBadgeProps {
  style?: StyleProp<ViewStyle>;
  testID?: string;
}

/**
 * The visible "AI" label on AI-made images (EU AI Act Art. 50, genai-effects.md): a small gradient pill over the
 * media. It is real content, not decoration, so screen readers hear "Made with AI".
 */
export function AiBadge({ style, testID = 'ai-badge' }: AiBadgeProps) {
  return (
    <View
      testID={testID}
      accessible
      accessibilityRole="text"
      accessibilityLabel={t('ai.badge.a11y')}
      style={[styles.badge, style]}
    >
      <Text variant="label.sm" tone="onGradient">
        {t('ai.badge')}
      </Text>
    </View>
  );
}

const styles = StyleSheet.create({
  badge: {
    alignSelf: 'flex-start',
    paddingHorizontal: space[2],
    paddingVertical: space[1] / 2,
    borderRadius: radius.full,
    backgroundColor: onMedia.glassStrong,
    backgroundImage: linearGradient('primary'),
  },
});
