// Self-hosts the CanvasKit WASM binary (no third-party CDN): copies it from node_modules into
// public/canvaskit/<version>/ so it is served same-origin under the CSP's connect-src 'self'.
// The versioned path lets next.config.ts mark it immutable. Run before `next dev` / `next build`.
import { copyFileSync, mkdirSync, readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const appDir = join(dirname(fileURLToPath(import.meta.url)), '..');
const require = createRequire(join(appDir, 'package.json'));
const binDir = dirname(require.resolve('canvaskit-wasm'));
const installed = JSON.parse(readFileSync(join(binDir, '..', 'package.json'), 'utf8')).version;

// The client loads /canvaskit/<CANVASKIT_VERSION>/; fail the build if the two drift apart.
const constants = readFileSync(join(appDir, 'src/features/holo/canvaskit-version.ts'), 'utf8');
const expected = /CANVASKIT_VERSION = '([^']+)'/.exec(constants)?.[1];
if (expected !== installed) {
  console.error(
    `canvaskit-wasm ${installed} is installed but src/features/holo/canvaskit-version.ts says ${expected}`,
  );
  process.exit(1);
}

const outDir = join(appDir, 'public', 'canvaskit', installed);
mkdirSync(outDir, { recursive: true });
copyFileSync(join(binDir, 'canvaskit.wasm'), join(outDir, 'canvaskit.wasm'));
console.log(`copied canvaskit.wasm ${installed} to public/canvaskit/${installed}/`);
