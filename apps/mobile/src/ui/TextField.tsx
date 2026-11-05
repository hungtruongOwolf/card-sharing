import { forwardRef, useState } from 'react';
import { StyleSheet, TextInput, View, type TextInputProps } from 'react-native';

import { MAX_FONT_SCALE, radius, space, typeStyle } from '@card-sharing/tokens';

import { t } from '../i18n';
import { Text } from './Text';
import { fontStyleFor } from './theme/fonts';
import { useTheme } from './theme/ThemeProvider';

export interface TextFieldProps extends Omit<TextInputProps, 'style' | 'placeholderTextColor'> {
  /** Visible label from `t()`, also read by VoiceOver/TalkBack. Required: placeholders are not labels. */
  label: string;
  /** Helper text shown under the field when there is no error. */
  hint?: string;
  /** Error message; shown in danger colour, announced, and replaces the hint. */
  error?: string;
}

/** Labelled text input with hint/error, focus ring and a platform-size touch target. */
export const TextField = forwardRef<TextInput, TextFieldProps>(function TextField(
  { label, hint, error, onFocus, onBlur, editable = true, testID, ...rest },
  ref,
) {
  const theme = useTheme();
  const [focused, setFocused] = useState(false);
  const body = typeStyle('body.lg');
  const c = theme.color;
  const borderColor = error ? c.danger.base : focused ? c.primary.base : theme.hairline;

  return (
    <View style={styles.root}>
      {/* The input carries the label for screen readers; the visible one is hidden to avoid a double read. */}
      <Text
        variant="label.md"
        tone="ink"
        accessibilityElementsHidden
        importantForAccessibility="no-hide-descendants"
      >
        {label}
      </Text>
      <TextInput
        ref={ref}
        testID={testID}
        accessibilityLabel={label}
        accessibilityHint={error ? t('textField.error.a11y', { message: error }) : hint}
        accessibilityState={{ disabled: !editable }}
        editable={editable}
        maxFontSizeMultiplier={MAX_FONT_SCALE}
        placeholderTextColor={c.fg.subtle}
        selectionColor={c.primary.base}
        onFocus={(e) => {
          setFocused(true);
          onFocus?.(e);
        }}
        onBlur={(e) => {
          setFocused(false);
          onBlur?.(e);
        }}
        style={[
          styles.input,
          {
            minHeight: Math.max(theme.touchMin, 48),
            color: c.fg.ink,
            backgroundColor: c.surface.raised,
            borderColor,
            borderWidth: focused || error ? 2 : 1,
            ...fontStyleFor(body.family, body.weight),
            fontSize: body.size,
            opacity: editable ? 1 : 0.6,
          },
        ]}
        {...rest}
      />
      {error ? (
        <Text
          variant="body.md"
          tone="danger"
          accessibilityLiveRegion="polite"
          accessibilityRole="alert"
        >
          {error}
        </Text>
      ) : hint ? (
        <Text
          variant="body.md"
          tone="muted"
          accessibilityElementsHidden
          importantForAccessibility="no-hide-descendants"
        >
          {hint}
        </Text>
      ) : null}
    </View>
  );
});

const styles = StyleSheet.create({
  root: { gap: space[1], alignSelf: 'stretch' },
  input: {
    borderRadius: radius.sm,
    paddingHorizontal: space[3],
    paddingVertical: space[2],
    textAlign: 'auto',
  },
});
