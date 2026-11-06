import type { Metadata } from 'next';
import { headers } from 'next/headers';
import { notFound } from 'next/navigation';

import { type PublicBoard, pinStylesheet } from '../../../features/public-board/public-board';
import { storeLinks } from '../../../features/store-links/store-links';
import { SITE_NAME } from '../../../site';

import { loadBoard } from './board';

type Props = { params: Promise<{ token: string }> };

const FALLBACK_TITLE = 'A shared board';

/** Never indexed, no referrer (next.config.ts also sends `X-Robots-Tag` and `Referrer-Policy`). */
const PRIVATE_LINK: Metadata = {
  robots: { index: false, follow: false, nocache: true },
  referrer: 'no-referrer',
};

function titleOf(board: PublicBoard): string {
  return board.title.trim() === '' ? FALLBACK_TITLE : board.title;
}

function descriptionOf(board: PublicBoard): string {
  return board.ownerDisplayName === null
    ? `A board shared on ${SITE_NAME}.`
    : `${board.ownerDisplayName} shared a board on ${SITE_NAME}.`;
}

export async function generateMetadata({ params }: Props): Promise<Metadata> {
  const { token } = await params;
  const result = await loadBoard(token);
  if (result.kind !== 'found') {
    return { ...PRIVATE_LINK, title: FALLBACK_TITLE };
  }
  const title = titleOf(result.board);
  const description = descriptionOf(result.board);
  // The OG image route answers 302 to a fresh signed URL of the watermarked snapshot (AC-24.7).
  const image = { url: `/b/${token}/og`, width: 1200, height: 630, alt: title };
  return {
    ...PRIVATE_LINK,
    title,
    description,
    openGraph: {
      type: 'website',
      siteName: SITE_NAME,
      title,
      description,
      url: `/b/${token}`,
      images: [image],
    },
    twitter: { card: 'summary_large_image', title, description, images: [image] },
  };
}

/**
 * `/b/<token>` (T-132, US-24): the board as guests see it, rendered on the server. Only the
 * owner's own cards show (watermarked, signed URLs from the media origin); every other card is a
 * neutral tile with no id. Every unusable token is the same 404 (`not-found.tsx`).
 */
export default async function PublicBoardPage({ params }: Props) {
  const { token } = await params;
  const result = await loadBoard(token);
  if (result.kind === 'gone') {
    notFound();
  }
  const stores = storeLinks(process.env);

  if (result.kind === 'unavailable') {
    return (
      <main className="card">
        <h1>We could not open this board</h1>
        <p className="muted">Please try again in a minute.</p>
      </main>
    );
  }

  const { board } = result;
  const nonce = (await headers()).get('x-nonce') ?? undefined;
  const cardIndexes = board.pins.flatMap((pin, index) => (pin.kind === 'card' ? [index] : []));

  return (
    <main className="board-page">
      <header className="board-header">
        <p className="muted">
          {board.ownerDisplayName === null
            ? 'A shared board'
            : `${board.ownerDisplayName} shared a board`}
        </p>
        <h1>{titleOf(board)}</h1>
      </header>
      {/* Pin placement as a nonce'd stylesheet: the CSP blocks style attributes. */}
      <style nonce={nonce}>{pinStylesheet(board.pins)}</style>
      <section className="board-stage" aria-label="Board">
        {board.pins.map((pin, index) => {
          if (pin.kind === 'card') {
            return (
              // Plain <img>: the image is a signed URL of a baked derivative; next/image would add
              // a style attribute that the CSP blocks.
              // eslint-disable-next-line @next/next/no-img-element
              <img
                key={index}
                className={`pin pin-${index} pin-card`}
                src={pin.imageUrl}
                alt={`Card ${cardIndexes.indexOf(index) + 1} of ${cardIndexes.length}`}
                width={pin.width}
                height={pin.height}
                decoding="async"
                referrerPolicy="no-referrer"
              />
            );
          }
          return (
            <div
              key={index}
              className={`pin pin-${index} pin-private`}
              role="img"
              aria-label="Private card"
            />
          );
        })}
      </section>
      <div className="invite-actions">
        {stores.map((store) => (
          <a key={store.href} className="invite-button" href={store.href} rel="noreferrer">
            {store.label}
          </a>
        ))}
      </div>
    </main>
  );
}
