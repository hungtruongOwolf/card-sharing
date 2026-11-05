import { useState } from 'react';
import { Pressable, StyleSheet, Switch, View } from 'react-native';
import { Gesture, GestureDetector } from 'react-native-gesture-handler';

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

/** Slider resolution: integer params move by 1, decimal params in 1 % of their range. */
export const SLIDER_STEPS = 100;

type NumberSpec = Extract<ParamSpec, { type: 'number' | 'integer' }>;

/** The value at `ratio` (0 = start of the track, 1 = end), snapped to the slider resolution and clamped. */
export function sliderValue(spec: NumberSpec, ratio: number): number {
  const range = spec.max - spec.min;
  const step = spec.type === 'integer' ? 1 : range / SLIDER_STEPS;
  const r = Math.min(1, Math.max(0, Number.isFinite(ratio) ? ratio : 0));
  const snapped = step > 0 ? Math.round((r * range) / step) * step : 0;
  // toFixed drops float noise (0.1 + 0.2) so equal positions give equal values and no extra re-render.
  return clampNumberParam(spec, Number((spec.min + snapped).toFixed(6)));
}

export function numberStep(spec: Extract<ParamSpec, { type: 'number' | 'integer' }>): number {
  const step = (spec.max - spec.min) / NUMBER_STEPS;
  return spec.type === 'integer' ? Math.max(1, Math.round(step)) : step;
}

interface ControlProps<S extends ParamSpec> {
  label: string;
  spec: S;
  value: ParamValue;
  /** `transient`: part of a drag, recorded as one undo step when the drag ends (`onSettle`). */
  onChange: (value: ParamValue, transient?: boolean) => void;
  onSettle: () => void;
}

interface SliderProps {
  spec: NumberSpec;
  value: number;
  testID: string;
  onDrag: (value: number) => void;
  onEnd: () => void;
}

/**
 * Drag track for a number param. Touch only: screen readers use the adjustable control around it, so the track is
 * hidden from them. A touch anywhere on the track jumps there; the whole drag is one undo step. The thumb follows
 * the finger directly (no animation), so it reads the same with reduced motion.
 */
function ParamSlider({ spec, value, testID, onDrag, onEnd }: SliderProps) {
  const theme = useTheme();
  const [width, setWidth] = useState(0);
  const range = spec.max - spec.min;
  const ratio = range > 0 ? Math.min(1, Math.max(0, (value - spec.min) / range)) : 0;
  const thumb = theme.touchMin / 2;

  const moveTo = (x: number) => {
    if (width <= 0) return;
    const next = sliderValue(spec, x / width);
    // A repeat of the same value (before the re-render) is a no-op in the editor reducer.
    if (next !== value) onDrag(next);
  };
  const gesture = Gesture.Pan()
    .withTestId(testID)
    .minDistance(0)
    .runOnJS(true)
    .onBegin((event) => moveTo(event.x))
    .onUpdate((event) => moveTo(event.x))
    .onFinalize(() => onEnd());

  return (
    <GestureDetector gesture={gesture}>
      <View
        testID={testID}
        style={[styles.track, { height: theme.touchMin }]}
        onLayout={(event) => setWidth(event.nativeEvent.layout.width)}
        accessibilityElementsHidden
        importantForAccessibility="no-hide-descendants"
      >
        <View style={[styles.rail, { backgroundColor: theme.hairline }]} />
        <View
          style={[
            styles.rail,
            { width: `${ratio * 100}%`, backgroundColor: theme.color.primary.base },
          ]}
        />
        <View
          style={[
            styles.thumb,
            {
              width: thumb,
              height: thumb,
              start: ratio * width - thumb / 2,
              backgroundColor: theme.color.surface.raised,
              borderColor: theme.color.primary.base,
            },
          ]}
        />
      </View>
    </GestureDetector>
  );
}

/**
 * Drag slider plus fine-step buttons, exposed to screen readers as one adjustable control (swipe up/down) with
 * the value read out.
 */
function NumberControl({ label, spec, value, onChange, onSettle }: ControlProps<NumberSpec>) {
  const now = value as number;
  const valueText = t('editor.param.value', { label, value: Number(now.toFixed(2)) });
  const nudge = (direction: 1 | -1) =>
    onChange(clampNumberParam(spec, now + direction * numberStep(spec)));
  return (
    <View
      style={styles.column}
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
      <Text style={styles.value}>{valueText}</Text>
      <View style={styles.sliderRow}>
        <Button
          label={t('editor.param.minus')}
          accessibilityLabel={t('editor.param.decrease', { label })}
          variant="secondary"
          disabled={now <= spec.min}
          onPress={() => nudge(-1)}
        />
        <ParamSlider
          spec={spec}
          value={now}
          testID={`param-slider-${label}`}
          onDrag={(next) => onChange(next, true)}
          onEnd={onSettle}
        />
        <Button
          label={t('editor.param.plus')}
          accessibilityLabel={t('editor.param.increase', { label })}
          variant="secondary"
          disabled={now >= spec.max}
          onPress={() => nudge(1)}
        />
      </View>
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
  /** `transient`: a slider drag in progress (one undo step, closed by `onSettle`). */
  onChange: (params: Record<string, ParamValue>, transient?: boolean) => void;
  onSettle: () => void;
}

/** One control per param of an entry, by param type; `ui: 'hidden'` params get none (registry data only). */
export function ParamControls({ entry, params, onChange, onSettle }: ParamControlsProps) {
  return (
    <View style={styles.column}>
      {Object.entries(entry.params).map(([name, spec]) => {
        if (spec.ui === 'hidden') return null;
        const value = params[name] ?? spec.default;
        const label = t(paramLabelKey(name));
        const change = (next: ParamValue, transient?: boolean) =>
          onChange({ ...params, [name]: next }, transient);
        const props = { label, value, onChange: change, onSettle };
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
  sliderRow: { flexDirection: 'row', alignItems: 'center', gap: space[2] },
  track: { flex: 1, justifyContent: 'center' },
  rail: { position: 'absolute', start: 0, height: 4, borderRadius: radius.full, width: '100%' },
  thumb: { position: 'absolute', borderRadius: radius.full, borderWidth: 2 },
  swatch: { borderRadius: radius.full },
});
