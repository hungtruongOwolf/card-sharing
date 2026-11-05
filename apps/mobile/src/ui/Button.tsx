import { ActivityIndicator, Pressable, StyleSheet, View, type PressableProps } from 'react-native';
import Animated, { useAnimatedStyle, useSharedValue } from 'react-native-reanimated';

import { radius, space } from '@card-sharing/tokens';

import { Text, type TextTone } from './Text';
import { animateTo } from './theme/motion';
import { useTheme, type Theme } from './theme/ThemeProvider';

export type ButtonVariant = 'primary' | 'secondary' | 'ghost' | 'danger';

export interface ButtonProps extends Omit<PressableProps, 'children' | 'style'> {
  /** Visible label from `t()`; also the accessibility label unless one is given. */
  label: string;
  variant?: ButtonVariant;
  /** Shows a spinner, announces "busy" and ignores presses. */
  loading?: boolean;
  /** Stretch to the parent's width. */
  block?: boolean;
}

const PRESSED_SCALE = 0.96;

interface ButtonColors {
  bg: string;
  border: string;
  fg: string;
  tone: TextTone;
}

function colors(theme: Theme, variant: ButtonVariant): ButtonColors {
  const c = theme.color;
  switch (variant) {
    case 'primary':
      return {
        bg: c.primary.base,
        border: c.primary.base,
        fg: c.primary.onBase,
        tone: 'onPrimary',
      };
    case 'danger':
      return { bg: c.danger.base, border: c.danger.base, fg: c.danger.onBase, tone: 'onDanger' };
    case 'secondary':
      return { bg: c.surface.raised, border: theme.hairline, fg: c.fg.ink, tone: 'ink' };
    case 'ghost':
      return { bg: 'transparent', border: 'transparent', fg: c.fg.ink, tone: 'ink' };
  }
}

/**
 * Pressable with a design-system "squish" (spring.snappy) on press. Reduced motion swaps the scale
 * for an opacity dip. Minimum size is the platform touch target; the label wraps at large font sizes
 * instead of truncating.
 */
export function Button({
  label,
  variant = 'primary',
  loading = false,
  block = false,
  disabled,
  accessibilityLabel,
  accessibilityState,
  onPressIn,
  onPressOut,
  ...rest
}: ButtonProps) {
  const theme = useTheme();
  const { bg, border, fg, tone } = colors(theme, variant);
  const pressed = useSharedValue(0);
  const inactive = Boolean(disabled) || loading;
  const reduced = theme.reducedMotion;

  const animatedStyle = useAnimatedStyle(() =>
    reduced
      ? { opacity: 1 - pressed.get() * 0.25 }
      : { transform: [{ scale: 1 - pressed.get() * (1 - PRESSED_SCALE) }] },
  );

  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={accessibilityLabel ?? label}
      accessibilityState={{ ...accessibilityState, disabled: inactive, busy: loading }}
      disabled={inactive}
      onPressIn={(e) => {
        pressed.set(animateTo(1, 'snappy', reduced));
        onPressIn?.(e);
      }}
      onPressOut={(e) => {
        pressed.set(animateTo(0, 'snappy', reduced));
        onPressOut?.(e);
      }}
      style={[block ? styles.block : styles.inline, disabled && !loading ? styles.disabled : null]}
      {...rest}
    >
      <Animated.View
        testID="button-surface"
        style={[
          styles.surface,
          {
            minHeight: theme.touchMin,
            minWidth: theme.touchMin,
            backgroundColor: bg,
            borderColor: border,
          },
          animatedStyle,
        ]}
      >
        {/* The label stays laid out while loading so the button keeps its size. */}
        <View style={loading ? styles.hidden : undefined}>
          <Text variant="label.md" tone={tone} style={styles.label}>
            {label}
          </Text>
        </View>
        {loading ? (
          <View style={styles.spinner} importantForAccessibility="no-hide-descendants">
            <ActivityIndicator color={fg} accessibilityElementsHidden />
          </View>
        ) : null}
      </Animated.View>
    </Pressable>
  );
}

const styles = StyleSheet.create({
  inline: { alignSelf: 'flex-start' },
  block: { alignSelf: 'stretch' },
  surface: {
    alignItems: 'center',
    justifyContent: 'center',
    paddingHorizontal: space[4],
    paddingVertical: space[2],
    borderRadius: radius.full,
    borderWidth: StyleSheet.hairlineWidth,
  },
  label: { textAlign: 'center' },
  disabled: { opacity: 0.45 },
  hidden: { opacity: 0 },
  spinner: { ...StyleSheet.absoluteFill, alignItems: 'center', justifyContent: 'center' },
});
