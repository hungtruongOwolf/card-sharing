import { StatusBar } from 'expo-status-bar';
import type { ReactNode } from 'react';
import { ScrollView, StyleSheet, View } from 'react-native';
import { SafeAreaView, type Edge } from 'react-native-safe-area-context';

import { space } from '@card-sharing/tokens';

import { Text } from './Text';
import { useTheme } from './theme/ThemeProvider';

export interface ScreenProps {
  children: ReactNode;
  /** Screen title from `t()`, exposed to screen readers as a header. */
  title?: string;
  /** Scrollable content (default). Turn off for full-bleed canvases that manage their own gestures. */
  scroll?: boolean;
  /** Safe-area edges to pad; defaults to all. */
  edges?: readonly Edge[];
  /** Centre content vertically (empty states, demos). */
  centered?: boolean;
  testID?: string;
}

/**
 * Root of every screen: canvas background, safe areas, status bar matching the scheme, screen gutter
 * (`space.4`) on the start/end edges so layouts mirror correctly in RTL. Scrolls by default so content
 * stays reachable at 200 % font size.
 */
export function Screen({
  children,
  title,
  scroll = true,
  edges = ['top', 'bottom', 'left', 'right'],
  centered = false,
  testID,
}: ScreenProps) {
  const theme = useTheme();
  const content = (
    <>
      {title ? (
        <Text variant="title.lg" accessibilityRole="header">
          {title}
        </Text>
      ) : null}
      {children}
    </>
  );
  return (
    <SafeAreaView
      testID={testID}
      edges={edges}
      style={[styles.root, { backgroundColor: theme.color.bg.canvas }]}
    >
      <StatusBar style={theme.scheme === 'dark' ? 'light' : 'dark'} />
      {scroll ? (
        <ScrollView
          contentContainerStyle={[styles.content, centered && styles.centered]}
          keyboardShouldPersistTaps="handled"
          keyboardDismissMode="interactive"
        >
          {content}
        </ScrollView>
      ) : (
        <View style={[styles.content, styles.fill, centered && styles.centered]}>{content}</View>
      )}
    </SafeAreaView>
  );
}

const styles = StyleSheet.create({
  root: { flex: 1 },
  fill: { flex: 1 },
  content: {
    flexGrow: 1,
    paddingHorizontal: space[4],
    paddingVertical: space[4],
    gap: space[4],
  },
  centered: { justifyContent: 'center', alignItems: 'center' },
});
