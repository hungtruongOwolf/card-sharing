import { StyleSheet, View } from 'react-native';
import Animated, { useAnimatedStyle, type SharedValue } from 'react-native-reanimated';

import { onMedia, radius, size, space } from '@card-sharing/tokens';

export interface StoryProgressProps {
  count: number;
  index: number;
  /** 0..1 of the current segment (UI thread). */
  progress: SharedValue<number>;
}

/** Stories progress segments: done ones full, the current one filling, the rest empty. Decorative for a11y. */
export function StoryProgress({ count, index, progress }: StoryProgressProps) {
  return (
    <View
      style={styles.row}
      testID="story-progress"
      accessibilityElementsHidden
      importantForAccessibility="no-hide-descendants"
    >
      {Array.from({ length: count }, (_, i) => (
        <View key={i} style={[styles.track, { backgroundColor: onMedia.track }]}>
          {i < index ? <View style={[styles.fill, styles.full]} /> : null}
          {i === index ? <CurrentFill progress={progress} /> : null}
        </View>
      ))}
    </View>
  );
}

function CurrentFill({ progress }: { progress: SharedValue<number> }) {
  const style = useAnimatedStyle(() => ({ width: `${Math.min(1, progress.get()) * 100}%` }));
  return <Animated.View testID="story-progress-current" style={[styles.fill, style]} />;
}

const styles = StyleSheet.create({
  row: { flexDirection: 'row', gap: space[1] },
  track: { flex: 1, height: size.progress, borderRadius: radius.full, overflow: 'hidden' },
  fill: { height: '100%', backgroundColor: onMedia.ink, borderRadius: radius.full },
  full: { width: '100%' },
});
