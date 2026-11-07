import { Platform, type PlatformOSType, type PressableProps } from 'react-native';

/**
 * The on/off state of a toggle button for assistive tech on the web (sprint 10 B9). Native reads
 * `accessibilityState.selected`, but react-native-web 0.21 ignores `accessibilityState` entirely, and `aria-selected`
 * is not a button state in ARIA; a toggle button announces `aria-pressed`, which react-native-web passes to the DOM.
 * Spread next to `accessibilityState` (native) on a `Pressable` with the button role.
 */
export function pressedStateProps(
  pressed: boolean,
  os: PlatformOSType = Platform.OS,
): Partial<PressableProps> {
  return os === 'web' ? ({ 'aria-pressed': pressed } as Partial<PressableProps>) : {};
}
