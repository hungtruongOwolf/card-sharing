import {
  cullPins,
  pinBounds,
  pinScreenLongSide,
  textureLod,
  visibleRect,
  type Camera,
  type Rect,
  type Size,
} from '@card-sharing/canvas-engine';
import {
  killedIds,
  resolveParams,
  type EffectEntry,
  type Registry,
  type RemoteRegistryConfig,
} from '@card-sharing/effects-registry';

import { planFinishModes } from '../editor/liveFinish';
import { pinGeometry, sortByZ, type BoardPin } from './boardModel';
import type { TextureRequest } from './textureCache';

/**
 * Pins are recorded for the view plus this share of it on every side, so panning within it needs no re-record
 * (design-system.md §7: no per-frame JS).
 */
export const CULL_MARGIN = 0.5;

export type SceneLayer =
  | { kind: 'static'; pins: BoardPin[] }
  | { kind: 'live'; pin: BoardPin; entry: EffectEntry; intensity: number };

export interface BoardScenePlan {
  /** Board area the layers cover; re-plan once the view leaves it. */
  window: Rect;
  /** Bottom to top: runs of baked pins between live-finish pins, so z order is exact. */
  layers: SceneLayer[];
  /** The pin being edited: drawn on top with a gesture-driven transform, never in a picture. */
  floating: { pin: BoardPin; finish: { entry: EffectEntry; intensity: number } | null } | null;
  /** Pins inside the window, bottom to top (accessibility layer, texture requests). */
  visible: BoardPin[];
  textures: TextureRequest[];
}

export interface PlanInput {
  pins: readonly BoardPin[];
  camera: Camera;
  view: Size;
  pixelRatio: number;
  reducedMotion: boolean;
  registry: Registry;
  remoteConfig?: RemoteRegistryConfig;
  /** The focused pin gets the first live slot (design-system.md §4: holo live only when selected/focused). */
  focusedId?: string | null;
  /** The selected pin in edit mode; it is lifted out of the pictures (implies focus). */
  floatingId?: string | null;
}

function finishOf(
  pin: BoardPin,
  registry: Registry,
  killed: ReadonlySet<string>,
): { entry: EffectEntry; intensity: number } | null {
  if (pin.content.kind !== 'card') return null;
  const finish = pin.content.card.finish;
  if (!finish) return null;
  const entry = registry.get(finish.id);
  if (!entry || entry.kind !== 'filter' || killed.has(entry.id)) return null;
  const params = resolveParams(entry, finish.params);
  const intensity = params.ok ? Number(params.params.intensity ?? 1) : 1;
  return { entry, intensity: Number.isFinite(intensity) ? intensity : 1 };
}

function overlapArea(a: Rect, b: Rect): number {
  const w = Math.min(a.x + a.width, b.x + b.width) - Math.max(a.x, b.x);
  const h = Math.min(a.y + a.height, b.y + b.height) - Math.max(a.y, b.y);
  return w > 0 && h > 0 ? w * h : 0;
}

/**
 * What to draw for a camera position (T-103): culls off-window pins, gives live finishes to the focused pin and
 * then the pins taking the most screen, within each finish's registry `cost.maxLive` (static under reduced
 * motion), and picks each card's texture level from its size on screen.
 */
export function planBoardScene(input: PlanInput): BoardScenePlan {
  const { camera, view, pixelRatio } = input;
  const window = visibleRect(camera, view, CULL_MARGIN);
  const onScreen = visibleRect(camera, view, 0);
  const visible = cullPins(
    sortByZ(input.pins).map((pin) => ({ ...pinGeometry(pin), pin })),
    window,
  ).map((g) => g.pin);

  const killed = killedIds(input.remoteConfig);
  const focusedId = input.floatingId ?? input.focusedId;
  const candidates = visible
    .map((pin) => ({
      pin,
      finish: finishOf(pin, input.registry, killed),
      shown: overlapArea(pinBounds(pinGeometry(pin)), onScreen),
      focused: pin.id === focusedId,
    }))
    .filter((c) => c.finish !== null && c.shown > 0)
    .sort((a, b) => Number(b.focused) - Number(a.focused) || b.shown - a.shown);
  const modes = planFinishModes(
    candidates.map((c) => c.finish!.entry),
    { reducedMotion: input.reducedMotion },
  );
  const live = new Map(
    candidates.filter((_, i) => modes[i] === 'live').map((c) => [c.pin.id, c.finish!]),
  );

  const floatingPin = input.floatingId
    ? (input.pins.find((p) => p.id === input.floatingId) ?? null)
    : null;
  const floating = floatingPin
    ? { pin: floatingPin, finish: live.get(floatingPin.id) ?? null }
    : null;

  const layers: SceneLayer[] = [];
  for (const pin of visible) {
    if (pin.id === floatingPin?.id) continue;
    const finish = live.get(pin.id);
    if (finish) {
      layers.push({ kind: 'live', pin, ...finish });
      continue;
    }
    const last = layers[layers.length - 1];
    if (last?.kind === 'static') last.pins.push(pin);
    else layers.push({ kind: 'static', pins: [pin] });
  }

  const screenLong = Math.max(view.width, view.height) * pixelRatio;
  const textures: TextureRequest[] = [];
  const textured =
    floatingPin && !visible.includes(floatingPin) ? [...visible, floatingPin] : visible;
  for (const pin of textured) {
    if (pin.content.kind !== 'card' || !pin.content.card.media) continue;
    const onScreenPx = pinScreenLongSide(pinGeometry(pin), camera.zoom) * pixelRatio;
    textures.push({
      key: pin.content.card.id,
      url: pin.content.card.media.renderUrl,
      lod: textureLod(onScreenPx, screenLong),
    });
  }
  return { window, layers, floating, visible, textures };
}
