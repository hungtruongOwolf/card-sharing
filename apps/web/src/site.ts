/** Public site origin used for absolute OG URLs. Server-only; not a secret. */
export function siteUrl(): URL {
  const raw = process.env.SITE_URL ?? 'http://localhost:3000';
  try {
    return new URL(raw);
  } catch {
    throw new Error(`SITE_URL is not a valid URL: ${raw}`);
  }
}

export const SITE_NAME = 'card-sharing';
