/**
 * Registry ledger (T-124, change-management): every id that ever shipped, append-only, and what stored references
 * to a retired id become. Stored cards, drafts and boards keep `{id, version, params}` forever, so an entry can
 * only leave `ENTRIES` together with a `RETIRED` migration; a test fails otherwise. Add new ids here in the same change
 * that adds the entry (a test fails when one is missing).
 */
export const SHIPPED_IDS: readonly string[] = [
  'bg.cork',
  'bg.dots',
  'bg.grid',
  'bg.kraft',
  'bg.lined',
  'bg.paper',
  'cut.diecut',
  'cut.none',
  'cut.raw',
  'cut.shape.circle',
  'cut.shape.heart',
  'cut.shape.rounded-square',
  'cut.torn',
  'deco.arrow',
  'deco.heart',
  'deco.pushpin',
  'deco.ribbon',
  'deco.squiggle',
  'deco.star',
  'deco.ticket',
  'deco.washi-dots',
  'deco.washi-grid',
  'deco.washi-stripes',
  'frame.polaroid',
  'fx.fire-breath',
  'fx.glossy',
  'fx.holo',
  'fx.laser-eyes',
  'fx.matte',
  'fx.paper-lift',
  'fx.sparkles',
  'overlay.speech',
  'overlay.text',
  'overlay.washi',
  'prop.bow',
  'prop.cat-ears',
  'prop.crown',
  'prop.halo',
  'prop.heart-glasses',
  'prop.mustache',
  'prop.party-hat',
  'prop.sunglasses',
];

/**
 * Retired id → the id that replaces it in stored references, or null when the reference is dropped (an effect or
 * finish disappears; a cut style falls back to the editor default). Empty until an entry is removed.
 */
export const RETIRED: Readonly<Record<string, string | null>> = {};

/** What a stored id becomes: itself, its replacement, or null (dropped). */
export function migrateId(
  id: string,
  retired: Readonly<Record<string, string | null>> = RETIRED,
): string | null {
  return id in retired ? (retired[id] ?? null) : id;
}
