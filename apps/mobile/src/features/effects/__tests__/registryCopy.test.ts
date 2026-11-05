import { ENTRIES } from '@card-sharing/effects-registry';

import { en } from '../../../i18n/messages/en';

describe('effects registry copy', () => {
  it('has an English message for every name, description and accessibility key', () => {
    const keys = ENTRIES.flatMap((entry) => [entry.name, entry.description, entry.a11yLabel]);
    expect(keys.filter((key) => !(key in en))).toEqual([]);
  });
});
