// https://docs.expo.dev/guides/using-eslint/
const { defineConfig, globalIgnores } = require('eslint/config');
const expoConfig = require('eslint-config-expo/flat');

const local = require('./eslint/local-plugin');

module.exports = defineConfig([
  expoConfig,
  globalIgnores(['dist/*', '.expo/*', 'coverage/*']),
  {
    // T-33: user-facing copy comes from src/i18n (ICU catalog); layouts use start/end for RTL.
    files: ['src/**/*.{ts,tsx}'],
    ignores: ['src/i18n/messages/**', '**/*.test.{ts,tsx}', '**/__tests__/**'],
    plugins: { 'card-sharing': local },
    rules: {
      'card-sharing/no-hardcoded-strings': 'error',
      'card-sharing/rtl-safe-styles': 'error',
    },
  },
]);
