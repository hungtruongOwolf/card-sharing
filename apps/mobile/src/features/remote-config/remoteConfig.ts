import type { RemoteRegistryConfig } from '@card-sharing/effects-registry';

import { parseTextFilterConfig, type TextFilterConfig } from '../../text';

/**
 * Public client config (`GET /v1/config`, T-27): feature flags, registry kill switches and product limits.
 * Unauthenticated and CDN-cached, so it carries no user data. Display only: the server enforces its own copy.
 * Read so far: the registry part, the caption and recipient limits and the text filter hints.
 */
export interface RemoteConfig {
  registry: RemoteRegistryConfig;
  /** `limits.caption_max_chars` (grapheme clusters), when present and sane. */
  captionMaxGraphemes?: number;
  /** `text_filter`: term lists for caption hints (#213), when present and well formed. */
  textFilter?: TextFilterConfig;
  /** `limits.card_recipients_max`: most friends one card can go to (T-77), when present and sane. */
  cardRecipientsMax?: number;
  /** `reactions` minus `kill_switches.reactions`, in display order (US-17); undefined when not served. */
  reactions?: string[];
}

const REACTION_ID = /^react\.[a-z0-9_-]{1,40}$/;

/** Upper bound accepted for a served length limit; anything else falls back to the app's default. */
const MAX_SERVED_LIMIT = 1000;

const positiveLimit = (value: unknown): number | undefined =>
  Number.isSafeInteger(value) && (value as number) > 0 && (value as number) <= MAX_SERVED_LIMIT
    ? (value as number)
    : undefined;

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
  const captionMaxGraphemes = isRecord(body.limits)
    ? positiveLimit(body.limits.caption_max_chars)
    : undefined;
  const cardRecipientsMax = isRecord(body.limits)
    ? positiveLimit(body.limits.card_recipients_max)
    : undefined;
  const textFilter = parseTextFilterConfig(body.text_filter);
  const killedReactions = new Set(stringList(kill.reactions));
  const reactions = Array.isArray(body.reactions)
    ? [...new Set(stringList(body.reactions))].filter(
        (id) => REACTION_ID.test(id) && !killedReactions.has(id),
      )
    : undefined;
  return {
    ...(reactions === undefined ? {} : { reactions }),
    ...(captionMaxGraphemes === undefined ? {} : { captionMaxGraphemes }),
    ...(cardRecipientsMax === undefined ? {} : { cardRecipientsMax }),
    ...(textFilter === undefined ? {} : { textFilter }),
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
