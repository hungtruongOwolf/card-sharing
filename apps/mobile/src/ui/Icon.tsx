import { StyleSheet, Text as RNText } from 'react-native';

/**
 * UI icons as glyphs (no icon font or SVG dependency, so they render the same in Expo Go, native builds and on the
 * web). Icons are decorative: the control that holds one carries the accessibility label.
 */
export const ICONS = {
  close: '✕',
  back: '‹',
  forward: '›',
  more: '⋯',
  send: '➤',
  undo: '↶',
  redo: '↷',
  text: 'Aa',
  sticker: '☺',
  effects: '✦',
  cut: '✂',
  finish: '◈',
  check: '✓',
  plus: '+',
  download: '⤓',
  pause: '❚❚',
  play: '▶',
  trash: '␡',
  flag: '⚑',
  camera: '◉',
  bell: '♪',
  board: '▦',
  people: '☻',
  settings: '⚙',
  shuffle: '⇄',
  layers: '≡',
  sparkle: '✧',
} as const;
export type IconName = keyof typeof ICONS;

export interface IconProps {
  name: IconName;
  size?: number;
  color: string;
}

export function Icon({ name, size = 20, color }: IconProps) {
  return (
    <RNText
      accessible={false}
      importantForAccessibility="no"
      allowFontScaling={false}
      style={[
        styles.glyph,
        { color, fontSize: size, lineHeight: Math.round(size * 1.2), minWidth: size },
      ]}
    >
      {ICONS[name]}
    </RNText>
  );
}

const styles = StyleSheet.create({
  glyph: { textAlign: 'center', fontWeight: '700', includeFontPadding: false },
});
