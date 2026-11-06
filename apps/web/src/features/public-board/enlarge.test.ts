import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { describe, it } from 'node:test';

import { CARD_ATTRIBUTE, cardIndex, cardLabel } from './enlarge.ts';

describe('cardIndex', () => {
  it('accepts only an in-range decimal index', () => {
    assert.equal(cardIndex('0', 3), 0);
    assert.equal(cardIndex('2', 3), 2);
    assert.equal(cardIndex('3', 3), null);
    assert.equal(cardIndex('-1', 3), null);
    assert.equal(cardIndex('1e1', 30), null);
    assert.equal(cardIndex('', 3), null);
    assert.equal(cardIndex(null, 3), null);
    assert.equal(cardIndex(undefined, 3), null);
  });
});

describe('cardLabel', () => {
  it('numbers cards from one', () => {
    assert.equal(cardLabel(0, 3), 'Card 1 of 3');
  });
});

describe('enlarge wiring', () => {
  const read = (path: string) => readFileSync(new URL(path, import.meta.url), 'utf8');
  const page = read('../../app/b/[token]/page.tsx');
  const dialog = read('./CardEnlarge.tsx');

  it('marks card buttons with the attribute the dialog reads', () => {
    assert.ok(page.includes(`${CARD_ATTRIBUTE}={ordinal}`));
    assert.match(page, /aria-haspopup="dialog"/);
  });

  it('uses a labelled native modal dialog and the baked still only (no CanvasKit)', () => {
    assert.match(dialog, /showModal\(\)/);
    assert.match(dialog, /aria-labelledby=/);
    assert.equal(/canvaskit|holo\/renderer/i.test(dialog + page), false);
  });
});
