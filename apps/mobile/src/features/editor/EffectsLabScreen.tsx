import { Canvas, createPicture, Picture } from '@shopify/react-native-skia';
import { useMemo, useState } from 'react';
import { StyleSheet, View } from 'react-native';
import { useFrameCallback } from 'react-native-reanimated';
import { scheduleOnRN } from 'react-native-worklets';

import { registry, type EffectEntry } from '@card-sharing/effects-registry';
import { space } from '@card-sharing/tokens';

import { t, type MessageKey } from '../../i18n';
import { FrameTimeOverlay, useFrameTimeCapture } from '../../perf';
import { Button, Screen, Text } from '../../ui';
import { labScene } from '../cut-styles/labScene';
import { hasEffectRenderer, LAB_COUNTS, labEntries } from '../effects';
import { drawCard } from './cardScene';
import { styleChoice, type CardDraft } from './editorState';

/** A sticker card with only `entry` on it, centred. */
export function labDraft(entry: EffectEntry): CardDraft {
  return {
    cut: { id: 'cut.diecut', version: 1, params: {} },
    finish: null,
    effects: [styleChoice(entry, { x: 0.5, y: 0.4 })],
    wholePhoto: false,
    caption: '',
  };
}

const CELL = 110;

/**
 * Device lab for effect frame times (T-124, effects-catalog.md §1 `cost`): N copies of one effect redrawn every
 * frame at the effect clock, with the frame-time readout. Run it on the reference devices (iPhone + a mid-range
 * Android, release build) and record p95 per effect against its `cost.gpu` budget. Dev builds reach it from home.
 */
export function EffectsLabScreen() {
  const entries = useMemo(() => labEntries(registry.entries, hasEffectRenderer), []);
  const [selected, setSelected] = useState<EffectEntry | undefined>(entries[0]);
  const [count, setCount] = useState<number>(LAB_COUNTS[1]);
  const [time, setTime] = useState(0);
  const scene = useMemo(() => labScene(CELL), []);
  useFrameCallback((frame) => {
    'worklet';
    scheduleOnRN(setTime, frame.timeSinceFirstFrame / 1000);
  });
  const draft: CardDraft | null = selected ? labDraft(selected) : null;
  const picture =
    draft && scene
      ? createPicture(
          (canvas) => {
            for (let i = 0; i < count; i++) {
              canvas.save();
              canvas.translate((i % 3) * CELL, Math.floor(i / 3) * CELL);
              drawCard(canvas, registry, draft, scene, { time });
              canvas.restore();
            }
          },
          { width: CELL * 3, height: CELL * Math.ceil(count / 3) },
        )
      : null;

  return (
    <Screen title={t('effectsLab.title')} testID="effects-lab">
      <Text tone="muted">{t('effectsLab.body')}</Text>
      <CapturePanel />
      <View style={styles.row}>
        {LAB_COUNTS.map((option) => (
          <Button
            key={option}
            label={t('effectsLab.count', { count: option })}
            variant={option === count ? 'primary' : 'secondary'}
            onPress={() => setCount(option)}
          />
        ))}
      </View>
      <View style={styles.row}>
        {entries.map((entry) => (
          <Button
            key={entry.id}
            label={t(entry.name as MessageKey)}
            variant={entry.id === selected?.id ? 'primary' : 'ghost'}
            onPress={() => setSelected(entry)}
          />
        ))}
      </View>
      {picture ? (
        <Canvas style={{ width: CELL * 3, height: CELL * Math.ceil(count / 3) }}>
          <Picture picture={picture} />
        </Canvas>
      ) : null}
    </Screen>
  );
}

/** Own component so the once-per-second report re-renders only the overlay. */
function CapturePanel() {
  const { report, reset } = useFrameTimeCapture({ enabled: true });
  return <FrameTimeOverlay report={report} onReset={reset} />;
}

const styles = StyleSheet.create({
  row: { flexDirection: 'row', flexWrap: 'wrap', gap: space[2] },
});
