import { Pressable, StyleSheet, type PressableProps } from 'react-native';
import Animated, { useAnimatedStyle, useSharedValue } from 'react-native-reanimated';

import { linearGradient, onMedia, radius } from '@card-sharing/tokens';

import { Icon, type IconName } from './Icon';
import { animateTo } from './theme/motion';
import { useTheme } from './theme/ThemeProvider';

/**
 * `glass`: translucent dark disc for controls over photos (viewer, editor); `surface`: on app surfaces;
 * `gradient`: the one primary action of a full-bleed screen (send).
 */
export type IconButtonVariant = 'glass' | 'surface' | 'gradient';

export interface IconButtonProps extends Omit<PressableProps, 'children' | 'style'> {
  icon: IconName;
  /** Required: an icon alone says nothing to a screen reader. */
  accessibilityLabel: string;
  variant?: IconButtonVariant;
  /** Diameter; never below the platform touch target. */
  size?: number;
  selected?: boolean;
}

const PRESSED_SCALE = 0.88;

/** Round icon control with a `spring.pop` press; reduced motion swaps the scale for an opacity dip. */
export function IconButton({
  icon,
  variant = 'glass',
  size,
  selected = false,
  disabled,
  accessibilityState,
  onPressIn,
  onPressOut,
  ...rest
}: IconButtonProps) {
  const theme = useTheme();
  const pressed = useSharedValue(0);
  const reduced = theme.reducedMotion;
  const diameter = Math.max(size ?? theme.touchMin, theme.touchMin);
  const c = theme.color;
  const fill =
    variant === 'gradient'
      ? { backgroundColor: c.primary.base, backgroundImage: linearGradient('primary') }
      : variant === 'surface'
        ? { backgroundColor: selected ? c.primary.soft : c.surface.raised }
        : { backgroundColor: selected ? onMedia.glassStrong : onMedia.glass };
  const ink =
    variant === 'gradient' ? c.primary.onGradient : variant === 'surface' ? c.fg.ink : onMedia.ink;
  const animated = useAnimatedStyle(() =>
    reduced
      ? { opacity: 1 - pressed.get() * 0.3 }
      : { transform: [{ scale: 1 - pressed.get() * (1 - PRESSED_SCALE) }] },
  );
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityState={{ ...accessibilityState, disabled: Boolean(disabled), selected }}
      disabled={disabled}
      hitSlop={4}
      onPressIn={(e) => {
        pressed.set(animateTo(1, 'pop', reduced));
        onPressIn?.(e);
      }}
      onPressOut={(e) => {
        pressed.set(animateTo(0, 'pop', reduced));
        onPressOut?.(e);
      }}
      style={disabled ? styles.disabled : undefined}
      {...rest}
    >
      <Animated.View
        testID="icon-button-surface"
        style={[
          styles.disc,
          { width: diameter, height: diameter },
          fill,
          selected && variant === 'glass' ? { borderColor: onMedia.ink, borderWidth: 2 } : null,
          animated,
        ]}
      >
        <Icon name={icon} color={ink} size={Math.round(diameter * 0.45)} />
      </Animated.View>
    </Pressable>
  );
}

const styles = StyleSheet.create({
  disc: { borderRadius: radius.full, alignItems: 'center', justifyContent: 'center' },
  disabled: { opacity: 0.4 },
});
