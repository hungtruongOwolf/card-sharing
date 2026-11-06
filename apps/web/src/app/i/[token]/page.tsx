import type { Metadata } from 'next';
import { headers } from 'next/headers';

import { type InviteLookup, lookupInvite } from '../../../features/invite/invite';
import { readServiceApiConfig, visitorIp } from '../../../features/service-api/service-api';
import { storeLinks } from '../../../features/store-links/store-links';
import { SITE_NAME } from '../../../site';

type Props = { params: Promise<{ token: string }> };

/**
 * Metadata is the same for every token (no inviter name), so link previews and crawlers learn
 * nothing from it. Invite links are capability URLs: never indexed, no referrer
 * (next.config.ts also sends `Referrer-Policy: no-referrer` and `X-Robots-Tag` for `/i/*`).
 */
export const metadata: Metadata = {
  title: 'You have an invite',
  description: `Someone invited you to be friends on ${SITE_NAME}.`,
  robots: { index: false, follow: false, nocache: true },
  referrer: 'no-referrer',
};

async function lookup(token: string): Promise<InviteLookup> {
  const config = readServiceApiConfig(process.env);
  if (config === null) {
    return { kind: 'unavailable' };
  }
  const ip = visitorIp(await headers(), config.clientIpHeader);
  return lookupInvite(token, config, ip);
}

/**
 * `/i/<token>` (T-48): the inviter's display name and the ways into the app. Rendered on the server
 * only; the page ships no component JavaScript of its own. Every unusable token renders the same
 * neutral "expired" page.
 */
export default async function InvitePage({ params }: Props) {
  const { token } = await params;
  const result = await lookup(token);
  const stores = storeLinks(process.env);

  if (result.kind === 'found') {
    return (
      <main className="card invite">
        <p className="muted">You have an invite</p>
        <h1>{result.displayName} wants to be friends</h1>
        <p className="muted">
          Open this invite in the {SITE_NAME} app to send a friend request. You are connected once
          they accept.
        </p>
        <div className="invite-actions">
          {/* The invite URL itself is the Universal / App Link: it opens the app where installed. */}
          <a className="invite-button invite-button-primary" href={`/i/${token}`} rel="nofollow">
            Open in the app
          </a>
          {stores.map((store) => (
            <a key={store.href} className="invite-button" href={store.href} rel="noreferrer">
              {store.label}
            </a>
          ))}
        </div>
      </main>
    );
  }

  if (result.kind === 'unavailable') {
    return (
      <main className="card invite">
        <h1>We could not open this invite</h1>
        <p className="muted">Please try again in a minute.</p>
      </main>
    );
  }

  return (
    <main className="card invite">
      <h1>This invite expired</h1>
      <p className="muted">Ask your friend to send you a new link.</p>
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
