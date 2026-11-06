import {
  Canvas,
  Group,
  Picture,
  Rect,
  Shader,
  Skia,
  useClock,
  type SkPicture,
} from '@shopify/react-native-skia';
import { useDerivedValue, useSharedValue } from 'react-native-reanimated';

import { HOLO_SKSL } from '@card-sharing/shaders';

import { GyroTiltDriver } from '../holo-card/GyroTiltDriver';
import type { Tilt } from '../holo-card/gyroTilt';
import { STATIC_TILT } from '../holo-card/holoLab';

// Compiled once per process; null means the SkSL failed to compile (the preview then shows the plain card).
const holoEffect = Skia.RuntimeEffect.Make(HOLO_SKSL);

export interface LiveFinishPreviewProps {
  /** The card drawn without its finish. */
  picture: SkPicture;
  width: number;
  height: number;
  /** The finish's `intensity` param (0..1). */
  intensity: number;
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
export function LiveFinishPreview({ picture, width, height, intensity }: LiveFinishPreviewProps) {
  const tilt = useSharedValue<Tilt>({ x: 0, y: 0 });
  const clock = useClock();
  const uniforms = useDerivedValue(() => {
    const current = tilt.get();
    return {
      u_size: [width, height],
      // `.value`, not `.get()`: Skia's jest mock clock is a plain `{ value }` object.
      u_time: clock.value / 1000,
      u_tilt: [clamp1(STATIC_TILT.x + current.x), clamp1(STATIC_TILT.y + current.y)],
    };
  });

  return (
    <>
      <GyroTiltDriver tilt={tilt} />
      <Canvas style={{ width, height }} testID="editor-live-finish">
        <Group layer>
          <Picture picture={picture} />
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
