import {
  Canvas,
  createPicture,
  Fill,
  Group,
  Image,
  Picture,
  Rect,
  Shader,
  Skia,
  useClock,
  type SkImage,
} from '@shopify/react-native-skia';
import { useEffect, useMemo, useReducer, useState } from 'react';
import { PixelRatio, StyleSheet, View } from 'react-native';
import { Gesture, GestureDetector } from 'react-native-gesture-handler';
import {
  useAnimatedReaction,
  useDerivedValue,
  useSharedValue,
  type SharedValue,
} from 'react-native-reanimated';
import { scheduleOnRN } from 'react-native-worklets';

import {
  boardToScreen,
  contains,
  intersects,
  panBy,
  pinBounds,
  visibleRect,
  zoomAround,
  type Camera,
  type Rect as BoardRect,
} from '@card-sharing/canvas-engine';
import {
  registry as shippedRegistry,
  type Registry,
  type RemoteRegistryConfig,
} from '@card-sharing/effects-registry';
import { HOLO_SKSL } from '@card-sharing/shaders';

import { t } from '../../i18n';
import { useTheme } from '../../ui';
import { GyroTiltDriver } from '../holo-card/GyroTiltDriver';
import type { Tilt } from '../holo-card/gyroTilt';
import { STATIC_TILT } from '../holo-card/holoLab';
import { pinGeometry, type BoardPin } from './boardModel';
import { CULL_MARGIN, planBoardScene } from './boardScene';
import {
  applyPinTransform,
  boardPalette,
  containRect,
  drawPinContent,
  drawPins,
  type BoardPalette,
} from './drawBoard';
import type { TextureCache } from './textureCache';

// Compiled once per process; null means the SkSL failed to compile (live pins then show their baked image only).
const holoEffect = Skia.RuntimeEffect.Make(HOLO_SKSL);

export interface BoardCanvasProps {
  pins: readonly BoardPin[];
  width: number;
  height: number;
  textures: TextureCache<SkImage>;
  initialCamera: Camera;
  registry?: Registry;
  /** `/v1/config` registry part: a killed finish draws static. */
  remoteConfig?: RemoteRegistryConfig;
  /** Gets the first live-finish slot. */
  focusedId?: string | null;
  /** Shared camera, when the parent drives it too (zoom to fit, lab knobs). */
  camera?: SharedValue<Camera>;
  testID?: string;
}

const clamp1 = (value: number) => {
  'worklet';
  return Math.max(-1, Math.min(1, value));
};

/** Re-renders when the texture cache reports a change. */
function useTextureVersion(textures: TextureCache<SkImage>): number {
  const [version, bump] = useReducer((n: number) => n + 1, 0);
  useEffect(() => textures.subscribe(bump), [textures]);
  return version;
}

function pinLabel(pin: BoardPin): string {
  switch (pin.content.kind) {
    case 'card': {
      const name = pin.content.card.sender.displayName;
      return name ? t('boards.pin.card', { name }) : t('boards.pin.card.someone');
    }
    case 'private_tile':
      return t('boards.pin.private');
    case 'removed':
      return t('boards.pin.removed');
  }
}

interface LivePinProps {
  pin: BoardPin;
  image: SkImage | null;
  intensity: number;
  tilt: SharedValue<Tilt>;
  palette: BoardPalette;
}

