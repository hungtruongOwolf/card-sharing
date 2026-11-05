import type { Metadata } from 'next';
import { notFound } from 'next/navigation';

import { radius } from '@card-sharing/tokens';

import { HoloEnlarge } from '../../../features/holo/HoloEnlarge';
import { SITE_NAME } from '../../../site';

import { isShareToken } from './token';

type Props = { params: Promise<{ token: string }> };

const TITLE = 'A board was shared with you';
const DESCRIPTION = 'Open it to see the cards.';

/**
 * Placeholder baked card (T-126 spike). Real cards come from the API as a signed, same-origin or
 * CDN URL of the server-baked still (effects-catalog `webPreview: 'baked'`).
 */
const CARD = {
  src: '/cards/sample-card.webp',
  width: 600,
  height: 840,
  alt: 'A cat sticker card with a holographic finish',
} as const;

export async function generateMetadata({ params }: Props): Promise<Metadata> {
  const { token } = await params;
  if (!isShareToken(token)) {
    return {};
  }
  return {
    title: TITLE,
    description: DESCRIPTION,
    // Share links are capability URLs: never index them.
    robots: { index: false, follow: false },
    openGraph: {
      type: 'website',
      siteName: SITE_NAME,
      title: TITLE,
      description: DESCRIPTION,
      url: `/b/${token}`,
    },
    twitter: { card: 'summary', title: TITLE, description: DESCRIPTION },
  };
}

/**
 * Share preview: the baked card is server-rendered (first paint and LCP need no JS);
 * the live holo view loads CanvasKit only when the card is tapped.
 */
export default async function SharedBoardPage({ params }: Props) {
  const { token } = await params;
  if (!isShareToken(token)) {
    notFound();
  }
  return (
    <main className="card">
      <h1>{TITLE}</h1>
      <p className="muted">{DESCRIPTION}</p>
      <HoloEnlarge
        imageUrl={CARD.src}
        imageWidth={CARD.width}
        imageHeight={CARD.height}
        alt={CARD.alt}
        cornerRadius={radius.md}
      >
        {/* Plain <img>: the still is already baked and sized, and next/image adds an inline style
            attribute that the nonce-based CSP blocks. */}
        {/* eslint-disable-next-line @next/next/no-img-element */}
        <img
          src={CARD.src}
          alt={CARD.alt}
          width={CARD.width}
          height={CARD.height}
          fetchPriority="high"
          decoding="async"
          className="card-image"
        />
      </HoloEnlarge>
    </main>
  );
}
