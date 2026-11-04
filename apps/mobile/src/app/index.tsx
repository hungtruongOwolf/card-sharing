import { StatusBar } from 'expo-status-bar';
import { StyleSheet, Text, useColorScheme, useWindowDimensions, View } from 'react-native';

import { space, themes, typography } from '@card-sharing/tokens';

import { HoloCard } from '../features/holo-card/HoloCard';

const CARD_ASPECT = 1.4;
const MAX_CARD_WIDTH = 340;

/** Toolchain proof: Skia shader + Reanimated clock + Gesture Handler on one screen. */
export default function HomeScreen() {
  const scheme = useColorScheme() === 'dark' ? 'dark' : 'light';
  const theme = themes[scheme];
  const { width } = useWindowDimensions();
  const cardWidth = Math.min(width - space[8] * 2, MAX_CARD_WIDTH);

  return (
    <View style={[styles.screen, { backgroundColor: theme.background }]}>
      <Text style={[styles.title, { color: theme.textPrimary }]}>card-sharing</Text>
      <HoloCard width={cardWidth} height={cardWidth * CARD_ASPECT} />
      <Text style={[styles.hint, { color: theme.textSecondary }]}>Drag the card to tilt it</Text>
      <StatusBar style="auto" />
    </View>
  );
}

const styles = StyleSheet.create({
  screen: {
    flex: 1,
    alignItems: 'center',
    justifyContent: 'center',
    gap: space[6],
  },
  title: {
    fontSize: typography.fontSize.xl,
    fontWeight: typography.fontWeight.bold,
  },
  hint: {
    fontSize: typography.fontSize.sm,
  },
});
