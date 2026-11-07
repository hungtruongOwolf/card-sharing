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
import { useCallback, useEffect, useMemo, useReducer, useState } from 'react';
import { PixelRatio, StyleSheet, View, type AccessibilityActionEvent } from 'react-native';
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
  clampPinScale,
  contains,
  fitCamera,
  hitTest,
  intersects,
  panBy,
  pinBounds,
  sceneBounds,
  screenToBoard,
  snapRotation,
  visibleRect,
  zoomAround,
  type Camera,
  type PinGeometry,
  type Rect as BoardRect,
  type Vec,
} from '@card-sharing/canvas-engine';
import {
  registry as shippedRegistry,
  type Registry,
  type RemoteRegistryConfig,
} from '@card-sharing/effects-registry';
import { HOLO_SKSL } from '@card-sharing/shaders';
import { space } from '@card-sharing/tokens';

import { t, type MessageKey } from '../../i18n';
import { animateTo, useTheme } from '../../ui';
import { drawLiftShadow, liftShadow } from '../effects';
import { GyroTiltDriver } from '../holo-card/GyroTiltDriver';
import type { Tilt } from '../holo-card/gyroTilt';
import { STATIC_TILT } from '../holo-card/holoLab';
import { drawBackdrop } from './boardDecor';
import { BOARD_SIZE, pinGeometry, sortByZ, type BoardPin } from './boardModel';
import { PIN_ACTIONS, type PinAction } from './boardOps';
import { CULL_MARGIN, planBoardScene } from './boardScene';
import {
  boardPalette,
  containRect,
  drawPinContent,
  drawPins,
  drawSelection,
  type BoardPalette,
} from './drawBoard';
import type { TextureCache } from './textureCache';

// Compiled once per process; null means the SkSL failed to compile (live pins then show their baked image only).
const holoEffect = Skia.RuntimeEffect.Make(HOLO_SKSL);

/** Pick-up lift (design-system.md §5.3: `spring.lift`; v2 lifts a little higher so the pick-up reads). */
const LIFT_SCALE = 1.06;
/** Paper-lift height of a picked-up card (the registry maximum): its shadow deepens while held (T-118). */
const PICKED_UP_LIFT = 3;

/** A pin's transform while it is being edited. */
export interface PinPose {
  x: number;
  y: number;
  scale: number;
  rotation: number;
}

export interface BoardCanvasProps {
  pins: readonly BoardPin[];
  /** Registry backdrop id (T-111); null = the default desk paper. */
  backdropId?: string | null;
  width: number;
  height: number;
  textures: TextureCache<SkImage>;
  initialCamera: Camera;
  registry?: Registry;
  /** `/v1/config` registry part: a killed finish draws static. */
  remoteConfig?: RemoteRegistryConfig;
  /** Gets the first live-finish slot. */
  focusedId?: string | null;
  /** Shared camera, when the parent drives it too. */
  camera?: SharedValue<Camera>;
  /** Edit mode (T-104): pins can be picked up, moved, scaled, rotated and get accessibility actions. */
  editable?: boolean;
  /** Which pins I may change in edit mode (AC-19.2/19.7); others are not tappable and get no actions. */
  canEdit?: (pin: BoardPin) => boolean;
  selectedId?: string | null;
  /** Tap on a pin selects it, tap on the empty board clears the selection. */
  onSelect?: (id: string | null) => void;
  /** A drag, pinch or rotation started on this pin (bring it to front). */
  onPickUp?: (id: string) => void;
  /** The gesture on a pin ended with this pose (rotation already snapped to detents). */
  onCommit?: (id: string, pose: PinPose) => void;
  /** A screen-reader action on a pin. */
  onAction?: (id: string, action: PinAction) => void;
  testID?: string;
}

type Mode = 'none' | 'camera' | 'pin';
interface HitPin extends PinGeometry {
  id: string;
  editable: boolean;
}

/** Without a rule from the screen, every pin but a private tile or removed card can be edited. */
const DEFAULT_CAN_EDIT = (pin: BoardPin) =>
  pin.content.kind !== 'private_tile' && pin.content.kind !== 'removed';

const clamp1 = (value: number) => {
  'worklet';
  return Math.max(-1, Math.min(1, value));
};

