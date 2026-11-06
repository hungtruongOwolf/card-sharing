/**
 * @jest-environment @shopify/react-native-skia/jestEnv.js
 */
import { screen, userEvent } from '@testing-library/react-native';

import { EFFECT_SLOT_KINDS, ENTRIES, registry } from '@card-sharing/effects-registry';

import { t } from '../../../i18n';
import { renderThemed } from '../../../test/render';
import { EFFECT_PASS_MS, hasEffectRenderer, labEntries, liveCostMs } from '../../effects';
import { EffectsLabScreen, labDraft } from '../EffectsLabScreen';

jest.mock('@shopify/react-native-skia', () =>
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  require('@shopify/react-native-skia/lib/commonjs/mock').Mock(
    (globalThis as { CanvasKit?: unknown }).CanvasKit,
  ),
);

describe('effects device lab (T-124)', () => {
  it('lists every drawable effect and draws N copies of the chosen one', async () => {
    const entries = labEntries(ENTRIES, hasEffectRenderer);
    expect(entries.map((e) => e.id)).toContain('fx.fire-breath');
    expect(labDraft(registry.get('fx.sparkles')!).effects[0]?.id).toBe('fx.sparkles');
    await renderThemed(<EffectsLabScreen />);
    expect(await screen.findByTestId('effects-lab')).toBeTruthy();
    const user = userEvent.setup();
    await user.press(screen.getByRole('button', { name: t('effectsLab.count', { count: 9 }) }));
    await user.press(screen.getByRole('button', { name: t('effects.fx.laserEyes.name') }));
    expect(screen.getByRole('button', { name: t('effects.fx.laserEyes.name') })).toBeTruthy();
  });

  it('keeps every live effect inside the frame budget (estimates until devices are measured)', () => {
    const effects = ENTRIES.filter(
      (e) => EFFECT_SLOT_KINDS.includes(e.kind) && !e.tags.includes('finish'),
    );
    expect(effects.filter((e) => liveCostMs(e) > EFFECT_PASS_MS).map((e) => e.id)).toEqual([]);
  });
});
