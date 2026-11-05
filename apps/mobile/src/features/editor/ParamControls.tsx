import { Pressable, StyleSheet, Switch, View } from 'react-native';

import {
  clampNumberParam,
  type EffectEntry,
  type ParamSpec,
  type ParamValue,
} from '@card-sharing/effects-registry';
import { radius, space } from '@card-sharing/tokens';

import { t, type MessageKey } from '../../i18n';
import { Button, Text, useTheme } from '../../ui';

/** Steps across a number param's range per stepper press. */
export const NUMBER_STEPS = 20;

/** Colours offered for `color` params (uppercase #RRGGBB, as the registry validates). */
export const PARAM_COLORS: readonly { hex: string; labelKey: MessageKey }[] = [
  { hex: '#FFFFFF', labelKey: 'editor.color.white' },
  { hex: '#1A1A1A', labelKey: 'editor.color.black' },
  { hex: '#F2B705', labelKey: 'editor.color.mustard' },
  { hex: '#F7A1C4', labelKey: 'editor.color.pink' },
  { hex: '#7FB7E6', labelKey: 'editor.color.sky' },
];

/** i18n key of a param's label: `effects.param.<name>` (a catalog test checks every visible param has one). */
export const paramLabelKey = (name: string) => `effects.param.${name}` as MessageKey;
/** i18n key of an enum value: `effects.param.<name>.<value>`. */
export const paramValueKey = (name: string, value: string) =>
  `effects.param.${name}.${value}` as MessageKey;

export function numberStep(spec: Extract<ParamSpec, { type: 'number' | 'integer' }>): number {
  const step = (spec.max - spec.min) / NUMBER_STEPS;
  return spec.type === 'integer' ? Math.max(1, Math.round(step)) : step;
}

interface ControlProps<S extends ParamSpec> {
  label: string;
  spec: S;
  value: ParamValue;
  onChange: (value: ParamValue) => void;
}

/**
 * Stepper exposed to screen readers as one adjustable control (swipe up/down), with the value read out.
 * The visible buttons serve touch; a drag slider comes with the gesture work.
 */
function NumberControl({
  label,
  spec,
  value,
  onChange,
}: ControlProps<Extract<ParamSpec, { type: 'number' | 'integer' }>>) {
  const now = value as number;
  const valueText = t('editor.param.value', { label, value: Number(now.toFixed(2)) });
  const nudge = (direction: 1 | -1) =>
    onChange(clampNumberParam(spec, now + direction * numberStep(spec)));
  return (
    <View
      style={styles.row}
      accessible
      accessibilityRole="adjustable"
      accessibilityLabel={label}
      accessibilityValue={{ min: spec.min, max: spec.max, now, text: valueText }}
      accessibilityActions={[{ name: 'increment' }, { name: 'decrement' }]}
      onAccessibilityAction={(event) => {
        if (event.nativeEvent.actionName === 'increment') nudge(1);
        if (event.nativeEvent.actionName === 'decrement') nudge(-1);
      }}
    >
      <Button
        label={t('editor.param.decrease', { label })}
        variant="secondary"
        disabled={now <= spec.min}
        onPress={() => nudge(-1)}
      />
      <Text style={styles.value}>{valueText}</Text>
      <Button
        label={t('editor.param.increase', { label })}
        variant="secondary"
        disabled={now >= spec.max}
        onPress={() => nudge(1)}
      />
    </View>
  );
}

function ToggleControl({ label, value, onChange }: ControlProps<ParamSpec>) {
  return (
    <View style={styles.row}>
      <Text style={styles.value}>{label}</Text>
      <Switch accessibilityLabel={label} value={value === true} onValueChange={onChange} />
    </View>
  );
}

function EnumControl({
  name,
  label,
  spec,
  value,
  onChange,
}: ControlProps<Extract<ParamSpec, { type: 'enum' }>> & { name: string }) {
  return (
    <View style={styles.column} accessibilityRole="radiogroup" accessibilityLabel={label}>
      <Text variant="label.md">{label}</Text>
      <View style={styles.row}>
        {spec.values.map((option) => (
          <Button
            key={option}
            label={t(paramValueKey(name, option))}
            variant={option === value ? 'primary' : 'secondary'}
            accessibilityRole="radio"
            accessibilityState={{ checked: option === value }}
            onPress={() => onChange(option)}
          />
        ))}
      </View>
    </View>
  );
}

function ColorControl({ label, value, onChange }: ControlProps<ParamSpec>) {
  const theme = useTheme();
  return (
    <View style={styles.column} accessibilityRole="radiogroup" accessibilityLabel={label}>
      <Text variant="label.md">{label}</Text>
      <View style={styles.row}>
        {PARAM_COLORS.map((color) => {
          const selected = color.hex === value;
          return (
            <Pressable
              key={color.hex}
              accessibilityRole="radio"
              accessibilityLabel={t(color.labelKey)}
              accessibilityState={{ checked: selected }}
              onPress={() => onChange(color.hex)}
              style={[
                styles.swatch,
                {
                  width: theme.touchMin,
                  height: theme.touchMin,
                  backgroundColor: color.hex,
                  borderColor: selected ? theme.color.primary.base : theme.hairline,
                  borderWidth: selected ? 3 : 1,
                },
              ]}
            />
          );
        })}
      </View>
    </View>
  );
}

export interface ParamControlsProps {
  entry: EffectEntry;
  params: Readonly<Record<string, ParamValue>>;
  onChange: (params: Record<string, ParamValue>) => void;
}

/** One control per param of an entry, by param type; `ui: 'hidden'` params get none (registry data only). */
export function ParamControls({ entry, params, onChange }: ParamControlsProps) {
  return (
    <View style={styles.column}>
      {Object.entries(entry.params).map(([name, spec]) => {
        if (spec.ui === 'hidden') return null;
        const value = params[name] ?? spec.default;
        const label = t(paramLabelKey(name));
        const change = (next: ParamValue) => onChange({ ...params, [name]: next });
        const props = { label, value, onChange: change };
        if (spec.type === 'number' || spec.type === 'integer') {
          return <NumberControl key={name} spec={spec} {...props} />;
        }
        if (spec.type === 'boolean') return <ToggleControl key={name} spec={spec} {...props} />;
        if (spec.type === 'enum') {
          return <EnumControl key={name} name={name} spec={spec} {...props} />;
        }
        return <ColorControl key={name} spec={spec} {...props} />;
      })}
    </View>
  );
}

const styles = StyleSheet.create({
  column: { gap: space[2], alignSelf: 'stretch' },
  row: {
    flexDirection: 'row',
    flexWrap: 'wrap',
    alignItems: 'center',
    justifyContent: 'center',
    gap: space[2],
  },
  value: { flexShrink: 1, textAlign: 'center' },
  swatch: { borderRadius: radius.full },
});
