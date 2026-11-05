/**
 * @jest-environment @shopify/react-native-skia/jestEnv.js
 */
import { screen, userEvent } from '@testing-library/react-native';

import { t } from '../../../i18n';
import { renderThemed } from '../../../test/render';
import { CutLabScreen } from '../CutLabScreen';

jest.mock('@shopify/react-native-skia', () =>
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  require('@shopify/react-native-skia/lib/commonjs/mock').Mock(
    (globalThis as { CanvasKit?: unknown }).CanvasKit,
  ),
);

// The screen bakes a 256² lab contour and renders with CanvasKit WASM: slow under a parallel full run.
const RENDER_TIMEOUT_MS = 20_000;

describe('CutLabScreen', () => {
  it(
    'lists the registry cut styles and applies the thick sticker preset',
    async () => {
      await renderThemed(<CutLabScreen />);
      const user = userEvent.setup();
      expect(await screen.findByTestId('cut-lab')).toBeTruthy();
      for (const key of ['effects.cut.none.name', 'effects.cut.raw.name'] as const) {
        expect(screen.getByRole('button', { name: t(key) })).toBeTruthy();
      }
      await user.press(screen.getByRole('button', { name: t('effects.cut.diecut.name') }));
      expect(
        screen.getByText(t('cutLab.param', { name: 'borderWidthPct', value: 3 })),
      ).toBeTruthy();

      await user.press(screen.getByRole('button', { name: t('effects.cut.diecut.preset.thick') }));
      expect(
        screen.getByText(t('cutLab.param', { name: 'borderWidthPct', value: 6 })),
      ).toBeTruthy();
      const increase = screen.getByRole('button', {
        name: t('cutLab.increase', { name: 'borderWidthPct' }),
      });
      expect(increase).toBeDisabled();

      await user.press(
        screen.getByRole('button', { name: t('cutLab.decrease', { name: 'borderWidthPct' }) }),
      );
      expect(
        screen.getByText(t('cutLab.param', { name: 'borderWidthPct', value: 5.725 })),
      ).toBeTruthy();
    },
    RENDER_TIMEOUT_MS,
  );
});
