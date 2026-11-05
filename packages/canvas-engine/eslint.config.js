import { defineConfig } from 'eslint/config';

import library from '../eslint-config/library.js';

export default defineConfig([
  library,
  {
    // Pixel and point loops index typed arrays within bounds they just checked; under noUncheckedIndexedAccess
    // the alternative to `!` is a branch per pixel in the hottest code of the bake.
    rules: { '@typescript-eslint/no-non-null-assertion': 'off' },
  },
]);
