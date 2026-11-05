import type { EffectEntry, ParamSpec, ParamValue } from './schema';

export type ParamsResult =
  { ok: true; params: Record<string, ParamValue> } | { ok: false; errors: string[] };

const COLOR_VALUE = /^#[0-9A-Fa-f]{6}([0-9A-Fa-f]{2})?$/;

function checkValue(spec: ParamSpec, value: unknown): boolean {
  switch (spec.type) {
    case 'number':
      return (
        typeof value === 'number' &&
        Number.isFinite(value) &&
        value >= spec.min &&
        value <= spec.max
      );
    case 'integer':
      return (
        Number.isSafeInteger(value) &&
        (value as number) >= spec.min &&
        (value as number) <= spec.max
      );
    case 'boolean':
      return typeof value === 'boolean';
    case 'color':
      return typeof value === 'string' && COLOR_VALUE.test(value);
    case 'enum':
      return typeof value === 'string' && spec.values.includes(value);
  }
}

/**
 * Strict check of stored or received params (same rules the server applies, SR-28): unknown names and
 * out-of-range or mistyped values are errors; missing params take their defaults.
 */
export function resolveParams(entry: EffectEntry, input: unknown): ParamsResult {
  if (input === undefined || input === null) input = {};
  if (typeof input !== 'object' || Array.isArray(input)) {
    return { ok: false, errors: ['params must be an object'] };
  }
  const given = input as Record<string, unknown>;
  const errors = Object.keys(given)
    .filter((name) => !(name in entry.params))
    .map((name) => `unknown param ${name}`);
  const params: Record<string, ParamValue> = {};
  for (const [name, spec] of Object.entries(entry.params)) {
    const value = name in given ? given[name] : spec.default;
    if (checkValue(spec, value)) params[name] = value as ParamValue;
    else errors.push(`invalid value for ${name}`);
  }
  return errors.length > 0 ? { ok: false, errors } : { ok: true, params };
}

/** Defaults for every param of an entry. */
export function defaultParams(entry: EffectEntry): Record<string, ParamValue> {
  return Object.fromEntries(
    Object.entries(entry.params).map(([name, spec]) => [name, spec.default]),
  );
}

/** Clamps a UI-supplied number into the param's range (sliders, gestures). Non-finite input gives the default. */
export function clampNumberParam(spec: ParamSpec, value: number): number {
  if (spec.type !== 'number' && spec.type !== 'integer') {
    throw new TypeError(`clampNumberParam on a ${spec.type} param`);
  }
  if (!Number.isFinite(value)) return spec.default;
  const clamped = Math.min(spec.max, Math.max(spec.min, value));
  return spec.type === 'integer' ? Math.round(clamped) : clamped;
}

/**
 * Lenient version of {@link resolveParams} for UI state and previews: numbers clamp into range, other invalid or
 * missing values take their defaults, unknown names are dropped. Never use it to accept data from the network.
 */
export function clampParams(entry: EffectEntry, input: unknown): Record<string, ParamValue> {
  const given =
    input !== null && typeof input === 'object' && !Array.isArray(input)
      ? (input as Record<string, unknown>)
      : {};
  const params: Record<string, ParamValue> = {};
  for (const [name, spec] of Object.entries(entry.params)) {
    const value = given[name];
    if ((spec.type === 'number' || spec.type === 'integer') && typeof value === 'number') {
      params[name] = clampNumberParam(spec, value);
    } else {
      params[name] = checkValue(spec, value) ? (value as ParamValue) : spec.default;
    }
  }
  return params;
}
