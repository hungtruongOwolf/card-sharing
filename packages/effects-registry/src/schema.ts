/**
 * Effect registry entry schema (context/wiki/product/effects-catalog.md §1, ADR-0003).
 *
 * Every cut style, finish, effect, prop and frame is one data entry. Screens never branch on ids:
 * they read entries, render through `renderer.ref`, and show what `resolveAvailability` allows.
 * Stored cards keep `{id, version, params}`; the server validates them against the exported JSON (T-60).
 */

export const EFFECT_KINDS = ['cutStyle', 'filter', 'overlay', 'prop', 'frame', 'layout'] as const;
export type EffectKind = (typeof EFFECT_KINDS)[number];

/** Id prefix each kind must use (`kind.slug`). `fx.*` covers filters and finishes. */
export const KIND_PREFIXES: Record<EffectKind, readonly string[]> = {
  cutStyle: ['cut'],
  filter: ['fx'],
  overlay: ['overlay', 'fx'],
  prop: ['prop'],
  frame: ['frame'],
  layout: ['layout'],
};

export const TIERS = ['0', '1', '1b', '2'] as const;
export type Tier = (typeof TIERS)[number];

export const INPUTS = [
  'image',
  'mask',
  'contour',
  'faceLandmarks',
  'animalPose',
  'bodyPose',
  'gyro',
  'time',
  'touch',
  'text',
  'multiPhoto',
] as const;
export type EffectInput = (typeof INPUTS)[number];

export const ANCHORS = ['none', 'bbox', 'head', 'eyes', 'mouth', 'top', 'userPoints'] as const;
export type Anchor = (typeof ANCHORS)[number];

export const RENDERER_TYPES = [
  'sksl',
  'imageFilter',
  'pathEffect',
  'particles',
  'anchoredAsset',
  'skottie',
  'layout',
  'composite',
] as const;
export type RendererType = (typeof RENDERER_TYPES)[number];

export const LAYERS = ['under', 'subject', 'over', 'frame'] as const;
export type Layer = (typeof LAYERS)[number];

/** Device capabilities an entry may require; probed by the app at start-up. */
export const CAPABILITIES = [
  'subjectMask',
  'faceLandmarks',
  'animalPose',
  'playServices',
  'gyro',
] as const;
export type Capability = (typeof CAPABILITIES)[number];

/**
 * UX shown instead of hiding an entry when a requirement fails. There is no server-cutout fallback in the MVP
 * (prd-mvp AC-7.6: Android without Play services uses a bundled on-device model).
 */
export const FALLBACKS = ['userPoints'] as const;
export type Fallback = (typeof FALLBACKS)[number];

export const GPU_COSTS = ['S', 'M', 'L'] as const;
export type GpuCost = (typeof GPU_COSTS)[number];

export const REDUCED_MOTION = ['static', 'simplified', 'none'] as const;
export type ReducedMotion = (typeof REDUCED_MOTION)[number];

export const UNLOCKS = ['free', 'ad', 'pro'] as const;
export type Unlock = (typeof UNLOCKS)[number];

export const PARAM_UIS = ['slider', 'toggle', 'color', 'segmented', 'hidden'] as const;
export type ParamUi = (typeof PARAM_UIS)[number];

/** A tunable parameter. Ranges are inclusive; the server rejects values outside them (SR-28). */
export type ParamSpec =
  | { type: 'number'; min: number; max: number; default: number; ui: ParamUi }
  | { type: 'integer'; min: number; max: number; default: number; ui: ParamUi }
  | { type: 'boolean'; default: boolean; ui: ParamUi }
  | { type: 'color'; default: string; ui: ParamUi }
  | { type: 'enum'; values: readonly string[]; default: string; ui: ParamUi };

export type ParamValue = number | boolean | string;

export interface PlatformSupport {
  /** Minimum OS: iOS major version string, Android API level. */
  min: string | number;
  requires: readonly Capability[];
  fallback?: Fallback;
}

export interface EffectEntry {
  /** Stable forever, `kind.slug` (e.g. `cut.diecut`); stored in card and board JSON. */
  id: string;
  /** Bump on any param or visual change; stored items keep their version. */
  version: number;
  /** i18n keys (apps/mobile/src/i18n/messages). */
  name: string;
  description: string;
  a11yLabel: string;
  kind: EffectKind;
  /** Extra picker tabs, e.g. `finish` lists an `fx.*` entry under finishes too. */
  tags: readonly string[];
  tier: Tier;
  inputs: readonly EffectInput[];
  anchor: Anchor;
  params: Readonly<Record<string, ParamSpec>>;
  renderer: { type: RendererType; ref: string | readonly string[] };
  layer: Layer;
  stackable: boolean;
  conflictsWith: readonly string[];
  platforms: {
    ios: PlatformSupport;
    android: PlatformSupport;
    webPreview: 'baked' | 'live';
  };
  cost: { gpu: GpuCost; bakeMs: number; liveAllowed: boolean; maxLive: number };
  export: { still: boolean; loop: null | { seconds: number; fps: number; seamless: boolean } };
  motion: { reduced: ReducedMotion };
  /** Default unlock; remote config and region policy may override it. */
  unlock: Unlock;
  /**
   * `featureFlag`: shipped dark until `/v1/config` flags it on. `minAppVersion`: hidden on older builds.
   * Every entry can be switched off remotely through `/v1/config` kill switches.
   */
  flags: { featureFlag?: string; minAppVersion?: string };
  /** Named param sets shown as picker tiles (e.g. a thick sticker border); UI only, stored items keep params. */
  presets?: readonly Preset[];
}

export interface Preset {
  /** Slug, unique within the entry. */
  id: string;
  /** i18n key. */
  name: string;
  /** Overrides on top of the defaults; must pass the entry's param rules. */
  params: Readonly<Record<string, ParamValue>>;
}
