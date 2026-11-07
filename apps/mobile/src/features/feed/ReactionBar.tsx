import { useEffect, useRef } from 'react';
import { Pressable, StyleSheet, Text as RNText, View } from 'react-native';
import Animated, {
  useAnimatedStyle,
  useSharedValue,
  withSequence,
  withTiming,
} from 'react-native-reanimated';

import { motion, onMedia, radius, space } from '@card-sharing/tokens';

import { t } from '../../i18n';
import { animateTo, pressedStateProps, useTheme } from '../../ui';
import { REACTION_DISPLAY } from './reactions';

export interface ReactionBarProps {
  /** Reaction ids to offer, in the served order (unknown ids already dropped). */
  reactions: readonly string[];
  /** My current reaction on this card. */
  chosen: string | null;
  /** A reaction is on its way: the bar ignores taps. */
  busy?: boolean;
  /** `media`: glass pills over a photo (story viewer); `surface`: on a feed tile. */
  tone?: 'media' | 'surface';
  onReact: (kind: string) => void;
}

const GLYPH = 26;

/**
 * Emoji reactions as pills. Choosing one pops it (`spring.pop`) and floats a copy up and away, like a Stories quick
 * reaction; reduced motion keeps only the selected outline. Selection is also an outline and the accessibility
 * state (`selected` on native, `aria-pressed` on the web, B9), never colour alone.
 */
export function ReactionBar({
  reactions,
  chosen,
  busy = false,
  tone = 'surface',
  onReact,
}: ReactionBarProps) {
  if (reactions.length === 0) return null;
  return (
    <View style={styles.bar} accessibilityLabel={t('feed.reactions.a11y')}>
      {reactions.map((kind) => (
        <ReactionPill
          key={kind}
          kind={kind}
          chosen={chosen === kind}
          busy={busy}
          tone={tone}
          onPress={() => onReact(kind)}
        />
      ))}
    </View>
  );
}

function ReactionPill({
  kind,
  chosen,
  busy,
  tone,
  onPress,
}: {
  kind: string;
  chosen: boolean;
  busy: boolean;
  tone: 'media' | 'surface';
  onPress: () => void;
}) {
  const theme = useTheme();
  const reduced = theme.reducedMotion;
  const display = REACTION_DISPLAY[kind]!;
  const scale = useSharedValue(1);
  const burst = useSharedValue(0);
  const wasChosen = useRef(chosen);

  useEffect(() => {
    if (chosen && !wasChosen.current && !reduced) {
      scale.set(
        withSequence(
          withTiming(1.35, { duration: motion.duration.xs }),
          animateTo(1, 'pop', false),
        ),
      );
      burst.set(0);
      burst.set(withTiming(1, { duration: motion.duration.xl }));
    }
    wasChosen.current = chosen;
  }, [chosen, reduced, scale, burst]);

  const pillStyle = useAnimatedStyle(() => ({ transform: [{ scale: scale.get() }] }));
  const burstStyle = useAnimatedStyle(() => {
    const p = burst.get();
    return {
      opacity: p > 0 && p < 1 ? 1 - p : 0,
      transform: [{ translateY: -60 * p }, { scale: 1 + p }],
    };
  });

  const c = theme.color;
  const background =
    tone === 'media'
      ? chosen
        ? onMedia.glassStrong
        : onMedia.glass
      : chosen
        ? c.primary.soft
        : c.bg.paperAlt;
  const ring = chosen ? (tone === 'media' ? onMedia.ink : c.primary.base) : 'transparent';

  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={t(display.labelKey)}
      accessibilityHint={t('feed.reactions.hint')}
      accessibilityState={{ selected: chosen, disabled: busy }}
      {...pressedStateProps(chosen)}
      disabled={busy}
      onPressIn={() => {
        if (!reduced) scale.set(animateTo(0.86, 'pop', false));
      }}
      onPressOut={() => {
        if (!reduced) scale.set(animateTo(1, 'pop', false));
      }}
      onPress={onPress}
      hitSlop={4}
    >
      <Animated.View
        style={[
          styles.pill,
          {
            minWidth: theme.touchMin,
            minHeight: theme.touchMin,
            backgroundColor: background,
            borderColor: ring,
          },
          pillStyle,
        ]}
      >
        <RNText allowFontScaling={false} style={styles.glyph}>
          {display.glyph}
        </RNText>
      </Animated.View>
      <Animated.View
        pointerEvents="none"
        accessibilityElementsHidden
        importantForAccessibility="no-hide-descendants"
        style={[styles.burst, burstStyle]}
      >
        <RNText allowFontScaling={false} style={styles.glyph}>
          {display.glyph}
        </RNText>
      </Animated.View>
    </Pressable>
  );
}

const styles = StyleSheet.create({
  bar: { flexDirection: 'row', flexWrap: 'wrap', gap: space[2], alignItems: 'center' },
  pill: {
    borderRadius: radius.full,
    borderWidth: 2,
    alignItems: 'center',
    justifyContent: 'center',
    paddingHorizontal: space[2],
  },
  glyph: { fontSize: GLYPH, lineHeight: GLYPH * 1.25, textAlign: 'center' },
  burst: { ...StyleSheet.absoluteFill, alignItems: 'center', justifyContent: 'center' },
});
