import type { FontRole } from '@card-sharing/tokens';

/**
 * Maps a token font role to a loaded font family name.
 *
 * The brand fonts (Bricolage Grotesque, DM Sans, Caveat, Anton, DM Mono) are not bundled yet, and an
 * unknown `fontFamily` logs an error on iOS, so every role resolves to the platform font for now.
 * When the font assets land, register them with expo-font and return their names here; no component changes.
 */
const loaded: Partial<Record<FontRole, string>> = {};

export function fontFamilyFor(role: FontRole): string | undefined {
  return loaded[role];
}
