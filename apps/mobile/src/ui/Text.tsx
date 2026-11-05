import { Text as RNText, type TextProps } from 'react-native';

import { typeStyle, type SemanticColors, type TypeVariant } from '@card-sharing/tokens';

import { fontStyleFor } from './theme/fonts';
import { useTheme } from './theme/ThemeProvider';

export type TextTone = 'ink' | 'muted' | 'primary' | 'onPrimary' | 'danger' | 'onDanger' | 'link';

const toneColor = (c: SemanticColors, tone: TextTone): string =>
  ({
    ink: c.fg.ink,
    muted: c.fg.muted,
    primary: c.primary.base,
    onPrimary: c.primary.onBase,
    danger: c.danger.base,
    onDanger: c.danger.onBase,
    link: c.accent.sky,
  })[tone];

export interface AppTextProps extends TextProps {
  /** Type token, e.g. `body.md` (design-system.md §3.2). */
  variant?: TypeVariant;
  tone?: TextTone;
}

/**
 * Themed text. Applies the type token, scales with the OS font size up to the token's cap
 * (200 % for UI text, 1.3× for display) and aligns to the writing direction (RTL-safe).
 * Children must come from `t()`; literals are rejected by the lint rule.
 */
export function Text({ variant = 'body.md', tone = 'ink', style, ...rest }: AppTextProps) {
  const theme = useTheme();
  const t = typeStyle(variant);
  return (
    <RNText
      maxFontSizeMultiplier={t.maxFontScale}
      {...rest}
      style={[
        {
          color: toneColor(theme.color, tone),
          ...fontStyleFor(t.family, t.weight),
          fontSize: t.size,
          lineHeight: t.lineHeight,
          textTransform: t.uppercase ? 'uppercase' : 'none',
          writingDirection: 'auto',
          textAlign: 'auto',
        },
        style,
      ]}
    />
  );
}
