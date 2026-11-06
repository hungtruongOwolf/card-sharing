import { StyleSheet, View } from 'react-native';

import { radius, space } from '@card-sharing/tokens';

import { Text, useTheme } from '../../ui';

/**
 * Small mustard pill (design-system: badges use `accent.mustard` with text in `accent.onMustard` only). Decorative
 * for screen readers: the control it sits on carries the meaning in its own label.
 */
export function Badge({ text, testID }: { text: string; testID?: string }) {
  const theme = useTheme();
  return (
    <View
      testID={testID}
      accessible={false}
      importantForAccessibility="no-hide-descendants"
      accessibilityElementsHidden
      style={[styles.pill, { backgroundColor: theme.color.accent.mustard }]}
    >
      <Text variant="label.sm" style={{ color: theme.color.accent.onMustard }}>
        {text}
      </Text>
    </View>
  );
}

const styles = StyleSheet.create({
  pill: {
    alignSelf: 'flex-start',
    paddingHorizontal: space[2],
    borderRadius: radius.full,
  },
});
