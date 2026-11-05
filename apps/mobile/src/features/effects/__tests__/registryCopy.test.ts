import { ENTRIES } from '@card-sharing/effects-registry';

import { en } from '../../../i18n/messages/en';
import { paramLabelKey, paramValueKey } from '../../editor/ParamControls';

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

  it('has a label for every editable param and every enum value (editor controls)', () => {
    const keys = ENTRIES.flatMap((entry) =>
      Object.entries(entry.params)
        .filter(([, spec]) => spec.ui !== 'hidden')
        .flatMap(([name, spec]) => [
          paramLabelKey(name),
          ...(spec.type === 'enum' ? spec.values.map((value) => paramValueKey(name, value)) : []),
        ]),
    );
    expect(keys.filter((key) => !(key in en))).toEqual([]);
  });
});