/** A card pin with its holo finish animated by the gyro and the ambient clock on the UI thread (T-65 shader). */
function LivePin({ pin, image, intensity, tilt, palette }: LivePinProps) {
  const clock = useClock();
  const dest = image ? containRect(image.width(), image.height()) : null;
  const w = dest?.width ?? 0;
  const h = dest?.height ?? 0;
  const uniforms = useDerivedValue(() => {
    const current = tilt.get();
    return {
      u_size: [w, h],
      // `.value`, not `.get()`: Skia's jest mock clock is a plain `{ value }` object.
      u_time: clock.value / 1000,
      u_tilt: [clamp1(STATIC_TILT.x + current.x), clamp1(STATIC_TILT.y + current.y)],
    };
  });
  const ghost = useMemo(
    () =>
      image
        ? null
        : createPicture((canvas) => {
            canvas.save();
            applyPinTransform(canvas, pin);
            drawPinContent(canvas, { pin, image: null }, palette);
            canvas.restore();
          }),
    [image, pin, palette],
  );
  if (!image || !dest) return ghost ? <Picture picture={ghost} /> : null;
  return (
    <Group
      transform={[
        { translateX: pin.x },
        { translateY: pin.y },
        { rotate: (pin.rotation * Math.PI) / 180 },
        { scale: pin.scale },
        { translateX: dest.x },
        { translateY: dest.y },
      ]}
    >
      <Group layer>
        <Image image={image} x={0} y={0} width={w} height={h} fit="fill" />
        {holoEffect ? (
          <Rect x={0} y={0} width={w} height={h} blendMode="srcATop" opacity={intensity}>
            <Shader source={holoEffect} uniforms={uniforms} />
          </Rect>
        ) : null}
      </Group>
    </Group>
  );
}

/**
 * The board (T-103, design-system.md §5.3/§7): one Skia canvas, pins culled to the view plus a margin and recorded
 * as pictures of baked textures, live finishes only for the few pins the registry budget allows (none under
 * reduced motion), and a camera that pans with one finger and zooms around the pinch focal point on the UI thread.
 * The canvas is opaque to screen readers, so an invisible layer of labelled views mirrors the pins on screen.
 */
