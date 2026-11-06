import { useMemo, useState } from 'react';
import { StyleSheet, useWindowDimensions, View } from 'react-native';

import { fitCamera } from '@card-sharing/canvas-engine';
import { space } from '@card-sharing/tokens';

import { t, type MessageKey } from '../../i18n';
import { FrameTimeOverlay, useFrameTimeCapture } from '../../perf';
import { Button, Screen, Text, ThemeProvider, useTheme } from '../../ui';
import { BoardCanvas } from './BoardCanvas';
import {
  BOARD_LAB_KNOBS,
  bakeLabSticker,
  DEFAULT_BOARD_LAB_SETTINGS,
  LAB_AREA,
  labBoard,
  type BoardLabSettings,
} from './boardLab';
import { createTextureCache } from './textureCache';

const CANVAS_HEIGHT = 420;

/**
 * Spike #182 lab: N baked stickers plus up to `cost.maxLive` live holo pins on one board canvas, with the
 * reduced-motion variant and the frame-time overlay. Pan with one finger, pinch to zoom; culling and texture
 * levels follow the camera. Measurement protocol: context/wiki/progress/sprint-6.md (spike #182).
 */
export function BoardLabScreen({ onClose }: { onClose: () => void }) {
  const [settings, setSettings] = useState<BoardLabSettings>(DEFAULT_BOARD_LAB_SETTINGS);
  return (
    <ThemeProvider reducedMotion={settings.forceReducedMotion ? true : undefined}>
      <BoardLab settings={settings} onChange={setSettings} onClose={onClose} />
    </ThemeProvider>
  );
}

interface BoardLabProps {
  settings: BoardLabSettings;
  onChange: (update: (s: BoardLabSettings) => BoardLabSettings) => void;
  onClose: () => void;
}

function BoardLab({ settings, onChange, onClose }: BoardLabProps) {
  const { reducedMotion } = useTheme();
  const { width } = useWindowDimensions();
  const view = { width: width - space[4] * 2, height: CANVAS_HEIGHT };
  const board = useMemo(
    () => labBoard(settings.pins, settings.holo),
    [settings.pins, settings.holo],
  );
  const [textures] = useState(() =>
    createTextureCache({ load: async (url, lod) => bakeLabSticker(url, lod) }),
  );
  const set = <K extends keyof BoardLabSettings>(key: K, value: BoardLabSettings[K]) =>
    onChange((s) => ({ ...s, [key]: value }));

  return (
    <Screen title={t('boards.lab.title')}>
      <Button label={t('common.close')} variant="secondary" onPress={onClose} />
      <Text tone="muted">{t('boards.lab.hint')}</Text>
      {settings.capture ? <CapturePanel /> : null}
      {reducedMotion ? <Text tone="muted">{t('holo.lab.reducedMotion')}</Text> : null}
      <BoardCanvas
        key={`${settings.pins}-${settings.holo}`}
        pins={board.pins}
        width={view.width}
        height={view.height}
        textures={textures}
        initialCamera={fitCamera(LAB_AREA, view)}
        testID="board-lab-canvas"
      />
      <KnobRow
        messageKey="boards.lab.knob.pins"
        options={BOARD_LAB_KNOBS.pins}
        value={settings.pins}
        onSelect={(v) => set('pins', v)}
      />
      <KnobRow
        messageKey="boards.lab.knob.holo"
        options={BOARD_LAB_KNOBS.holo}
        value={settings.holo}
        onSelect={(v) => set('holo', v)}
      />
      <View style={styles.row}>
        <Toggle
          messageKey="holo.lab.toggle.reducedMotion"
          on={settings.forceReducedMotion}
          onPress={() => set('forceReducedMotion', !settings.forceReducedMotion)}
        />
        <Toggle
          messageKey="holo.lab.toggle.capture"
          on={settings.capture}
          onPress={() => set('capture', !settings.capture)}
        />
      </View>
    </Screen>
  );
}

/** Own component so the once-per-second report re-renders only the overlay, not the board. */
function CapturePanel() {
  const { report, reset } = useFrameTimeCapture({ enabled: true });
  return <FrameTimeOverlay report={report} onReset={reset} />;
}

interface KnobRowProps {
  messageKey: MessageKey;
  options: readonly number[];
  value: number;
  onSelect: (value: number) => void;
}

function KnobRow({ messageKey, options, value, onSelect }: KnobRowProps) {
  return (
    <View style={styles.knob}>
      <Text variant="label.md">{t(messageKey)}</Text>
      <View style={styles.row} accessibilityRole="radiogroup">
        {options.map((option) => (
          <Button
            key={option}
            label={t('holo.lab.option', { value: option })}
            variant={option === value ? 'primary' : 'secondary'}
            accessibilityRole="radio"
            accessibilityState={{ selected: option === value }}
            onPress={() => onSelect(option)}
          />
        ))}
      </View>
    </View>
  );
}

interface ToggleProps {
  messageKey: MessageKey;
  on: boolean;
  onPress: () => void;
}

function Toggle({ messageKey, on, onPress }: ToggleProps) {
  return (
    <Button
      label={t('holo.lab.toggle', { name: t(messageKey), state: on ? 'on' : 'off' })}
      variant={on ? 'primary' : 'secondary'}
      accessibilityRole="switch"
      accessibilityState={{ checked: on }}
      onPress={onPress}
    />
  );
}

const styles = StyleSheet.create({
  knob: { gap: space[2] },
  row: { flexDirection: 'row', flexWrap: 'wrap', gap: space[2] },
});
