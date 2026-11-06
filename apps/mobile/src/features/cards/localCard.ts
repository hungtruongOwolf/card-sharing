import { resolveParams, type ParamValue, type Registry } from '@card-sharing/effects-registry';

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
}

export interface StyleRef {
  id: string;
  version: number;
  params: Record<string, ParamValue>;
}

export type StyleBlockResult = { ok: true; style: StyleBlock } | { ok: false; error: string };

/**
 * The strict send-time check (the editor clamps leniently; the server validates again, SR-28): every id must
 * exist at exactly the stored version and fit its slot, and params must pass `resolveParams`.
 */
export function styleBlock(registry: Registry, draft: CardDraft): StyleBlockResult {
  const cut = styleRef(registry, draft.cut, 'cutStyle');
  if (!cut.ok) return cut;
  if (!draft.finish) return { ok: true, style: { cut: cut.ref } };
  const finish = styleRef(registry, draft.finish, 'filter');
  if (!finish.ok) return finish;
  return { ok: true, style: { cut: cut.ref, finish: finish.ref } };
}

function styleRef(
  registry: Registry,
  choice: StyleChoice,
  kind: 'cutStyle' | 'filter',
): { ok: true; ref: StyleRef } | { ok: false; error: string } {
  const entry = registry.get(choice.id);
  if (!entry || entry.kind !== kind) return { ok: false, error: `unknown ${choice.id}` };
  if (entry.version !== choice.version) return { ok: false, error: `stale ${choice.id}` };
  const params = resolveParams(entry, choice.params);
  if (!params.ok) return { ok: false, error: `${choice.id}: ${params.errors.join(', ')}` };
  return { ok: true, ref: { id: entry.id, version: entry.version, params: params.params } };
}
