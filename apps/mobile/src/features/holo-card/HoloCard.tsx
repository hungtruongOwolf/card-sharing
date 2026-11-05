import { Canvas, RoundedRect, Shader, Skia, useClock } from '@shopify/react-native-skia';
import { useMemo } from 'react';
import { StyleSheet, View } from 'react-native';
import { Gesture, GestureDetector } from 'react-native-gesture-handler';
import {
  ReduceMotion,
  useDerivedValue,
  useReducedMotion,
  useSharedValue,
  withSpring,
} from 'react-native-reanimated';

import { radius, spring } from '@card-sharing/tokens';

import { t } from '../../i18n';
import { Text } from '../../ui';
import { HOLO_SKSL } from './holoShader';

// Compiled once per process; null means the SkSL failed to compile.
const holoEffect = Skia.RuntimeEffect.Make(HOLO_SKSL);

type Props = { width: number; height: number };

const springBack = { ...spring.gentle, reduceMotion: ReduceMotion.System };

export function HoloCard({ width, height }: Props) {
  const reducedMotion = useReducedMotion();
  const clock = useClock();
  const tiltX = useSharedValue(0);
  const tiltY = useSharedValue(0);

  const pan = useMemo(
    () =>
      Gesture.Pan()
        .onUpdate((event) => {
          tiltX.set(Math.max(-1, Math.min(1, event.translationX / width)));
          tiltY.set(Math.max(-1, Math.min(1, event.translationY / height)));
        })
        .onEnd(() => {
          tiltX.set(withSpring(0, springBack));
          tiltY.set(withSpring(0, springBack));
        }),
    [width, height, tiltX, tiltY],
  );

  // Runs on the UI thread; respects the OS "reduce motion" setting (no ambient animation).
  const uniforms = useDerivedValue(() => ({
    u_size: [width, height],
    u_time: reducedMotion ? 0 : clock.get() / 1000,
    u_tilt: [tiltX.get(), tiltY.get()],
  }));

  if (!holoEffect) {
    return <Text accessibilityRole="alert">{t('holo.unavailable')}</Text>;
  }

  return (
    <GestureDetector gesture={pan}>
      <View accessible accessibilityRole="image" accessibilityLabel={t('holo.label')}>
        <Canvas style={[styles.canvas, { width, height }]}>
          <RoundedRect x={0} y={0} width={width} height={height} r={radius.md}>
            <Shader source={holoEffect} uniforms={uniforms} />
          </RoundedRect>
        </Canvas>
      </View>
    </GestureDetector>
  );
}

const styles = StyleSheet.create({
  canvas: { borderRadius: radius.md },
});
