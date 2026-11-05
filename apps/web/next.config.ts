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
      // Self-hosted CanvasKit WASM lives under a versioned path (scripts/copy-canvaskit.mjs).
      {
        source: '/canvaskit/:version/:file*',
        headers: [{ key: 'Cache-Control', value: 'public, max-age=31536000, immutable' }],
      },
    ];
  },
};

export default nextConfig;
