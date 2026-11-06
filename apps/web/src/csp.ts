/**
 * Per-request nonce-based Content-Security-Policy (security-baseline §5): no 'unsafe-inline'
 * scripts or styles, no framing. Pure, so the policy per route is unit-tested (csp.test.ts).
 */

export type CspInput = {
  readonly nonce: string;
  readonly pathname: string;
  readonly isDev: boolean;
  /** Origin of signed media (public board images), or `null` when none is configured. */
  readonly mediaOrigin: string | null;
};

export function buildCsp({ nonce, pathname, isDev, mediaOrigin }: CspInput): string {
  // Invite pages (/i/<token>) render text and links only.
  const isInvite = pathname.startsWith('/i/');
  // Public board pages (/b/<token>) show signed images from the media origin; no WebAssembly.
  const isBoard = pathname.startsWith('/b/');
  // 'wasm-unsafe-eval' is kept on other pages for the CanvasKit spike (#179 removes it); it allows
  // WebAssembly compilation only, not JS eval.
  const wasm = isInvite || isBoard ? '' : " 'wasm-unsafe-eval'";
  let imgSrc = "img-src 'self' blob: data:";
  if (isInvite) imgSrc = "img-src 'self'";
  if (isBoard) imgSrc = mediaOrigin === null ? "img-src 'self'" : `img-src 'self' ${mediaOrigin}`;
  return [
    "default-src 'self'",
    `script-src 'self' 'nonce-${nonce}' 'strict-dynamic'${wasm}${isDev ? " 'unsafe-eval'" : ''}`,
    `style-src 'self' 'nonce-${nonce}'`,
    imgSrc,
    "font-src 'self'",
    "connect-src 'self'",
    "object-src 'none'",
    "base-uri 'self'",
    "form-action 'self'",
    "frame-ancestors 'none'",
    ...(isDev ? [] : ['upgrade-insecure-requests']),
  ].join('; ');
}