export function BoardCanvas({
  pins,
  width,
  height,
  textures,
  initialCamera,
  registry = shippedRegistry,
  remoteConfig,
  focusedId = null,
  camera: external,
  testID = 'board-canvas',
}: BoardCanvasProps) {
  const theme = useTheme();
  const palette = useMemo(() => boardPalette(theme.color), [theme.color]);
  const own = useSharedValue<Camera>(initialCamera);
  const camera = external ?? own;
  const [settled, setSettled] = useState<Camera>(() => camera.get());
  const textureVersion = useTextureVersion(textures);
  const tilt = useSharedValue<Tilt>({ x: 0, y: 0 });
  const view = useMemo(() => ({ width, height }), [width, height]);

  const plan = useMemo(
    () =>
      planBoardScene({
        pins,
        camera: settled,
        view,
        pixelRatio: PixelRatio.get(),
        reducedMotion: theme.reducedMotion,
        registry,
        remoteConfig,
        focusedId,
      }),
    [pins, settled, view, theme.reducedMotion, registry, remoteConfig, focusedId],
  );

  useEffect(() => {
    void textures.ensure(plan.textures);
  }, [textures, plan.textures]);

  // Re-plan only when the view leaves the recorded window: panning inside it costs no JS.
  const windowRect = useSharedValue<BoardRect>(plan.window);
  useEffect(() => windowRect.set(plan.window), [plan.window, windowRect]);
  useAnimatedReaction(
    () => camera.get(),
    (current, previous) => {
      if (previous === null || current === previous) return;
      const shown = visibleRect(current, view, 0);
      if (!contains(windowRect.get(), shown)) {
        windowRect.set(visibleRect(current, view, CULL_MARGIN));
        scheduleOnRN(setSettled, current);
      }
    },
    [view],
  );

  const pictures = useMemo(
    () =>
      plan.layers.map((layer) =>
        layer.kind === 'static'
          ? createPicture(
              (canvas) =>
                drawPins(
                  canvas,
                  layer.pins.map((pin) => ({
                    pin,
                    image:
                      pin.content.kind === 'card'
                        ? (textures.get(pin.content.card.id) ?? null)
                        : null,
                  })),
                  palette,
                ),
              Skia.XYWHRect(plan.window.x, plan.window.y, plan.window.width, plan.window.height),
            )
          : null,
      ),
    // textureVersion: re-record when a texture arrives.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [plan, palette, textures, textureVersion],
  );

  // Last translation / scale seen, so pan and pinch each apply only their own delta and compose.
  const lastPan = useSharedValue({ x: 0, y: 0 });
  const lastScale = useSharedValue(1);
  const gesture = useMemo(() => {
    const settle = () => {
      'worklet';
      scheduleOnRN(setSettled, camera.get());
    };
    const pan = Gesture.Pan()
      .withTestId(`${testID}-pan`)
      .onStart((e) => {
        lastPan.set({ x: e.translationX, y: e.translationY });
      })
      .onUpdate((e) => {
        const last = lastPan.get();
        camera.set(panBy(camera.get(), e.translationX - last.x, e.translationY - last.y));
        lastPan.set({ x: e.translationX, y: e.translationY });
      })
      .onEnd(settle);
    const pinch = Gesture.Pinch()
      .withTestId(`${testID}-pinch`)
      .onStart((e) => {
        lastScale.set(e.scale);
      })
      .onUpdate((e) => {
        const factor = lastScale.get() > 0 ? e.scale / lastScale.get() : 1;
        camera.set(zoomAround(camera.get(), { x: e.focalX, y: e.focalY }, factor));
        lastScale.set(e.scale);
      })
      .onEnd(settle);
    return Gesture.Simultaneous(pan, pinch);
  }, [camera, lastPan, lastScale, testID]);

  const transform = useDerivedValue(() => {
    const c = camera.get();
    return [{ scale: c.zoom }, { translateX: -c.x }, { translateY: -c.y }];
  });

  const onScreen = visibleRect(settled, view, 0);
  const labelled = plan.visible
    .map((pin) => ({ pin, bounds: pinBounds(pinGeometry(pin)) }))
    .filter(({ bounds }) => intersects(bounds, onScreen));
  const liveCount = plan.layers.filter((l) => l.kind === 'live').length;

  return (
    <GestureDetector gesture={gesture}>
      <View
        style={{ width, height }}
        testID={testID}
        accessibilityLabel={t('boards.canvas.label', { count: plan.visible.length })}
      >
        {liveCount > 0 && !theme.reducedMotion ? <GyroTiltDriver tilt={tilt} /> : null}
        <Canvas style={StyleSheet.absoluteFill}>
          <Fill color={palette.desk} />
          <Group transform={transform}>
            {plan.layers.map((layer, i) => {
              if (layer.kind === 'static') {
                const picture = pictures[i];
                return picture ? <Picture key={`s${i}`} picture={picture} /> : null;
              }
              const image =
                layer.pin.content.kind === 'card'
                  ? (textures.get(layer.pin.content.card.id) ?? null)
                  : null;
              return (
                <LivePin
                  key={layer.pin.id}
                  pin={layer.pin}
                  image={image}
                  intensity={layer.intensity}
                  tilt={tilt}
                  palette={palette}
                />
              );
            })}
          </Group>
        </Canvas>
        {/* Canvas coordinates are physical: the overlay is always left-to-right so `start` means left. */}
        <View style={[StyleSheet.absoluteFill, styles.ltr]} pointerEvents="none">
          {labelled.map(({ pin, bounds }) => {
            const topLeft = boardToScreen(settled, bounds);
            return (
              <View
                key={pin.id}
                testID={`board-pin-${pin.id}`}
                accessible
                accessibilityRole="image"
                accessibilityLabel={pinLabel(pin)}
                style={{
                  position: 'absolute',
                  start: topLeft.x,
                  top: topLeft.y,
                  width: bounds.width * settled.zoom,
                  height: bounds.height * settled.zoom,
                }}
              />
            );
          })}
        </View>
      </View>
    </GestureDetector>
  );
}

const styles = StyleSheet.create({
  ltr: { direction: 'ltr' },
});
