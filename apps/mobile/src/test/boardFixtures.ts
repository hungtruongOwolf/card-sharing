import type { CardItem } from '../features/sharing/cardsApi';
import type { BoardPin, PinContent } from '../features/boards/boardModel';

let next = 0;

/** A visible card as the server's per-viewer resolution returns it. */
export function cardItem(over: Partial<CardItem> = {}): CardItem {
  next += 1;
  const id = `0192f0aa-0000-7000-8000-${String(next).padStart(12, '0')}`;
  return {
    id,
    sender: { id: 'u-1', displayName: 'Mai' },
    caption: null,
    altText: null,
    media: { renderUrl: `https://cdn.test/${id}.png?sig=x`, maskUrl: null, expiresAt: Infinity },
    finish: null,
    sentAt: 0,
    isMine: false,
    status: null,
    hold: null,
    myReaction: null,
    ...over,
  };
}

export function pin(over: Partial<BoardPin> = {}, content?: PinContent): BoardPin {
  next += 1;
  return {
    id: `0192f0bb-0000-7000-8000-${String(next).padStart(12, '0')}`,
    x: 0,
    y: 0,
    scale: 1,
    rotation: 0,
    zKey: 'a0',
    version: 1,
    content: content ?? { kind: 'card', card: cardItem() },
    ...over,
  };
}

export const holo = (intensity = 1) => ({ id: 'fx.holo', version: 1, params: { intensity } });
