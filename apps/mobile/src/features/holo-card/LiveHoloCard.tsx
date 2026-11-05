import {
  Canvas,
  drawAsImage,
  Group,
  Image,
  RoundedRect,
  Shader,
  Skia,
  type SkImage,
} from '@shopify/react-native-skia';
import { useEffect, useMemo, useState } from 'react';
import { PixelRatio, StyleSheet, View } from 'react-native';
import { Gesture, GestureDetector } from 'react-native-gesture-handler';
import Animated, {
  ReduceMotion,
  useAnimatedStyle,
  useDerivedValue,
  useSharedValue,
  withTiming,
  type DerivedValue,
  type SharedValue,
} from 'react-native-reanimated';

import { HOLO_SKSL } from '@card-sharing/shaders';
import { motion, radius } from '@card-sharing/tokens';

import { t } from '../../i18n';
import { animateTo, Text } from '../../ui';

import type { Tilt } from './gyroTilt';
import { SQUISH_SCALE, SQUISH_TILT_BUMP, STATIC_TILT, type HoloMode } from './holoLab';

// Compiled once per process; null means the SkSL failed to compile.
const holoEffect = Skia.RuntimeEffect.Make(HOLO_SKSL);

export interface LiveHoloCardProps {
  width: number;
  height: number;
  mode: HoloMode;
  /** Shared gyro tilt (UI thread), -1..1 per axis. Only read in `live` mode. */
  tilt: SharedValue<Tilt>;
  /** Shader time in seconds; pass a constant shared value to stop the ambient animation. */
  time: DerivedValue<number>;
  /** Live mode renders the shader into a canvas this much smaller, upscaled by the compositor. */
  resolutionScale: number;
  reducedMotion: boolean;
  skiaDebug?: boolean;
  /** Called once a baked card has its image, with the bake time in ms. */
  onBaked?: (ms: number) => void;
}

const staticUniforms = (width: number, height: number) => ({
  u_size: [width, height],
  u_time: 0,
  u_tilt: [STATIC_TILT.x, STATIC_TILT.y],
});

const clamp1 = (v: number) => {
  'worklet';
  return Math.max(-1, Math.min(1, v));
};

/**
 * One holo card for the T-52 lab: live (gyro + squish + time on the UI thread), baked (rendered once
 * to an image) or static (reduced motion). Squish is a view transform, so it never repaints the canvas.
 */
export function LiveHoloCard(props: LiveHoloCardProps) {
  const { width, height, mode, reducedMotion } = props;
  const press = useSharedValue(0);

  const tap = useMemo(
    () =>
      Gesture.Tap()
        .maxDuration(10_000)
        .onBegin(() => {
          press.set(
            reducedMotion
              ? animateTo(1, 'snappy', true)
              : withTiming(1, { duration: motion.duration.xs, reduceMotion: ReduceMotion.Never }),
          );
        })
        .onFinalize(() => {
          press.set(animateTo(0, 'squish', reducedMotion));
        }),
    [press, reducedMotion],
  );

  // Reduced motion: no scale, an opacity dip as press feedback (same rule as ui/Button).
  const squishStyle = useAnimatedStyle(() => {
    const p = press.get();
    if (reducedMotion) return { opacity: 1 - p * 0.2 };
    return {
      transform: [
        { scaleX: 1 + (SQUISH_SCALE.x - 1) * p },
        { scaleY: 1 + (SQUISH_SCALE.y - 1) * p },
      ],
    };
  });

  if (!holoEffect) {
    return <Text accessibilityRole="alert">{t('holo.unavailable')}</Text>;
  }

  return (
    <GestureDetector gesture={tap}>
      <Animated.View
        accessible
        accessibilityRole="image"
        accessibilityLabel={t('holo.lab.cardLabel')}
        style={[styles.card, { width, height }, squishStyle]}
      >
        {mode === 'live' ? (
          <LiveSurface {...props} press={press} />
        ) : mode === 'baked' ? (
          <BakedSurface {...props} />
        ) : (
          <Canvas style={{ width, height }} debug={props.skiaDebug}>
            <RoundedRect x={0} y={0} width={width} height={height} r={radius.md}>
              <Shader source={holoEffect} uniforms={staticUniforms(width, height)} />
            </RoundedRect>
          </Canvas>
        )}
      </Animated.View>
    </GestureDetector>
  );
}

function LiveSurface({
  width,
  height,
  tilt,
  time,
  resolutionScale,
  skiaDebug,
  press,
}: LiveHoloCardProps & { press: SharedValue<number> }) {
  const scale = Math.max(0.25, Math.min(1, resolutionScale));
  const w = Math.round(width * scale);
  const h = Math.round(height * scale);

  // UI thread only: recomputed when tilt, press or time change, then Skia repaints this canvas.
  const uniforms = useDerivedValue(() => {
    const tl = tilt.get();
    return {
      u_size: [w, h],
      u_time: time.get(),
      u_tilt: [clamp1(tl.x), clamp1(tl.y + press.get() * SQUISH_TILT_BUMP)],
    };
  });

  if (!holoEffect) return null;
  // A smaller canvas scaled up about its centre: the GPU fills scale² of the pixels.
  return (
    <View style={[styles.centre, { width, height }]}>
      <Canvas debug={skiaDebug} style={{ width: w, height: h, transform: [{ scale: 1 / scale }] }}>
        <RoundedRect x={0} y={0} width={w} height={h} r={radius.md * scale}>
          <Shader source={holoEffect} uniforms={uniforms} />
        </RoundedRect>
      </Canvas>
    </View>
  );
}

function BakedSurface({ width, height, skiaDebug, onBaked }: LiveHoloCardProps) {
  const [image, setImage] = useState<SkImage | null>(null);

  useEffect(() => {
    if (!holoEffect) return;
    let cancelled = false;
    const ratio = PixelRatio.get();
    const started = performance.now();
    drawAsImage(
      <Group transform={[{ scale: ratio }]}>
        <RoundedRect x={0} y={0} width={width} height={height} r={radius.md}>
          <Shader source={holoEffect} uniforms={staticUniforms(width, height)} />
        </RoundedRect>
      </Group>,
      { width: Math.round(width * ratio), height: Math.round(height * ratio) },
    ).then(
      (baked) => {
        if (cancelled) return;
        setImage(baked);
        onBaked?.(performance.now() - started);
      },
      () => {
        // Bake failed: the card stays empty; the lab is a dev tool, so no retry.
      },
    );
    return () => {
      cancelled = true;
    };
  }, [width, height, onBaked]);

  return (
    <Canvas style={{ width, height }} debug={skiaDebug}>
      {image ? <Image image={image} x={0} y={0} width={width} height={height} fit="fill" /> : null}
    </Canvas>
  );
}

const styles = StyleSheet.create({
  card: { borderRadius: radius.md },
  centre: { alignItems: 'center', justifyContent: 'center', overflow: 'hidden' },
});
