// Self-hosts the CanvasKit WASM for the web target (ADR-0009): copies the build React Native Skia loads
// (`canvaskit-wasm/bin/full`, see LoadSkiaWeb) into public/, which Expo serves at the site root and copies into
// the web export. No third-party CDN. index.web.ts loads it from `/canvaskit.wasm`.
// Run before `expo start --web` / `expo export -p web` (the `web` and `build:web` package scripts do).
import { copyFileSync, mkdirSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const appDir = join(dirname(fileURLToPath(import.meta.url)), '..');
const require = createRequire(join(appDir, 'package.json'));
// Resolve from Skia's own location so the copy always matches the version Skia depends on.
const skiaDir = dirname(require.resolve('@shopify/react-native-skia/package.json'));
const wasm = createRequire(join(skiaDir, 'package.json')).resolve(
  'canvaskit-wasm/bin/full/canvaskit.wasm',
);

const outDir = join(appDir, 'public');
mkdirSync(outDir, { recursive: true });
copyFileSync(wasm, join(outDir, 'canvaskit.wasm'));
console.log('copied canvaskit.wasm to apps/mobile/public/');
