/**
 * Must equal the installed `canvaskit-wasm` version; scripts/copy-canvaskit.mjs checks it.
 * The WASM is served from /canvaskit/<version>/ with an immutable cache header (next.config.ts).
 */
export const CANVASKIT_VERSION = '0.41.0';
export const CANVASKIT_BASE_PATH = `/canvaskit/${CANVASKIT_VERSION}/`;
