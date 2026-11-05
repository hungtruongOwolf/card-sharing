// Component and unit tests (jest-expo, SDK 57 → Jest 29). https://docs.expo.dev/develop/unit-testing/
/** @type {import('jest').Config} */
module.exports = {
  preset: 'jest-expo',
  setupFilesAfterEnv: ['<rootDir>/jest.setup.ts'],
  // Internal packages resolve through tsconfig paths, not node_modules (see pnpm-workspace.yaml).
  moduleNameMapper: {
    '^@card-sharing/tokens$': '<rootDir>/../../packages/tokens/src/index.ts',
    '^@card-sharing/shaders$': '<rootDir>/../../packages/shaders/src/index.ts',
    '^@card-sharing/effects-registry$': '<rootDir>/../../packages/effects-registry/src/index.ts',
  },
  testPathIgnorePatterns: ['/node_modules/', '/dist/', '/.expo/'],
  maxWorkers: '50%',
};
