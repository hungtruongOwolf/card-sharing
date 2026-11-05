import type { RemoteRegistryConfig } from '@card-sharing/effects-registry';

/**
 * Public client config (`GET /v1/config`, T-27): feature flags, registry kill switches and product limits.
 * Unauthenticated and CDN-cached, so it carries no user data. Display only: the server enforces its own copy.
 * Only the registry part is read so far; product limits join when a screen needs them.
 */
export interface RemoteConfig {
  registry: RemoteRegistryConfig;
}

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

const stringList = (value: unknown): string[] =>
  Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string') : [];

/**
 * Narrows an untrusted response body. Malformed parts become empty (no flags on, nothing killed) instead of
 * throwing, so one bad field cannot take the editor down.
 */
export function parseRemoteConfig(body: unknown): RemoteConfig | null {
  if (!isRecord(body)) return null;
  const flags = isRecord(body.flags)
    ? Object.fromEntries(Object.entries(body.flags).filter(([, on]) => typeof on === 'boolean'))
    : {};
  const kill = isRecord(body.kill_switches) ? body.kill_switches : {};
  return {
    registry: {
      flags: flags as Record<string, boolean>,
      kill_switches: {
        cut_styles: stringList(kill.cut_styles),
        finishes: stringList(kill.finishes),
        effects: stringList(kill.effects),
        reactions: stringList(kill.reactions),
      },
    },
  };
}

export type FetchFn = (input: string, init?: RequestInit) => Promise<Response>;

export async function fetchRemoteConfig(
  baseUrl: string,
  fetchFn: FetchFn = (input, init) => fetch(input, init),
): Promise<RemoteConfig> {
  const response = await fetchFn(`${baseUrl}/v1/config`, {
    headers: { Accept: 'application/json' },
  });
  if (!response.ok) throw new Error(`GET /v1/config failed with ${response.status}`);
  const config = parseRemoteConfig(await response.json());
  if (!config) throw new Error('GET /v1/config returned an unexpected body');
  return config;
}
