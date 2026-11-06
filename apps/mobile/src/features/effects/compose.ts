import {
  clampParams,
  defaultParams,
  type EffectEntry,
  type ParamValue,
  type Registry,
} from '@card-sharing/effects-registry';

import type { EffectRenderers } from './effectRenderers';
import { EFFECT_RENDERERS } from './shippedRenderers';

/**
 * Composition order of a card's effects (T-114, effects-catalog.md §1): cut style → filters → `under` / `subject`
 * / `over` entries → frame. Within a layer the user's order is kept (later on top).
 */

/** A stored choice as the registry stores it on a card. */
export interface EffectChoice {
  id: string;
  version: number;
  params: Readonly<Record<string, ParamValue>>;
}

export interface ResolvedEffect {
  entry: EffectEntry;
  params: Record<string, ParamValue>;
}

export interface LayeredEffects {
  /** Filter-kind effects, painted on the card's own pixels after the finish. */
  filters: ResolvedEffect[];
  under: ResolvedEffect[];
  subject: ResolvedEffect[];
  over: ResolvedEffect[];
  /** At most one frame (frames are not stackable); a second one is ignored. */
  frame: ResolvedEffect | null;
}

/**
 * Splits a card's effects into draw layers. Unknown ids (an entry removed from the registry, a draft from a newer
 * build) are skipped rather than crashing the editor or a bake; params are clamped to the registry ranges.
 */
export function layerEffects(registry: Registry, choices: readonly EffectChoice[]): LayeredEffects {
  const layers: LayeredEffects = { filters: [], under: [], subject: [], over: [], frame: null };
  for (const choice of choices) {
    const entry = registry.get(choice.id);
    if (!entry) continue;
    const resolved = { entry, params: clampParams(entry, choice.params) };
    if (entry.kind === 'filter') layers.filters.push(resolved);
    else if (entry.layer === 'frame') layers.frame ??= resolved;
    else layers[entry.layer].push(resolved);
  }
  return layers;
}

/**
 * Built-in effects a card gets without choosing them (tag `builtIn`, e.g. paper lift: "built into every sticker",
 * effects-catalog.md §3.3). They are not user choices, so they are never stored on the draft (AC-21.4 is about
 * effects the user picks): they apply to sticker cards (the cut style traces the subject and the card has one),
 * with default params, unless switched off remotely.
 */
export function builtInEffects(
  registry: Registry,
  cut: EffectEntry,
  hasSubject: boolean,
  killed: ReadonlySet<string> = new Set(),
  renderers: EffectRenderers = EFFECT_RENDERERS,
): ResolvedEffect[] {
  const sticker = hasSubject && (cut.inputs.includes('mask') || cut.inputs.includes('contour'));
  if (!sticker) return [];
  return registry
    .byTag('builtIn')
    .filter((entry) => !killed.has(entry.id) && hasEffectRenderer(entry, renderers))
    .map((entry) => ({ entry, params: defaultParams(entry) }));
}

/**
 * Whether the app can draw a non-filter effect entry (filters are checked against the finish renderers by the
 * editor). Entries without a renderer are never offered.
 */
export function hasEffectRenderer(
  entry: EffectEntry,
  renderers: EffectRenderers = EFFECT_RENDERERS,
): boolean {
  if (typeof entry.renderer.ref !== 'string') return false;
  const table = entry.layer === 'frame' ? renderers.frames : renderers.effects;
  return entry.renderer.ref in table;
}
