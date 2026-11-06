// Jest environment for tests that render with real Skia (CanvasKit WASM): `@jest-environment <rootDir>/jest.skia-environment.js`.
//
// Same as `@shopify/react-native-skia/jestEnv.js` (a Node environment with `global.CanvasKit`), plus two Jest 30 fixes
// that jest-expo applies to its own environment (jest-expo/src/preset/nativeEnvironment.js):
// - `jest-environment-node` is resolved from jest-expo, so it is the Jest 30 copy and not the Jest 29 one that
//   `@react-native/jest-preset` still pulls in (the hoisted layout decides which one a bare import gets).
// - Since Jest 30.4, teardown reads every enumerable global to clear mocks. Expo installs globals lazily (`fetch`,
//   `structuredClone`, `URL`, ...) through getters that `require()` on first access, which throws once the test file has
//   finished. Accessor properties are skipped so those getters never run during teardown.
const path = require('node:path');

const { TestEnvironment } = require(
  require.resolve('jest-environment-node', {
    paths: [path.dirname(require.resolve('jest-expo/package.json'))],
  }),
);
const CanvasKitInit = require('canvaskit-wasm/bin/full/canvaskit');

/** @this {import('jest-mock').ModuleMocker} */
function clearMocksOnScope(scope) {
  for (const key of Object.keys(scope)) {
    const descriptor = Object.getOwnPropertyDescriptor(scope, key);
    if (!descriptor || typeof descriptor.get === 'function') continue;
    const value = descriptor.value;
    if (
      value != null &&
      (typeof value === 'object' || typeof value === 'function') &&
      '_isMockFunction' in value &&
      this.isMockFunction(value) &&
      typeof value.mockClear === 'function'
    ) {
      value.mockClear();
    }
  }
}

class SkiaEnvironment extends TestEnvironment {
  constructor(config, context) {
    super(config, context);
    this.moduleMocker.clearMocksOnScope = clearMocksOnScope;
  }

  async setup() {
    await super.setup();
    this.global.CanvasKit = await CanvasKitInit({});
  }
}

module.exports = SkiaEnvironment;
module.exports.TestEnvironment = SkiaEnvironment;
