import {
  ANCHORS,
  CAPABILITIES,
  EFFECT_KINDS,
  FALLBACKS,
  GPU_COSTS,
  INPUTS,
  KIND_PREFIXES,
  LAYERS,
  PARAM_UIS,
  REDUCED_MOTION,
  RENDERER_TYPES,
  TIERS,
  UNLOCKS,
  type EffectEntry,
  type ParamSpec,
  type PlatformSupport,
} from './schema';
import { resolveParams } from './params';

/**
 * Hand-written validation of registry entries (no schema library: zod is not a workspace dependency and the
 * rules below are mostly cross-field checks a schema library would not express anyway).
 */

export const ID_PATTERN = /^[a-z]+(\.[a-z0-9]+(-[a-z0-9]+)*)+$/;
const I18N_KEY_PATTERN = /^[a-z][a-zA-Z0-9]*(\.[a-zA-Z0-9-]+)+$/;
const PARAM_NAME_PATTERN = /^[a-z][a-zA-Z0-9]*$/;
const COLOR_PATTERN = /^#[0-9A-F]{6}([0-9A-F]{2})?$/;
const SEMVER_PATTERN = /^\d+\.\d+\.\d+$/;

const isOneOf = <T extends string>(values: readonly T[], value: unknown): value is T =>
  typeof value === 'string' && (values as readonly string[]).includes(value);

const isPositiveInt = (value: number) => Number.isSafeInteger(value) && value > 0;

function paramErrors(name: string, spec: ParamSpec): string[] {
  const at = `param ${name}`;
  const errors: string[] = [];
  if (!PARAM_NAME_PATTERN.test(name)) errors.push(`${at}: name must be camelCase`);
  if (!isOneOf(PARAM_UIS, spec.ui)) errors.push(`${at}: unknown ui ${String(spec.ui)}`);
  switch (spec.type) {
    case 'number':
    case 'integer': {
      const { min, max, default: def } = spec;
      if (![min, max, def].every(Number.isFinite))
        errors.push(`${at}: min/max/default must be finite`);
      if (!(min < max)) errors.push(`${at}: min must be below max`);
      if (!(def >= min && def <= max)) errors.push(`${at}: default ${def} outside ${min}..${max}`);
      if (spec.type === 'integer' && ![min, max, def].every(Number.isSafeInteger)) {
        errors.push(`${at}: integer bounds and default must be integers`);
      }
      break;
    }
    case 'boolean':
      if (typeof spec.default !== 'boolean') errors.push(`${at}: default must be a boolean`);
      break;
    case 'color':
      if (!COLOR_PATTERN.test(spec.default))
        errors.push(`${at}: default must be #RRGGBB(AA) uppercase`);
      break;
    case 'enum':
      if (spec.values.length < 2) errors.push(`${at}: enum needs at least two values`);
      if (new Set(spec.values).size !== spec.values.length)
        errors.push(`${at}: duplicate enum values`);
      if (!spec.values.includes(spec.default)) errors.push(`${at}: default not in values`);
      break;
    default:
      errors.push(`${at}: unknown type`);
  }
  return errors;
}

function platformErrors(name: string, platform: PlatformSupport): string[] {
  const errors: string[] = [];
  for (const capability of platform.requires) {
    if (!isOneOf(CAPABILITIES, capability))
      errors.push(`${name}: unknown capability ${capability}`);
  }
  if (platform.fallback !== undefined && !isOneOf(FALLBACKS, platform.fallback)) {
    errors.push(`${name}: unknown fallback ${platform.fallback}`);
  }
  return errors;
}

