import {
  Canvas,
  Group,
  Image,
  Picture,
  Rect,
  Shader,
  Skia,
  useClock,
  type SkImage,
  type SkPicture,
} from '@shopify/react-native-skia';
import { useDerivedValue, useSharedValue, type SharedValue } from 'react-native-reanimated';

import { HOLO_SKSL } from '@card-sharing/shaders';

import { GyroTiltDriver } from '../holo-card/GyroTiltDriver';
import type { Tilt } from '../holo-card/gyroTilt';
import { SQUISH_TILT_BUMP, STATIC_TILT } from '../holo-card/holoLab';

// Compiled once per process; null means the SkSL failed to compile (the preview then shows the plain card).
const holoEffect = Skia.RuntimeEffect.Make(HOLO_SKSL);

export interface LiveFinishPreviewProps {
  /**
   * The card under the shine: a recorded picture without its finish (editor), or a received card's baked
   * render (full-screen view, T-81), fitted inside the canvas.
   */
  card: { picture: SkPicture } | { image: SkImage };
  width: number;
  height: number;
  /** The finish's `intensity` param (0..1). */
  intensity: number;
  /** Squish press (0 = released); pushes the foil along y (design-system.md §5.2). */
  press?: SharedValue<number>;
  testID?: string;
}

const clamp1 = (value: number) => {
  'worklet';
  return Math.max(-1, Math.min(1, value));
};

/**
 * The focused card with a live holo shine (T-65): gyro tilt and the ambient shimmer drive the shader uniforms
 * on the UI thread, the card itself is a recorded picture. Mounted only when `planFinishModes` says `live`,
 * so reduced motion never registers the sensor. The shine is SrcATop inside a layer: card pixels only.
 */
export function LiveFinishPreview({
  card,
  width,
  height,
  intensity,
  press,
  testID = 'editor-live-finish',
}: LiveFinishPreviewProps) {
  const tilt = useSharedValue<Tilt>({ x: 0, y: 0 });
  const clock = useClock();
  const uniforms = useDerivedValue(() => {
    const current = tilt.get();
    return {
      u_size: [width, height],
      // `.value`, not `.get()`: Skia's jest mock clock is a plain `{ value }` object.
      u_time: clock.value / 1000,
      u_tilt: [
        clamp1(STATIC_TILT.x + current.x),
        clamp1(STATIC_TILT.y + current.y + (press?.get() ?? 0) * SQUISH_TILT_BUMP),
      ],
    };
  });

  return (
    <>
      <GyroTiltDriver tilt={tilt} />
      <Canvas style={{ width, height }} testID={testID}>
        <Group layer>
          {'picture' in card ? (
            <Picture picture={card.picture} />
          ) : (
            <Image image={card.image} x={0} y={0} width={width} height={height} fit="contain" />
          )}
          {holoEffect ? (
            <Rect x={0} y={0} width={width} height={height} blendMode="srcATop" opacity={intensity}>
              <Shader source={holoEffect} uniforms={uniforms} />
            </Rect>
          ) : null}
        </Group>
      </Canvas>
    </>
  );
}
