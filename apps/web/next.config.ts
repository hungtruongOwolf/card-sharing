import type { NextConfig } from 'next';

// Static security headers for every response. The CSP is per-request (nonce-based) and is
// set in src/proxy.ts — a static CSP would need 'unsafe-inline' for Next's inline scripts.
const securityHeaders = [
  { key: 'Strict-Transport-Security', value: 'max-age=63072000; includeSubDomains' },
  { key: 'X-Content-Type-Options', value: 'nosniff' },
  { key: 'X-Frame-Options', value: 'DENY' },
  { key: 'Referrer-Policy', value: 'strict-origin-when-cross-origin' },
  { key: 'Permissions-Policy', value: 'camera=(), microphone=(), geolocation=(), payment=()' },
  { key: 'Cross-Origin-Opener-Policy', value: 'same-origin' },
];

/** Request headers that make Next.js answer with an RSC payload instead of HTML. */
const RSC_REQUEST_HEADERS = ['rsc', 'next-router-prefetch', 'next-router-state-tree'] as const;

const nextConfig: NextConfig = {
  reactStrictMode: true,
  poweredByHeader: false,
  // Never ship source maps to browsers (security-baseline §4); upload privately to error tracking later.
  productionBrowserSourceMaps: false,
  turbopack: {
    // canvaskit-wasm's loader has a Node-only branch that requires fs/path; never taken in browsers.
    resolveAlias: {
      fs: { browser: './src/empty-module.ts' },
      path: { browser: './src/empty-module.ts' },
    },
  },
  async headers() {
    return [
      { source: '/:path*', headers: securityHeaders },
      // Invite links are capability URLs (SR-17): no referrer, never indexed or cached. Later
      // entries override the same keys above.
      {
        source: '/i/:token*',
        headers: [
          { key: 'Referrer-Policy', value: 'no-referrer' },
          { key: 'X-Robots-Tag', value: 'noindex, nofollow, noarchive' },
          { key: 'Cache-Control', value: 'private, no-store' },
        ],
      },
      // Public board links are capability URLs too (SR-17, #238): no referrer, never indexed. The OG
      // route (/b/<token>/og) sets its own Cache-Control and Cache-Tag.
      {
        source: '/b/:token*',
        headers: [
          { key: 'Referrer-Policy', value: 'no-referrer' },
          { key: 'X-Robots-Tag', value: 'noindex, nofollow, noarchive' },
        ],
      },
      // The board HTML may sit in a shared cache for 60 s (T-132; less than the 10-minute image
      // URLs, and AC-24.4 "404 within 60 s" holds even without a purge). Browsers always revalidate.
      {
        source: '/b/:token',
        missing: RSC_REQUEST_HEADERS.map((key) => ({ type: 'header' as const, key })),
        headers: [{ key: 'Cache-Control', value: 'public, max-age=0, s-maxage=60' }],
      },
      // RSC payloads of the same URL are never cached: a CDN that ignores Vary must not serve one
      // in place of the HTML.
      ...RSC_REQUEST_HEADERS.map((key) => ({
        source: '/b/:token',
        has: [{ type: 'header' as const, key }],
        headers: [{ key: 'Cache-Control', value: 'private, no-store' }],
      })),
      // Self-hosted CanvasKit WASM lives under a versioned path (scripts/copy-canvaskit.mjs).
      {
        source: '/canvaskit/:version/:file*',
        headers: [{ key: 'Cache-Control', value: 'public, max-age=31536000, immutable' }],
      },
    ];
  },
};

export default nextConfig;
