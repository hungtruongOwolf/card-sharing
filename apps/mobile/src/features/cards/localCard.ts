import {
  EFFECT_SLOT_KINDS,
  MAX_EFFECTS,
  resolveParams,
  stackingErrors,
  type EffectEntry,
  type EffectKind,
  type ParamValue,
  type Registry,
} from '@card-sharing/effects-registry';

import type { CapturedPhoto } from '../capture';
import type { CardDraft, StyleChoice } from '../editor/editorState';
import { newIdempotencyKey } from '../friends/idempotencyKey';

/**
 * The local card model (T-67): what a card is before it is sent. It keeps the photo reference and the editable
 * choices (`{id, version, params}` per style), never only a rendered image, so the card stays re-editable until it
 * is sent (AC-21.5); the render is baked from it at send time. Lives only on the device.
 */
export const LOCAL_CARD_SCHEMA = 1;

export interface LocalCard {
  /** Client id, reused as the `client_card_id` / Idempotency-Key when the card is sent. */
  id: string;
  schema: typeof LOCAL_CARD_SCHEMA;
  photo: CapturedPhoto;
  draft: CardDraft;
  /** ISO-8601 timestamps. */
  createdAt: string;
  updatedAt: string;
}

export function newLocalCard(
  photo: CapturedPhoto,
  draft: CardDraft,
  now: Date = new Date(),
  id: string = newIdempotencyKey(),
): LocalCard {
  const at = now.toISOString();
  return { id, schema: LOCAL_CARD_SCHEMA, photo, draft, createdAt: at, updatedAt: at };
}

export function withDraft(card: LocalCard, draft: CardDraft, now: Date = new Date()): LocalCard {
  return { ...card, draft, updatedAt: now.toISOString() };
}

/** `style` block of `POST /v1/cards` (api-mvp.md). */
export interface StyleBlock {
  cut: StyleRef;
  finish?: StyleRef;
  /** Stacked effects in draw order of choice; omitted when there are none. */
  effects?: StyleRef[];
}

export interface StyleRef {
  id: string;
  version: number;
  params: Record<string, ParamValue>;
}

export type StyleBlockResult = { ok: true; style: StyleBlock } | { ok: false; error: string };

/**
 * The strict send-time check (the editor clamps leniently; the server validates again, SR-28): every id must
 * exist at exactly the stored version and fit its slot, params must pass `resolveParams`, and the chosen entries
 * must stack (no duplicate non-stackable entry, no `conflictsWith` pair, at most `MAX_EFFECTS` effects).
 */
export function styleBlock(registry: Registry, draft: CardDraft): StyleBlockResult {
  const cut = styleRef(registry, draft.cut, ['cutStyle']);
  if (!cut.ok) return cut;
  const style: StyleBlock = { cut: cut.ref };
  const chosen: EffectEntry[] = [cut.entry];
  if (draft.finish) {
    const finish = styleRef(registry, draft.finish, ['filter']);
    if (!finish.ok) return finish;
    style.finish = finish.ref;
    chosen.push(finish.entry);
  }
  if (draft.effects.length > MAX_EFFECTS) return { ok: false, error: 'too many effects' };
  const effects: StyleRef[] = [];
  for (const choice of draft.effects) {
    const effect = styleRef(registry, choice, EFFECT_SLOT_KINDS);
    if (!effect.ok) return effect;
    effects.push(effect.ref);
    chosen.push(effect.entry);
  }
  const stacking = stackingErrors(chosen);
  if (stacking.length > 0) return { ok: false, error: stacking.join(', ') };
  if (effects.length > 0) style.effects = effects;
  return { ok: true, style };
}

function styleRef(
  registry: Registry,
  choice: StyleChoice,
  kinds: readonly EffectKind[],
): { ok: true; ref: StyleRef; entry: EffectEntry } | { ok: false; error: string } {
  const entry = registry.get(choice.id);
  if (!entry || !kinds.includes(entry.kind)) return { ok: false, error: `unknown ${choice.id}` };
  if (entry.version !== choice.version) return { ok: false, error: `stale ${choice.id}` };
  const params = resolveParams(entry, choice.params);
  if (!params.ok) return { ok: false, error: `${choice.id}: ${params.errors.join(', ')}` };
  return { ok: true, entry, ref: { id: entry.id, version: entry.version, params: params.params } };
}
