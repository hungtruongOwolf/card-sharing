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
  // Workspace packages export TypeScript source.
  transpilePackages: ['@card-sharing/tokens'],
  async headers() {
    return [{ source: '/:path*', headers: securityHeaders }];
  },
};

export default nextConfig;
