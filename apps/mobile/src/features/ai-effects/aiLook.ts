import type { CardDraft } from '../editor/editorState';
import { isTemplateId } from './aiEffectsApi';

/**
 * An AI effect result applied to a draft: the server-made `card_render` upload replaces the card render when the
 * card is sent. It belongs to the look it was made from (`basis`): once the shape, finish, effects or "whole photo"
 * change, the result no longer matches the card and is ignored (undo brings it back). The caption is not part of
 * the render, so it may change freely.
 */
export interface AiLook {
  readonly jobId: string;
  readonly templateId: string;
  readonly templateVersion: number;
  /** Server title when it was made, for the editor chip (bounded). */
  readonly title: string;
  /** The generated `card_render` upload (clean when applied). */
  readonly outputMediaId: string;
  /** {@link lookBasis} of the draft the input was baked from. */
  readonly basis: string;
}

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const BASIS = /^[0-9a-f]{8}$/;
const MAX_TITLE_LENGTH = 60;

/** FNV-1a 32-bit as 8 hex digits: a short, stable fingerprint (drafts must stay under 2 KB). */
function fnv1a(text: string): string {
  let hash = 0x811c9dc5;
  for (let i = 0; i < text.length; i += 1) {
    hash ^= text.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash.toString(16).padStart(8, '0');
}

/** Fingerprint of what the render shows (not the caption, not the AI look itself). */
export function lookBasis(draft: CardDraft): string {
  return fnv1a(JSON.stringify([draft.cut, draft.finish, draft.effects, draft.wholePhoto]));
}

/** The draft's AI look when it still matches the draft, else null. */
export function activeAiLook(draft: CardDraft): AiLook | null {
  const look = draft.ai;
  return look && look.basis === lookBasis(draft) ? look : null;
}

/** Reads a stored look defensively; anything malformed is dropped (the draft itself stays). */
export function parseAiLook(value: unknown): AiLook | null {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) return null;
  const v = value as Record<string, unknown>;
  if (typeof v.jobId !== 'string' || !UUID.test(v.jobId)) return null;
  if (typeof v.outputMediaId !== 'string' || !UUID.test(v.outputMediaId)) return null;
  if (typeof v.templateId !== 'string' || !isTemplateId(v.templateId)) return null;
  if (!Number.isSafeInteger(v.templateVersion) || (v.templateVersion as number) < 0) return null;
  if (typeof v.basis !== 'string' || !BASIS.test(v.basis)) return null;
  if (typeof v.title !== 'string' || v.title.length === 0 || v.title.length > MAX_TITLE_LENGTH) {
    return null;
  }
  return {
    jobId: v.jobId,
    templateId: v.templateId,
    templateVersion: v.templateVersion as number,
    title: v.title,
    outputMediaId: v.outputMediaId,
    basis: v.basis,
  };
}
