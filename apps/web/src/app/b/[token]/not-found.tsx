import { storeLinks } from '../../../features/store-links/store-links';

/**
 * The one answer for every unusable board link (malformed, unknown, revoked, expired, board or
 * owner gone): identical for all of them, so a page never tells whether a token existed.
 */
export default function BoardNotFound() {
  const stores = storeLinks(process.env);
  return (
    <main className="card invite">
      <h1>This board is not available</h1>
      <p className="muted">The link may have expired or been turned off by its owner.</p>
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