const ACTION_LABELS: Record<PinAction, MessageKey> = {
  moveUp: 'boards.action.moveUp',
  moveDown: 'boards.action.moveDown',
  moveLeft: 'boards.action.moveLeft',
  moveRight: 'boards.action.moveRight',
  rotateLeft: 'boards.action.rotateLeft',
  rotateRight: 'boards.action.rotateRight',
  bigger: 'boards.action.bigger',
  smaller: 'boards.action.smaller',
  bringToFront: 'boards.action.bringToFront',
  sendToBack: 'boards.action.sendToBack',
  remove: 'boards.action.remove',
};
const A11Y_ACTIONS = PIN_ACTIONS.map((name) => ({ name, label: t(ACTION_LABELS[name]) }));
const isPinAction = (name: string): name is PinAction =>
  (PIN_ACTIONS as readonly string[]).includes(name);

/** Re-renders when the texture cache reports a change. */
function useTextureVersion(textures: TextureCache<SkImage>): number {
  const [version, bump] = useReducer((n: number) => n + 1, 0);
  useEffect(() => textures.subscribe(bump), [textures]);
  return version;
}

function pinLabel(pin: BoardPin, registry: Registry): string {
  switch (pin.content.kind) {
    case 'card': {
      if (pin.content.card.isMine) return t('boards.pin.card.mine');
      const name = pin.content.card.sender.displayName;
      return name ? t('boards.pin.card', { name }) : t('boards.pin.card.someone');
    }
    case 'private_tile':
      return t('boards.pin.private');
    case 'removed':
      return t('boards.pin.removed');
    case 'decoration': {
      const entry = registry.get(pin.content.decoration.id);
      return entry?.kind === 'decoration'
        ? t('boards.pin.decoration', { name: t(entry.name as MessageKey) })
        : t('boards.pin.decoration.unknown');
    }
  }
}

const imageOf = (pin: BoardPin, textures: TextureCache<SkImage>) =>
  pin.content.kind === 'card' ? (textures.get(pin.content.card.id) ?? null) : null;

interface PinBodyProps {
  pin: BoardPin;
  image: SkImage | null;
  /** Live finish intensity, or null to draw the baked content only. */
  liveIntensity: number | null;
  tilt: SharedValue<Tilt>;
  palette: BoardPalette;
  selected: boolean;
}

/**
 * A pin's content centred on the origin: the baked texture, with the holo shine animated by the gyro and the
 * ambient clock on the UI thread when it holds a live slot (T-65 shader).
 */
function PinBody({ pin, image, liveIntensity, tilt, palette, selected }: PinBodyProps) {
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
  const live = liveIntensity !== null && image !== null && dest !== null && holoEffect !== null;
  const baked = useMemo(
    () =>
      createPicture((canvas) => {
        if (!live) drawPinContent(canvas, { pin, image }, palette);
        if (selected) drawSelection(canvas, palette, pin);
      }),
    [live, pin, image, palette, selected],
  );
  // A held card casts the deeper paper-lift shadow, drawn below everything else of the pin.
  const shadow = useMemo(
    () =>
      selected && image && w > 0 && pin.content.kind === 'card'
        ? createPicture((canvas) =>
            drawLiftShadow(
              canvas,
              (layer) => drawPinContent(layer, { pin, image }, palette),
              liftShadow(PICKED_UP_LIFT, Math.min(w, h)),
            ),
          )
        : null,
    [selected, image, w, h, pin, palette],
  );
  return (
    <>
      {shadow ? <Picture picture={shadow} /> : null}
      {live && dest ? (
        <Group transform={[{ translateX: dest.x }, { translateY: dest.y }]}>
          <Group layer>
            <Image image={image} x={0} y={0} width={w} height={h} fit="fill" />
            <Rect x={0} y={0} width={w} height={h} blendMode="srcATop" opacity={liveIntensity}>
              <Shader source={holoEffect} uniforms={uniforms} />
            </Rect>
          </Group>
        </Group>
      ) : null}
      <Picture picture={baked} />
    </>
  );
}

/**
 * The board (T-103/T-104, design-system.md §5.3/§7): one Skia canvas, pins culled to the view plus a margin and
 * recorded as pictures of baked textures, live finishes only for the few pins the registry budget allows (none under
 * reduced motion). One finger pans the board and two pinch-zoom it around the focal point; in edit mode the same
 * gestures started on a pin pick it up (lift + bring to front) and move, scale and rotate it on the UI thread, with
 * magnetic rotation detents, and commit once on release. The canvas is opaque to screen readers, so an invisible
 * layer of labelled views mirrors the pins on screen and carries the move/rotate/scale/layer/remove actions.
 */
