import { StyleSheet, View, type StyleProp, type ViewStyle } from 'react-native';

import { radius, space } from '@card-sharing/tokens';

import { t } from '../i18n';
import { Button, Text, useTheme } from '../ui';

import type { FrameStats } from './frameStats';
import type { FrameTimeReport } from './useFrameTimeCapture';

export interface FrameTimeOverlayProps {
  report: FrameTimeReport | null;
  onReset: () => void;
  style?: StyleProp<ViewStyle>;
}

const oneDecimal = (value: number) => Math.round(value * 10) / 10;

function sessionValues(s: FrameStats) {
  return {
    seconds: Math.round(s.durationMs / 1000),
    frames: s.frames,
    p50: oneDecimal(s.p50Ms),
    p95: oneDecimal(s.p95Ms),
    p99: oneDecimal(s.p99Ms),
    max: oneDecimal(s.maxMs),
    dropped: oneDecimal(s.droppedPct),
  };
}

/**
 * Dev readout of {@link useFrameTimeCapture}: the last second and the whole session since reset.
 * Shown in the holo lab; drop it on any screen that needs a frame-time check.
 */
export function FrameTimeOverlay({ report, onReset, style }: FrameTimeOverlayProps) {
  const theme = useTheme();
  return (
    <View
      style={[
        styles.panel,
        { backgroundColor: theme.color.surface.raised, borderColor: theme.hairline },
        style,
      ]}
    >
      <Text variant="label.md">{t('perf.overlay.title')}</Text>
      {report ? (
        <>
          <Text>
            {t('perf.overlay.live', {
              fps: oneDecimal(report.live.avgFps),
              p95: oneDecimal(report.live.p95Ms),
              dropped: oneDecimal(report.live.droppedPct),
            })}
          </Text>
          <Text>{t('perf.overlay.session', sessionValues(report.session))}</Text>
        </>
      ) : (
        <Text tone="muted">{t('perf.overlay.waiting')}</Text>
      )}
      <Button label={t('perf.overlay.reset')} variant="ghost" onPress={onReset} />
    </View>
  );
}

const styles = StyleSheet.create({
  panel: {
    alignSelf: 'stretch',
    gap: space[1],
    padding: space[3],
    borderRadius: radius.md,
    borderWidth: StyleSheet.hairlineWidth,
  },
});
