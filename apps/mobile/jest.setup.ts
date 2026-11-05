/* eslint-disable @typescript-eslint/no-require-imports -- jest.mock factories must use require() */
// Worklets + Reanimated in Jest: https://docs.swmansion.com/react-native-reanimated/docs/guides/testing/
jest.mock('react-native-worklets', () => require('react-native-worklets/src/mock'));
jest.mock(
  'react-native-safe-area-context',
  () => require('react-native-safe-area-context/jest/mock').default,
);

require('react-native-reanimated').setUpTests();
