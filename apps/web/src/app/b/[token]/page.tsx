import type { Metadata } from 'next';
import { notFound } from 'next/navigation';

import { SITE_NAME } from '../../../site';

import { isShareToken } from './token';

type Props = { params: Promise<{ token: string }> };

const TITLE = 'A board was shared with you';
const DESCRIPTION = 'Open it to see the cards.';

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

/** Placeholder share preview: static HTML only, no data fetching yet. */
export default async function SharedBoardPage({ params }: Props) {
  const { token } = await params;
  if (!isShareToken(token)) {
    notFound();
  }
  return (
    <main className="card">
      <h1>{TITLE}</h1>
      <p className="muted">{DESCRIPTION}</p>
      <p className="muted">Board previews are coming soon.</p>
    </main>
  );
}
