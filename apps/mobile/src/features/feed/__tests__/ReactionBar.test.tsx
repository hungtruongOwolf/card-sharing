import { screen } from '@testing-library/react-native';
import { Platform } from 'react-native';

import { t } from '../../../i18n';
import { renderThemed } from '../../../test/render';
import { ReactionBar } from '../ReactionBar';

const render = (chosen: string | null) =>
  renderThemed(
    <ReactionBar reactions={['react.heart', 'react.paw']} chosen={chosen} onReact={jest.fn()} />,
  );
const pill = (key: Parameters<typeof t>[0]) => screen.getByRole('button', { name: t(key) });

describe('ReactionBar selected state (B9)', () => {
  it('tells assistive tech which reaction is mine on native (accessibilityState)', async () => {
    await render('react.heart');
    expect(pill('feed.reaction.heart').props.accessibilityState).toMatchObject({ selected: true });
    expect(pill('feed.reaction.paw').props.accessibilityState).toMatchObject({ selected: false });
    expect(pill('feed.reaction.heart').props['aria-pressed']).toBeUndefined();
  });

  it('announces the toggle as aria-pressed on the web', async () => {
    const os = Platform.OS;
    Object.defineProperty(Platform, 'OS', { value: 'web', configurable: true });
    try {
      await render('react.paw');
      expect(pill('feed.reaction.paw').props['aria-pressed']).toBe(true);
      expect(pill('feed.reaction.heart').props['aria-pressed']).toBe(false);
    } finally {
      Object.defineProperty(Platform, 'OS', { value: os, configurable: true });
    }
  });
});
