/* eslint-disable @typescript-eslint/no-require-imports -- the app is required only after CanvasKit has loaded */
// Web entry (ADR-0009). Native platforms use index.ts.
// `@expo/metro-runtime` must be the first import (Fast Refresh on web), as in expo-router/entry-classic.
import '@expo/metro-runtime';
import { LoadSkiaWeb } from '@shopify/react-native-skia/lib/module/web';

import { t } from './src/i18n';
import { mountPhoneFrame, shouldFrame, topLevel } from './src/platform/web/phoneFrame';

if (shouldFrame(topLevel(window))) {
  // Desktop: draw the phone frame; the app runs inside it (src/platform/web/phoneFrame.ts).
  mountPhoneFrame(document, window, window.location.href, t('app.name'));
} else {
  // React Native Skia on web needs CanvasKit before any Skia module is evaluated, so the app (whose routes
  // import Skia) is required only once it has loaded. scripts/copy-canvaskit.mjs puts the WASM in public/.
  void LoadSkiaWeb({ locateFile: (file: string) => `/${file}` }).then(() => {
    const { App } = require('expo-router/build/qualified-entry');
    const { renderRootComponent } = require('expo-router/build/renderRootComponent');
    renderRootComponent(App);
  });
}
