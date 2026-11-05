import { ENTRIES } from '@card-sharing/effects-registry';

import { en } from '../../../i18n/messages/en';

describe('effects registry copy', () => {
  it('has an English message for every name, description, accessibility and preset key', () => {
    const keys = ENTRIES.flatMap((entry) => [
      entry.name,
      entry.description,
      entry.a11yLabel,
      ...(entry.presets ?? []).map((preset) => preset.name),
    ]);
    expect(keys.filter((key) => !(key in en))).toEqual([]);
  });
});
