/**
 * Hands a re-auth link from the tab it opened in to the tab that asked for it (ADR-0012).
 *
 * A re-auth link opens in a new browser tab, but the PKCE verifier lives only in the memory of the tab that asked
 * (its "confirm it's you" sheet), so the new tab passes `{requestId, token}` over a `BroadcastChannel` (same origin
 * only, nothing persisted) and the asking tab answers `ack`. Listeners in the same JS context hear the messages too,
 * which is how it works on native, where the link opens in the same app. The token alone is useless: redeeming it
 * needs the asking tab's verifier and its session.
 */

export type MagicLinkMessage =
  | { readonly type: 'link'; readonly requestId: string; readonly token: string }
  | { readonly type: 'ack'; readonly requestId: string };

export interface MagicLinkChannel {
  post(message: MagicLinkMessage): void;
  subscribe(listener: (message: MagicLinkMessage) => void): () => void;
}

const CHANNEL_NAME = 'cs.magic-link';
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const TOKEN = /^[A-Za-z0-9_-]{43}$/;

/** Only well-formed messages get through (another script on the origin could post anything). */
export function parseMagicLinkMessage(value: unknown): MagicLinkMessage | null {
  if (typeof value !== 'object' || value === null) return null;
  const { type, requestId, token } = value as Record<string, unknown>;
  if (typeof requestId !== 'string' || !UUID.test(requestId)) return null;
  if (type === 'ack') return { type, requestId };
  if (type === 'link' && typeof token === 'string' && TOKEN.test(token)) {
    return { type, requestId, token };
  }
  return null;
}

interface BroadcastLike {
  postMessage(message: unknown): void;
  addEventListener(type: 'message', listener: (event: { data: unknown }) => void): void;
  /** Node only (tests): do not keep the process alive. */
  unref?(): void;
}

type BroadcastConstructor = new (name: string) => BroadcastLike;

export function createMagicLinkChannel(
  Broadcast: BroadcastConstructor | undefined = (
    globalThis as { BroadcastChannel?: BroadcastConstructor }
  ).BroadcastChannel,
): MagicLinkChannel {
  const listeners = new Set<(message: MagicLinkMessage) => void>();
  const deliver = (value: unknown) => {
    const message = parseMagicLinkMessage(value);
    if (message) for (const listener of [...listeners]) listener(message);
  };
  let broadcast: BroadcastLike | null = null;
  try {
    broadcast = Broadcast ? new Broadcast(CHANNEL_NAME) : null;
    broadcast?.addEventListener('message', (event) => deliver(event.data));
    broadcast?.unref?.();
  } catch {
    broadcast = null;
  }
  return {
    post(message) {
      broadcast?.postMessage(message);
      // A BroadcastChannel does not deliver to its own sender: same-context listeners hear it here.
      deliver(message);
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
}

let appChannel: MagicLinkChannel | null = null;

/** The app's channel (one per JS context). */
export function appMagicLinkChannel(): MagicLinkChannel {
  return (appChannel ??= createMagicLinkChannel());
}
