import type { Capability, EffectEntry, Fallback, PlatformSupport, WebSupport } from './schema';

/** The `GET /v1/config` fields routing needs (shape of the API's `AppConfigResponse`). */
export interface RemoteRegistryConfig {
  flags?: Readonly<Record<string, boolean>>;
  kill_switches?: Readonly<
    Partial<Record<'cut_styles' | 'finishes' | 'effects' | 'reactions', readonly string[]>>
  >;
}

interface DeviceBase {
  appVersion: string;
  capabilities: ReadonlySet<Capability>;
}

/**
 * This device as the registry sees it. A native device carries its OS version; the web target (ADR-0009) has none
 * to compare (a browser is not an iOS or Android release), only the capabilities the page can provide.
 */
export type DeviceContext =
  | (DeviceBase & {
      platform: 'ios' | 'android';
      /** iOS version (`"17.4"`) or Android API level (`34`). */
      osVersion: string | number;
    })
  | (DeviceBase & { platform: 'web' });

export type HiddenReason = 'killSwitch' | 'featureFlag' | 'minAppVersion' | 'os' | 'requires';

export type Availability =
  | { status: 'available' }
  | { status: 'fallback'; fallback: Fallback; missing: Capability[] }
  | { status: 'hidden'; reason: HiddenReason };

/**
 * Every id switched off remotely. Ids are unique across kinds, so the per-group lists of the config
 * (`cut_styles`, `finishes`, `effects`, `reactions`) merge into one set; a misfiled id still switches off.
 */
export function killedIds(config: RemoteRegistryConfig | undefined): ReadonlySet<string> {
  return new Set(Object.values(config?.kill_switches ?? {}).flatMap((ids) => ids ?? []));
}

function numericParts(version: string | number): number[] {
  return String(version)
    .split('.')
    .map((part) => Number.parseInt(part, 10))
    .map((part) => (Number.isFinite(part) ? part : 0));
}

/** Compares dotted versions numerically (`"17"` vs `"17.4"`, `"0.10.0"` vs `"0.9.1"`). */
export function compareVersions(a: string | number, b: string | number): number {
  const left = numericParts(a);
  const right = numericParts(b);
  for (let i = 0; i < Math.max(left.length, right.length); i++) {
    const diff = (left[i] ?? 0) - (right[i] ?? 0);
    if (diff !== 0) return Math.sign(diff);
  }
  return 0;
}

/** What the entry needs on this device's platform. */
export function platformSupport(
  entry: EffectEntry,
  device: DeviceContext,
): PlatformSupport | WebSupport {
  return entry.platforms[device.platform];
}

/** The capabilities the entry needs on this device's platform that the device does not have. */
export function missingCapabilities(entry: EffectEntry, device: DeviceContext): Capability[] {
  return platformSupport(entry, device).requires.filter(
    (capability) => !device.capabilities.has(capability),
  );
}

/**
 * Decides whether the app shows an entry on this device, in order: remote kill switch, feature flag
 * (entries ship dark), app version, OS version (native only), then capabilities, where a declared fallback UX
 * (e.g. tap-the-eyes) keeps the entry visible. Display only: the server validates stored styles itself.
 */
export function resolveAvailability(
  entry: EffectEntry,
  device: DeviceContext,
  config?: RemoteRegistryConfig,
  killed: ReadonlySet<string> = killedIds(config),
): Availability {
  if (killed.has(entry.id)) return { status: 'hidden', reason: 'killSwitch' };
  const { featureFlag, minAppVersion } = entry.flags;
  if (featureFlag !== undefined && config?.flags?.[featureFlag] !== true) {
    return { status: 'hidden', reason: 'featureFlag' };
  }
  if (minAppVersion !== undefined && compareVersions(device.appVersion, minAppVersion) < 0) {
    return { status: 'hidden', reason: 'minAppVersion' };
  }
  if (
    device.platform !== 'web' &&
    compareVersions(device.osVersion, entry.platforms[device.platform].min) < 0
  ) {
    return { status: 'hidden', reason: 'os' };
  }
  const missing = missingCapabilities(entry, device);
  if (missing.length === 0) return { status: 'available' };
  const { fallback } = platformSupport(entry, device);
  if (fallback !== undefined) return { status: 'fallback', fallback, missing };
  return { status: 'hidden', reason: 'requires' };
}

/** Entries the picker shows (available or with a fallback UX), in registry order. */
export function visibleEntries(
  entries: readonly EffectEntry[],
  device: DeviceContext,
  config?: RemoteRegistryConfig,
): { entry: EffectEntry; availability: Exclude<Availability, { status: 'hidden' }> }[] {
  const killed = killedIds(config);
  return entries.flatMap((entry) => {
    const availability = resolveAvailability(entry, device, config, killed);
    return availability.status === 'hidden' ? [] : [{ entry, availability }];
  });
}
