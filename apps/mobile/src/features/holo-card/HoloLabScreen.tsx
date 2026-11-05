import { useClock } from '@shopify/react-native-skia';
import { useCallback, useMemo, useState } from 'react';
import { PixelRatio, StyleSheet, useWindowDimensions, View } from 'react-native';
import { useDerivedValue, useSharedValue } from 'react-native-reanimated';

import { space } from '@card-sharing/tokens';

import { t, type MessageKey } from '../../i18n';
import { FrameTimeOverlay, useFrameTimeCapture } from '../../perf';
import { Button, Screen, Text, ThemeProvider, useTheme } from '../../ui';

import type { Tilt } from './gyroTilt';
import { GyroTiltDriver } from './GyroTiltDriver';
import {
  cardSize,
  DEFAULT_HOLO_LAB_SETTINGS,
  HOLO_KNOBS,
  liveFillPixels,
  planHoloModes,
  type HoloLabSettings,
} from './holoLab';
import { LiveHoloCard } from './LiveHoloCard';

const CARD_ASPECT = 1.4;
const GRID_GAP = space[3];

/**
 * T-52 holo lab: N holo cards driven by one gravity sensor, squish on press, a maxLive cap (the rest
 * are baked), a resolution-scale knob, the reduced-motion variant and a frame-time overlay. The
 * measurement protocol is in context/raw/research/2026-11-05-holo-spike.md.
 */
export function HoloLabScreen() {
  const [settings, setSettings] = useState<HoloLabSettings>(DEFAULT_HOLO_LAB_SETTINGS);
  return (
    // Forcing reduced motion re-themes the lab only, so its buttons follow the variant too.
    <ThemeProvider reducedMotion={settings.forceReducedMotion ? true : undefined}>
      <HoloLab settings={settings} onChange={setSettings} />
    </ThemeProvider>
  );
}

interface HoloLabProps {
  settings: HoloLabSettings;
  onChange: (update: (s: HoloLabSettings) => HoloLabSettings) => void;
}

function HoloLab({ settings, onChange }: HoloLabProps) {
  const { reducedMotion } = useTheme();
  const { width: windowWidth } = useWindowDimensions();
  const [gyroAvailable, setGyroAvailable] = useState(true);
  const [lastBakeMs, setLastBakeMs] = useState<number | null>(null);

  const tilt = useSharedValue<Tilt>({ x: 0, y: 0 });
  const clock = useClock();
  const seconds = useDerivedValue(() => clock.get() / 1000);
  const stopped = useDerivedValue(() => 0);
  const time = settings.ambient && !reducedMotion ? seconds : stopped;

  const modes = useMemo(
    () => planHoloModes(settings.cardCount, settings.maxLive, reducedMotion),
    [settings.cardCount, settings.maxLive, reducedMotion],
  );
  const size = cardSize(windowWidth - space[4] * 2, settings.columns, GRID_GAP, CARD_ASPECT);
  const fillMp = liveFillPixels(modes, size, PixelRatio.get(), settings.resolutionScale) / 1e6;
  const liveCount = modes.filter((m) => m === 'live').length;
  const usesGyro = !reducedMotion && liveCount > 0;

  const onBaked = useCallback((ms: number) => setLastBakeMs(Math.round(ms)), []);
  const set = <K extends keyof HoloLabSettings>(key: K, value: HoloLabSettings[K]) =>
    onChange((s) => ({ ...s, [key]: value }));

  return (
    <Screen title={t('holo.lab.title')}>
      {usesGyro ? <GyroTiltDriver tilt={tilt} onAvailability={setGyroAvailable} /> : null}
      <Text tone="muted">{t('holo.lab.hint')}</Text>
      {settings.capture ? <CapturePanel /> : null}
      <Text>
        {t('holo.lab.summary', {
          live: liveCount,
          baked: modes.length - liveCount,
          fill: Math.round(fillMp * 100) / 100,
        })}
      </Text>
      {reducedMotion ? <Text tone="muted">{t('holo.lab.reducedMotion')}</Text> : null}
      {usesGyro && !gyroAvailable ? <Text tone="danger">{t('holo.lab.noGyro')}</Text> : null}
      {lastBakeMs !== null ? (
        <Text tone="muted">{t('holo.lab.bakeMs', { ms: lastBakeMs })}</Text>
      ) : null}

      <View style={styles.grid}>
        {modes.map((mode, i) => (
          <LiveHoloCard
            // Index keys are fine: the list is regenerated from the knobs, never reordered.
            key={i}
            width={size.width}
            height={size.height}
            mode={mode}
            tilt={tilt}
            time={time}
            resolutionScale={settings.resolutionScale}
            reducedMotion={reducedMotion}
            skiaDebug={settings.skiaDebug}
            onBaked={onBaked}
          />
        ))}
      </View>

      <KnobRow
        messageKey="holo.lab.knob.cards"
        options={HOLO_KNOBS.cardCount}
        value={settings.cardCount}
        onSelect={(v) => set('cardCount', v)}
      />
      <KnobRow
        messageKey="holo.lab.knob.maxLive"
        options={HOLO_KNOBS.maxLive}
        value={settings.maxLive}
        onSelect={(v) => set('maxLive', v)}
      />
      <KnobRow
        messageKey="holo.lab.knob.scale"
        options={HOLO_KNOBS.resolutionScale}
        value={settings.resolutionScale}
        onSelect={(v) => set('resolutionScale', v)}
      />
      <KnobRow
        messageKey="holo.lab.knob.columns"
        options={HOLO_KNOBS.columns}
        value={settings.columns}
        onSelect={(v) => set('columns', v)}
      />
      <View style={styles.row}>
        <Toggle
          messageKey="holo.lab.toggle.ambient"
          on={settings.ambient}
          onPress={() => set('ambient', !settings.ambient)}
        />
        <Toggle
          messageKey="holo.lab.toggle.reducedMotion"
          on={settings.forceReducedMotion}
          onPress={() => set('forceReducedMotion', !settings.forceReducedMotion)}
        />
        <Toggle
          messageKey="holo.lab.toggle.skiaDebug"
          on={settings.skiaDebug}
          onPress={() => set('skiaDebug', !settings.skiaDebug)}
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

/** Own component so the once-per-second report re-renders only the overlay, not the cards. */
function CapturePanel() {
  const { report, reset } = useFrameTimeCapture({ enabled: true });
  return <FrameTimeOverlay report={report} onReset={reset} />;
}

interface KnobRowProps<V extends number> {
  messageKey: MessageKey;
  options: readonly V[];
  value: number;
  onSelect: (value: V) => void;
}

function KnobRow<V extends number>({ messageKey, options, value, onSelect }: KnobRowProps<V>) {
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
  grid: { flexDirection: 'row', flexWrap: 'wrap', gap: GRID_GAP },
  knob: { gap: space[2] },
  row: { flexDirection: 'row', flexWrap: 'wrap', gap: space[2] },
});