/** Returns every rule an entry breaks (empty when valid). Cross-entry rules live in {@link validateRegistry}. */
export function validateEntry(entry: EffectEntry): string[] {
  const errors: string[] = [];
  if (!ID_PATTERN.test(entry.id)) errors.push('id must be kind.slug (lowercase, dot separated)');
  if (!isOneOf(EFFECT_KINDS, entry.kind)) {
    errors.push(`unknown kind ${String(entry.kind)}`);
  } else if (!KIND_PREFIXES[entry.kind].includes(entry.id.split('.')[0] ?? '')) {
    errors.push(`id prefix does not match kind ${entry.kind}`);
  }
  if (!isPositiveInt(entry.version)) errors.push('version must be a positive integer');
  for (const key of [entry.name, entry.description, entry.a11yLabel]) {
    if (!I18N_KEY_PATTERN.test(key)) errors.push(`not an i18n key: ${key}`);
  }
  if (!isOneOf(TIERS, entry.tier)) errors.push(`unknown tier ${String(entry.tier)}`);
  if (entry.inputs.length === 0) errors.push('inputs must not be empty');
  for (const input of entry.inputs) {
    if (!isOneOf(INPUTS, input)) errors.push(`unknown input ${input}`);
  }
  if (!isOneOf(ANCHORS, entry.anchor)) errors.push(`unknown anchor ${String(entry.anchor)}`);
  for (const [name, spec] of Object.entries(entry.params)) errors.push(...paramErrors(name, spec));
  if (!isOneOf(RENDERER_TYPES, entry.renderer.type)) errors.push('unknown renderer type');
  const refs = typeof entry.renderer.ref === 'string' ? [entry.renderer.ref] : entry.renderer.ref;
  if (refs.length === 0 || refs.some((ref) => ref.trim() === ''))
    errors.push('renderer ref is empty');
  if (entry.renderer.type === 'composite' && refs.length < 2)
    errors.push('composite needs 2+ refs');
  if (!isOneOf(LAYERS, entry.layer)) errors.push(`unknown layer ${String(entry.layer)}`);
  if (entry.conflictsWith.includes(entry.id)) errors.push('an entry cannot conflict with itself');
  errors.push(...platformErrors('ios', entry.platforms.ios));
  errors.push(...platformErrors('android', entry.platforms.android));
  const { cost } = entry;
  if (!isOneOf(GPU_COSTS, cost.gpu)) errors.push('unknown gpu cost');
  if (!(Number.isFinite(cost.bakeMs) && cost.bakeMs >= 0)) errors.push('bakeMs must be >= 0');
  if (!Number.isSafeInteger(cost.maxLive) || cost.maxLive < 0) errors.push('maxLive must be >= 0');
  if (cost.liveAllowed !== cost.maxLive > 0) errors.push('liveAllowed must match maxLive > 0');
  if (entry.export.loop !== null && !entry.inputs.includes('time')) {
    errors.push('only time-driven effects may export a loop');
  }
  if (!isOneOf(REDUCED_MOTION, entry.motion.reduced)) errors.push('unknown reduced-motion mode');
  if (!isOneOf(UNLOCKS, entry.unlock)) errors.push(`unknown unlock ${String(entry.unlock)}`);
  const { minAppVersion } = entry.flags;
  if (minAppVersion !== undefined && !SEMVER_PATTERN.test(minAppVersion)) {
    errors.push('minAppVersion must be x.y.z');
  }
  errors.push(...presetErrors(entry));
  return errors;
}

const PRESET_ID_PATTERN = /^[a-z0-9]+(-[a-z0-9]+)*$/;

function presetErrors(entry: EffectEntry): string[] {
  const errors: string[] = [];
  const ids = new Set<string>();
  for (const preset of entry.presets ?? []) {
    const at = `preset ${preset.id}`;
    if (!PRESET_ID_PATTERN.test(preset.id)) errors.push(`${at}: id must be a slug`);
    if (ids.has(preset.id)) errors.push(`${at}: duplicate preset id`);
    ids.add(preset.id);
    if (!I18N_KEY_PATTERN.test(preset.name)) errors.push(`${at}: not an i18n key: ${preset.name}`);
    const result = resolveParams(entry, preset.params);
    if (!result.ok) errors.push(...result.errors.map((error) => `${at}: ${error}`));
  }
  return errors;
}

/** Validates every entry plus cross-entry rules: unique ids and `conflictsWith` pointing at real entries. */
export function validateRegistry(entries: readonly EffectEntry[]): string[] {
  const errors: string[] = [];
  const ids = new Set<string>();
  for (const entry of entries) {
    if (ids.has(entry.id)) errors.push(`${entry.id}: duplicate id`);
    ids.add(entry.id);
    errors.push(...validateEntry(entry).map((error) => `${entry.id}: ${error}`));
  }
  for (const entry of entries) {
    for (const other of entry.conflictsWith) {
      if (!ids.has(other)) errors.push(`${entry.id}: conflictsWith unknown id ${other}`);
    }
  }
  return errors;
}