export function BoardCanvas({
  pins,
  backdropId = null,
  width,
  height,
  textures,
  initialCamera,
  registry = shippedRegistry,
  remoteConfig,
  focusedId = null,
  camera: external,
  editable = false,
  canEdit = DEFAULT_CAN_EDIT,
  selectedId = null,
  onSelect,
  onPickUp,
  onCommit,
  onAction,
  testID = 'board-canvas',
}: BoardCanvasProps) {
  const theme = useTheme();
  const reducedMotion = theme.reducedMotion;
  const palette = useMemo(
    () => boardPalette(theme.color, theme.scheme),
    [theme.color, theme.scheme],
  );
  const own = useSharedValue<Camera>(initialCamera);
  const camera = external ?? own;
  // The parent's camera starts at `initialCamera` too; reading the shared value during render is not allowed.
  const [settled, setSettled] = useState<Camera>(initialCamera);
  const textureVersion = useTextureVersion(textures);
  const tilt = useSharedValue<Tilt>({ x: 0, y: 0 });
  const view = useMemo(() => ({ width, height }), [width, height]);
  const floatingId = editable ? selectedId : null;

  const plan = useMemo(
    () =>
      planBoardScene({
        pins,
        camera: settled,
        view,
        pixelRatio: PixelRatio.get(),
        reducedMotion,
        registry,
        remoteConfig,
        focusedId,
        floatingId,
      }),
    [pins, settled, view, reducedMotion, registry, remoteConfig, focusedId, floatingId],
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

  // The backdrop covers the board area inside the recorded window; beyond the board is the desk.
  const backdrop = useMemo(() => {
    const w = plan.window;
    const x0 = Math.max(0, w.x);
    const y0 = Math.max(0, w.y);
    const x1 = Math.min(BOARD_SIZE.width, w.x + w.width);
    const y1 = Math.min(BOARD_SIZE.height, w.y + w.height);
    if (x1 <= x0 || y1 <= y0) return null;
    const area = { x: x0, y: y0, width: x1 - x0, height: y1 - y0 };
    return createPicture(
      (canvas) => drawBackdrop(canvas, backdropId, area, palette.decor, registry),
      Skia.XYWHRect(area.x, area.y, area.width, area.height),
    );
  }, [plan.window, backdropId, palette, registry]);

  const pictures = useMemo(
    () =>
      plan.layers.map((layer) =>
        layer.kind === 'static'
          ? createPicture(
              (canvas) =>
                drawPins(
                  canvas,
                  layer.pins.map((pin) => ({ pin, image: imageOf(pin, textures) })),
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

  // Gesture state lives on the UI thread; JS hears about a gesture only when it starts on a pin and when it ends.
  const hitPins = useSharedValue<HitPin[]>([]);
  useEffect(() => {
    hitPins.set(
      sortByZ(pins).map((pin) => ({ id: pin.id, editable: canEdit(pin), ...pinGeometry(pin) })),
    );
  }, [pins, hitPins, canEdit]);
  const mode = useSharedValue<Mode>('none');
  const activeGestures = useSharedValue(0);
  const activeId = useSharedValue<string | null>(null);
  const pose = useSharedValue<PinPose>({ x: 0, y: 0, scale: 1, rotation: 0 });
  const lift = useSharedValue(1);
  const lastPan = useSharedValue({ x: 0, y: 0 });
  const lastScale = useSharedValue(1);
  const lastRotation = useSharedValue(0);

  const floatingPin = plan.floating?.pin ?? null;
  useEffect(() => {
    if (!floatingPin || mode.get() === 'pin') return;
    activeId.set(floatingPin.id);
    pose.set({
      x: floatingPin.x,
      y: floatingPin.y,
      scale: floatingPin.scale,
      rotation: floatingPin.rotation,
    });
  }, [floatingPin, activeId, mode, pose]);

  const pickUp = useCallback((id: string) => onPickUp?.(id), [onPickUp]);
  const commit = useCallback((id: string, next: PinPose) => onCommit?.(id, next), [onCommit]);
  const select = useCallback((id: string | null) => onSelect?.(id), [onSelect]);
  const zoomToFit = useCallback(() => {
    const area = sceneBounds(pins.map(pinGeometry));
    if (area) camera.set(fitCamera(area, view, space[6]));
    setSettled(camera.get());
  }, [pins, view, camera]);

  const gesture = useMemo(() => {
    const pinAt = (point: Vec): HitPin | null => {
      'worklet';
      const list = hitPins.get();
      const index = hitTest(list, screenToBoard(camera.get(), point));
      return index >= 0 ? list[index]! : null;
    };
    const begin = (point: Vec) => {
      'worklet';
      activeGestures.set(activeGestures.get() + 1);
      if (mode.get() !== 'none') return;
      // The topmost pin under the finger; one I may not change lets the gesture move the board instead.
      const top = editable ? pinAt(point) : null;
      const hit = top?.editable ? top : null;
      if (!hit) {
        mode.set('camera');
        return;
      }
      mode.set('pin');
      activeId.set(hit.id);
      pose.set({ x: hit.x, y: hit.y, scale: hit.scale, rotation: hit.rotation });
      lift.set(animateTo(LIFT_SCALE, 'lift', reducedMotion));
      scheduleOnRN(pickUp, hit.id);
    };
    const end = () => {
      'worklet';
      activeGestures.set(Math.max(0, activeGestures.get() - 1));
      if (activeGestures.get() > 0) return;
      const id = activeId.get();
      if (mode.get() === 'pin' && id !== null) {
        const current = pose.get();
        const final = {
          x: current.x,
          y: current.y,
          scale: clampPinScale(current.scale),
          rotation: snapRotation(current.rotation).rotation,
        };
        pose.set(final);
        lift.set(animateTo(1, 'drop', reducedMotion));
        scheduleOnRN(commit, id, final);
      }
      mode.set('none');
      scheduleOnRN(setSettled, camera.get());
    };

    const pan = Gesture.Pan()
      .withTestId(`${testID}-pan`)
      .onStart((e) => {
        lastPan.set({ x: e.translationX, y: e.translationY });
        begin({ x: e.x - e.translationX, y: e.y - e.translationY });
      })
      .onUpdate((e) => {
        const last = lastPan.get();
        const dx = e.translationX - last.x;
        const dy = e.translationY - last.y;
        lastPan.set({ x: e.translationX, y: e.translationY });
        if (mode.get() === 'pin') {
          const zoom = camera.get().zoom;
          const p = pose.get();
          pose.set({ ...p, x: p.x + dx / zoom, y: p.y + dy / zoom });
        } else {
          camera.set(panBy(camera.get(), dx, dy));
        }
      })
      .onEnd(end);
    const pinch = Gesture.Pinch()
      .withTestId(`${testID}-pinch`)
      .onStart((e) => {
        lastScale.set(e.scale);
        begin({ x: e.focalX, y: e.focalY });
      })
      .onUpdate((e) => {
        const factor = lastScale.get() > 0 ? e.scale / lastScale.get() : 1;
        lastScale.set(e.scale);
        if (mode.get() === 'pin') {
          const p = pose.get();
          pose.set({ ...p, scale: clampPinScale(p.scale * factor) });
        } else {
          camera.set(zoomAround(camera.get(), { x: e.focalX, y: e.focalY }, factor));
        }
      })
      .onEnd(end);
    const rotation = Gesture.Rotation()
      .withTestId(`${testID}-rotation`)
      .onStart((e) => {
        lastRotation.set(e.rotation);
        begin({ x: e.anchorX, y: e.anchorY });
      })
      .onUpdate((e) => {
        const delta = ((e.rotation - lastRotation.get()) * 180) / Math.PI;
        lastRotation.set(e.rotation);
        if (mode.get() === 'pin') {
          const p = pose.get();
          pose.set({ ...p, rotation: p.rotation + delta });
        }
      })
      .onEnd(end);
    const tap = Gesture.Tap()
      .withTestId(`${testID}-tap`)
      .onEnd((e, success) => {
        if (!success || !editable) return;
        const top = pinAt({ x: e.x, y: e.y });
        scheduleOnRN(select, top?.editable ? top.id : null);
      });
    const doubleTap = Gesture.Tap()
      .numberOfTaps(2)
      .withTestId(`${testID}-double-tap`)
      .onEnd((e, success) => {
        if (success && !pinAt({ x: e.x, y: e.y })) scheduleOnRN(zoomToFit);
      });
    return Gesture.Race(
      Gesture.Simultaneous(pan, pinch, rotation),
      Gesture.Exclusive(doubleTap, tap),
    );
  }, [
    activeGestures,
    activeId,
    camera,
    commit,
    editable,
    hitPins,
    lastPan,
    lastRotation,
    lastScale,
    lift,
    mode,
    pickUp,
    pose,
    reducedMotion,
    select,
    testID,
    zoomToFit,
  ]);

  const transform = useDerivedValue(() => {
    const c = camera.get();
    return [{ scale: c.zoom }, { translateX: -c.x }, { translateY: -c.y }];
  });
  const floatingTransform = useDerivedValue(() => {
    const p = pose.get();
    return [
      { translateX: p.x },
      { translateY: p.y },
      { rotate: (snapRotation(p.rotation).rotation * Math.PI) / 180 },
      { scale: p.scale * lift.get() },
    ];
  });

  const onScreen = visibleRect(settled, view, 0);
  const labelled = [
    ...plan.visible,
    ...(floatingPin && !plan.visible.includes(floatingPin) ? [floatingPin] : []),
  ]
    .map((pin) => ({ pin, bounds: pinBounds(pinGeometry(pin)) }))
    .filter(({ bounds }) => intersects(bounds, onScreen));
  const liveCount =
    plan.layers.filter((l) => l.kind === 'live').length + (plan.floating?.finish ? 1 : 0);
  const onA11yAction = (id: string) => (event: AccessibilityActionEvent) => {
    const name = event.nativeEvent.actionName;
    if (isPinAction(name)) onAction?.(id, name);
  };

  return (
    <GestureDetector gesture={gesture}>
      <View
        style={{ width, height }}
        testID={testID}
        accessibilityLabel={t('boards.canvas.label', { count: plan.visible.length })}
      >
        {liveCount > 0 && !reducedMotion ? <GyroTiltDriver tilt={tilt} /> : null}
        <Canvas style={StyleSheet.absoluteFill}>
          <Fill color={palette.desk} />
          <Group transform={transform}>
            {backdrop ? <Picture picture={backdrop} /> : null}
            {plan.layers.map((layer, i) => {
              if (layer.kind === 'static') {
                const picture = pictures[i];
                return picture ? <Picture key={`s${i}`} picture={picture} /> : null;
              }
              const { pin } = layer;
              return (
                <Group
                  key={pin.id}
                  transform={[
                    { translateX: pin.x },
                    { translateY: pin.y },
                    { rotate: (pin.rotation * Math.PI) / 180 },
                    { scale: pin.scale },
                  ]}
                >
                  <PinBody
                    pin={pin}
                    image={imageOf(pin, textures)}
                    liveIntensity={layer.intensity}
                    tilt={tilt}
                    palette={palette}
                    selected={false}
                  />
                </Group>
              );
            })}
            {plan.floating ? (
              <Group transform={floatingTransform}>
                <PinBody
                  pin={plan.floating.pin}
                  image={imageOf(plan.floating.pin, textures)}
                  liveIntensity={plan.floating.finish?.intensity ?? null}
                  tilt={tilt}
                  palette={palette}
                  selected
                />
              </Group>
            ) : null}
          </Group>
        </Canvas>
        {/* Canvas coordinates are physical: the overlay is always left-to-right so `start` means left. */}
        <View style={[StyleSheet.absoluteFill, styles.ltr]} pointerEvents="none">
          {labelled.map(({ pin, bounds }) => {
            const topLeft = boardToScreen(settled, bounds);
            const pinEditable = editable && canEdit(pin);
            return (
              <View
                key={pin.id}
                testID={`board-pin-${pin.id}`}
                accessible
                accessibilityRole="image"
                accessibilityLabel={pinLabel(pin, registry)}
                accessibilityHint={pinEditable ? t('boards.pin.hint') : undefined}
                accessibilityState={pinEditable ? { selected: pin.id === selectedId } : undefined}
                accessibilityActions={pinEditable ? A11Y_ACTIONS : undefined}
                onAccessibilityAction={pinEditable ? onA11yAction(pin.id) : undefined}
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
