import type { MessageKey } from '../../i18n';

/**
 * How the app draws each reaction id served by `GET /v1/config` (config/client.yaml `reactions`, US-17). The server
 * owns the set and its order; an id the app does not know yet is simply not offered.
 */
export const REACTION_DISPLAY: Readonly<Record<string, { glyph: string; labelKey: MessageKey }>> = {
  'react.heart': { glyph: '❤️', labelKey: 'feed.reaction.heart' },
  'react.laugh': { glyph: '😂', labelKey: 'feed.reaction.laugh' },
  'react.wow': { glyph: '😮', labelKey: 'feed.reaction.wow' },
  'react.sad': { glyph: '😢', labelKey: 'feed.reaction.sad' },
  'react.fire': { glyph: '🔥', labelKey: 'feed.reaction.fire' },
  'react.paw': { glyph: '🐾', labelKey: 'feed.reaction.paw' },
  'react.squish': { glyph: '🫧', labelKey: 'feed.reaction.squish' },
};

/** The served set in its order, keeping only ids the app can draw. */
export function shownReactions(served: readonly string[] | undefined): string[] {
  return (served ?? []).filter((id) => id in REACTION_DISPLAY);
}
