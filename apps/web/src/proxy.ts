import { type NextRequest, NextResponse } from 'next/server';

import { buildCsp } from './csp';
import { mediaOrigin } from './features/public-board/public-board';

/**
 * Per-request nonce-based CSP (src/csp.ts). Next.js reads the nonce from the request's CSP header
 * and applies it to its own scripts, which requires dynamic rendering (see src/app/layout.tsx);
 * pages that need their own inline `<style>` read it from `x-nonce`.
 */
export function proxy(request: NextRequest) {
  const nonce = Buffer.from(crypto.randomUUID()).toString('base64');
  const csp = buildCsp({
    nonce,
    pathname: request.nextUrl.pathname,
    isDev: process.env.NODE_ENV === 'development',
    mediaOrigin: mediaOrigin(process.env),
  });

  const requestHeaders = new Headers(request.headers);
  requestHeaders.set('x-nonce', nonce);
  requestHeaders.set('Content-Security-Policy', csp);

  const response = NextResponse.next({ request: { headers: requestHeaders } });
  response.headers.set('Content-Security-Policy', csp);
  return response;
}

export const config = {
  matcher: [
    {
      source: '/((?!api|_next/static|_next/image|favicon.ico).*)',
      missing: [
        { type: 'header', key: 'next-router-prefetch' },
        { type: 'header', key: 'purpose', value: 'prefetch' },
      ],
    },
  ],
};
