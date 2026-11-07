import { StyleSheet, Text as RNText, View } from 'react-native';

import { linearGradient, palette, radius, size as sizes } from '@card-sharing/tokens';

import { graphemes } from '../text';
import { fontStyleFor } from './theme/fonts';
import { useTheme } from './theme/ThemeProvider';

/** `unseen`: gradient story ring (something new); `seen`: quiet hairline ring; `none`: just the face. */
export type AvatarRing = 'unseen' | 'seen' | 'none';

export interface AvatarProps {
  /** Stable id: picks the background colour, so a friend keeps one colour everywhere. */
  id: string;
  /** Display name (already from the server or `t()`); its first grapheme is the initial. */
  name: string;
  size?: number;
  ring?: AvatarRing;
  testID?: string;
}

/** Vivid fills for initials; dark ink on all of them (each passes 4.5:1 against `night.950`). */
const FILLS = [
  palette.sun[400],
  palette.tangerine[400],
  palette.pink[400],
  palette.violet[300],
  palette.sky[300],
  palette.mint[300],
] as const;

/** Deterministic colour index for an id (FNV-1a). */
export function avatarFill(id: string): string {
  let hash = 0x811c9dc5;
  for (let i = 0; i < id.length; i++) {
    hash ^= id.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193);
  }
  return FILLS[(hash >>> 0) % FILLS.length] as string;
}

/** The first grapheme of a name, upper-cased (an emoji stays one character); '?' for an empty name. */
export function initialOf(name: string): string {
  const first = graphemes(name.trim())[0];
  return first ? first.toLocaleUpperCase() : '?';
}

/**
 * A friend's face: initials on a colour picked from their id, with an optional story ring. Decorative for screen
 * readers; the row or button that holds it names the friend and says whether something is new.
 */
export function Avatar({ id, name, size = sizes.avatar.lg, ring = 'none', testID }: AvatarProps) {
  const theme = useTheme();
  const { stroke, gap } = sizes.ring;
  // With a ring the face sits inside the stroke and keeps a canvas-coloured gap (its border) to the ring.
  const inner = ring === 'none' ? size : size - stroke * 2;
  const glyph = (ring === 'none' ? inner : inner - gap * 2) * 0.42;
  const face = (
    <View
      style={[
        styles.face,
        { width: inner, height: inner, backgroundColor: avatarFill(id) },
        ring !== 'none' ? { borderWidth: gap, borderColor: theme.color.bg.canvas } : null,
      ]}
    >
      <RNText
        allowFontScaling={false}
        style={[
          styles.initial,
          { fontSize: glyph, color: palette.night[950], ...fontStyleFor('display', '700') },
        ]}
      >
        {initialOf(name)}
      </RNText>
    </View>
  );
  if (ring === 'none') {
    return (
      <View testID={testID} accessible={false} importantForAccessibility="no-hide-descendants">
        {face}
      </View>
    );
  }
  return (
    <View
      testID={testID}
      accessible={false}
      importantForAccessibility="no-hide-descendants"
      style={[
        styles.ring,
        { width: size, height: size, padding: stroke },
        ring === 'unseen'
          ? { backgroundColor: palette.pink[400], backgroundImage: linearGradient('story') }
          : { backgroundColor: theme.hairline },
      ]}
    >
      {face}
    </View>
  );
}

const styles = StyleSheet.create({
  ring: { borderRadius: radius.full, alignItems: 'center', justifyContent: 'center' },
  face: { borderRadius: radius.full, alignItems: 'center', justifyContent: 'center' },
  initial: { textAlign: 'center' },
});
