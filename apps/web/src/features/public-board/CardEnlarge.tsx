'use client';

import { type MouseEvent, type ReactNode, useId, useRef, useState } from 'react';

import { CARD_ATTRIBUTE, type EnlargeCard, cardIndex } from './enlarge';

interface Props {
  cards: readonly EnlargeCard[];
  /** The server-rendered pins; card pins are buttons carrying `data-card`. */
  children: ReactNode;
}

/**
 * The board stage plus one modal dialog that shows a tapped card's baked still large (T-135,
 * US-25). One delegated click handler serves every card button, so the pins stay server-rendered
 * and the page ships only this small component. The native modal `<dialog>` traps focus and closes
 * on Escape; focus returns to the card that opened it.
 */
export function CardEnlarge({ cards, children }: Props) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const openerRef = useRef<HTMLElement | null>(null);
  const [current, setCurrent] = useState<number | null>(null);
  const titleId = useId();
  const card = current === null ? undefined : cards[current];

  const onStageClick = (event: MouseEvent<HTMLElement>) => {
    const target = event.target instanceof Element ? event.target : null;
    const button = target?.closest<HTMLElement>(`button[${CARD_ATTRIBUTE}]`) ?? null;
    const index = cardIndex(button?.getAttribute(CARD_ATTRIBUTE), cards.length);
    if (button === null || index === null) {
      return;
    }
    openerRef.current = button;
    setCurrent(index);
    dialogRef.current?.showModal();
  };

  return (
    <>
      <section className="board-stage" aria-label="Board" onClick={onStageClick}>
        {children}
      </section>
      <dialog
        ref={dialogRef}
        className="enlarge-dialog"
        aria-labelledby={titleId}
        onClose={() => {
          setCurrent(null);
          openerRef.current?.focus();
        }}
        onClick={(event) => {
          // A click on the backdrop (the dialog element itself) closes it.
          if (event.target === dialogRef.current) dialogRef.current?.close();
        }}
      >
        <h2 id={titleId} className="enlarge-title">
          {card?.label ?? ''}
        </h2>
        {card && (
          // Plain <img>: a signed URL of a baked derivative; next/image adds a style attribute
          // that the CSP blocks.
          // eslint-disable-next-line @next/next/no-img-element
          <img
            src={card.src}
            alt={card.label}
            width={card.width}
            height={card.height}
            className="enlarge-image"
            referrerPolicy="no-referrer"
          />
        )}
        {/* The only focusable control: showModal() moves focus here. */}
        <button type="button" className="enlarge-close" onClick={() => dialogRef.current?.close()}>
          Close
        </button>
      </dialog>
    </>
  );
}
