/* eslint-disable @typescript-eslint/no-require-imports -- jest.mock factories must use require() */
// Worklets + Reanimated in Jest: https://docs.swmansion.com/react-native-reanimated/docs/guides/testing/
jest.mock('react-native-worklets', () => require('react-native-worklets/src/mock'));
jest.mock(
  'react-native-safe-area-context',
  () => require('react-native-safe-area-context/jest/mock').default,
);

require('react-native-reanimated').setUpTests();

// Brand fonts are embedded natively by the expo-font config plugin; report them as registered so component
// snapshots show the faces a real build renders (src/ui/theme/fonts.ts falls back to the platform font otherwise).
jest.mock('expo-font', () => {
  const actual = jest.requireActual('expo-font');
  return {
    ...actual,
    // Resolved lazily: fonts.ts itself imports expo-font.
    getLoadedFonts: jest.fn(() => {
      const { BRAND_FACES } = jest.requireActual('./src/ui/theme/fonts');
      return Object.values(BRAND_FACES).flatMap((byWeight) => Object.values(byWeight as object));
    }),
  };
});
