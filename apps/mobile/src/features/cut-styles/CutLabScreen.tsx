import { Canvas, createPicture, Picture } from '@shopify/react-native-skia';
import { useMemo, useState } from 'react';
import { StyleSheet, useWindowDimensions, View } from 'react-native';

import {
  clampNumberParam,
  clampParams,
  registry,
  type EffectEntry,
  type ParamValue,
} from '@card-sharing/effects-registry';
import { space } from '@card-sharing/tokens';

import { t, type MessageKey } from '../../i18n';
import { Button, Screen, Text } from '../../ui';
import { drawCutStyle, hasCutRenderer } from './drawCutStyle';
import { labScene } from './labScene';

const MAX_PREVIEW = 320;
const SLIDER_STEPS = 20;

type ParamsById = Record<string, Record<string, ParamValue>>;

/**
 * Dev screen for cut styles (T-62): live preview of every registry cut style on a synthetic subject, with its
 * presets and a stepper per slider param. Built only from registry data, so a new entry appears here unchanged.
 */
export function CutLabScreen() {
  const { width } = useWindowDimensions();
  const size = Math.round(Math.min(width - space[4] * 2, MAX_PREVIEW));
  const entries = useMemo(() => registry.byKind('cutStyle').filter(hasCutRenderer), []);
  const [selectedId, setSelectedId] = useState(entries[0]?.id);
  const [paramsById, setParamsById] = useState<ParamsById>({});
  const scene = useMemo(() => labScene(size), [size]);

  const entry = entries.find((candidate) => candidate.id === selectedId);
  const storedParams = entry ? paramsById[entry.id] : undefined;
  const params = entry ? clampParams(entry, storedParams) : {};
  // Recorded per render: a handful of draw calls, and this dev screen only re-renders on taps.
  const picture =
    scene && entry
      ? createPicture((canvas) => drawCutStyle(canvas, entry, storedParams, scene), {
          width: size,
          height: size,
        })
      : null;

  const setParams = (target: EffectEntry, next: Record<string, ParamValue>) =>
    setParamsById((all) => ({ ...all, [target.id]: clampParams(target, next) }));

  return (
    <Screen title={t('cutLab.title')} testID="cut-lab">
      <Text tone="muted">{t('cutLab.hint')}</Text>
      <View
        style={[styles.preview, { width: size, height: size }]}
        accessible
        accessibilityRole="image"
        accessibilityLabel={
          entry ? t('cutLab.previewLabel', { style: t(entry.a11yLabel as MessageKey) }) : undefined
        }
      >
        {picture ? (
          <Canvas style={{ width: size, height: size }}>
            <Picture picture={picture} />
          </Canvas>
        ) : (
          <Text tone="muted">{t('cutLab.unavailable')}</Text>
        )}
      </View>
      <View style={styles.row}>
        {entries.map((candidate) => (
          <Button
            key={candidate.id}
            label={t(candidate.name as MessageKey)}
            variant={candidate.id === selectedId ? 'primary' : 'secondary'}
            accessibilityState={{ selected: candidate.id === selectedId }}
            onPress={() => setSelectedId(candidate.id)}
          />
        ))}
      </View>
      {entry?.presets?.length ? (
        <View style={styles.row}>
          {entry.presets.map((preset) => (
            <Button
              key={preset.id}
              label={t(preset.name as MessageKey)}
              variant="ghost"
              onPress={() => setParams(entry, { ...params, ...preset.params })}
            />
          ))}
        </View>
      ) : null}
      {entry
        ? Object.entries(entry.params).map(([name, spec]) => {
            if (spec.ui !== 'slider' || (spec.type !== 'number' && spec.type !== 'integer'))
              return null;
            const value = params[name] as number;
            const step = (spec.max - spec.min) / SLIDER_STEPS;
            const nudge = (direction: 1 | -1) =>
              setParams(entry, {
                ...params,
                [name]: clampNumberParam(spec, value + direction * step),
              });
            return (
              <View key={name} style={styles.row}>
                <Button
                  label={t('cutLab.decrease', { name })}
                  variant="secondary"
                  disabled={value <= spec.min}
                  onPress={() => nudge(-1)}
                />
                <Text>{t('cutLab.param', { name, value })}</Text>
                <Button
                  label={t('cutLab.increase', { name })}
                  variant="secondary"
                  disabled={value >= spec.max}
                  onPress={() => nudge(1)}
                />
              </View>
            );
          })
        : null}
    </Screen>
  );
}

const styles = StyleSheet.create({
  preview: { alignSelf: 'center', alignItems: 'center', justifyContent: 'center' },
  row: {
    flexDirection: 'row',
    flexWrap: 'wrap',
    justifyContent: 'center',
    alignItems: 'center',
    gap: space[2],
  },
});
