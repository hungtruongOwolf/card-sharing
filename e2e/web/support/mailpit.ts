/**
 * Reads the sign-in emails the API worker sent to Mailpit (local and CI mail catcher, `API_MAIL_TRANSPORT=smtp`),
 * through Mailpit's HTTP API (v1: `GET /api/v1/search?query=...`, `GET /api/v1/message/{ID}` → `Text`).
 */
const MAILPIT_URL = process.env.MAILPIT_URL ?? 'http://127.0.0.1:8025';

/** `<API_MAGIC_LINK_URL>#rid=<request id>&token=<token>[&for=reauth]` (crates/identity/src/magic_link.rs). */
const SIGN_IN_LINK =
  /https?:\/\/\S+\/auth\/link#rid=[0-9a-f-]{36}&token=[A-Za-z0-9_-]{43}(?:&for=reauth)?/;

interface SearchResult {
  messages?: { ID?: string }[];
}

async function getJson<T>(path: string): Promise<T> {
  const response = await fetch(`${MAILPIT_URL}${path}`, {
    headers: { Accept: 'application/json' },
  });
  if (!response.ok) throw new Error(`Mailpit ${path} -> ${response.status}`);
  return (await response.json()) as T;
}

/**
 * The link (sign-in or re-auth) of the newest email to `to`, waiting for the worker to send it (it is a background job). Links in
 * `seen` are skipped, so a test can wait for a second link to the same address.
 */
export async function signInLink(
  to: string,
  seen: ReadonlySet<string> = new Set(),
  timeoutMs = 30_000,
): Promise<string> {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const search = await getJson<SearchResult>(
      `/api/v1/search?query=${encodeURIComponent(`to:"${to}"`)}&limit=5`,
    );
    for (const { ID } of search.messages ?? []) {
      if (!ID) continue;
      const message = await getJson<{ Text?: string }>(`/api/v1/message/${encodeURIComponent(ID)}`);
      const link = SIGN_IN_LINK.exec(message.Text ?? '')?.[0];
      if (link && !seen.has(link)) return link;
    }
    if (Date.now() > deadline) throw new Error(`no sign-in email to ${to} within ${timeoutMs} ms`);
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
}
