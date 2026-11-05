import { screen, userEvent } from '@testing-library/react-native';

import { t } from '../../../i18n';
import { renderThemed } from '../../../test/render';
import { CaptionSheet } from '../CaptionSheet';
import { captionInputCap, checkCaption, CAPTION_MAX_GRAPHEMES } from '../captionRules';

/** Built at runtime so the source holds no literal term. */
const BLOCKED = ['f', 'u', 'c', 'k'].join('');
const CAT = '\u{1F431}';

async function renderSheet(props: Partial<Parameters<typeof CaptionSheet>[0]> = {}) {
  const onSave = jest.fn();
  const onClose = jest.fn();
  await renderThemed(
    <CaptionSheet visible caption="" max={10} onSave={onSave} onClose={onClose} {...props} />,
  );
  return { onSave, onClose, user: userEvent.setup(), input: screen.getByTestId('caption-input') };
}

describe('caption rules', () => {
  it('counts graphemes against the 80 default and caps raw input generously', () => {
    expect(CAPTION_MAX_GRAPHEMES).toBe(80);
    expect(checkCaption(CAT.repeat(80))).toMatchObject({ length: 80, issue: null });
    expect(checkCaption(CAT.repeat(81)).issue).toBe('tooLong');
    expect(checkCaption('').issue).toBeNull();
    expect(captionInputCap(80)).toBeGreaterThanOrEqual(80 * 11);
  });
});

describe('CaptionSheet', () => {
  it('counts emoji as one character each and saves the sanitized caption', async () => {
    const { onSave, onClose, user, input } = await renderSheet();
    await user.type(input, `hi ${CAT}${CAT}\u2067`);
    // The counter is the field's hint, which screen readers hear with the field.
    expect(screen.getByTestId('caption-input').props.accessibilityHint).toBe(
      t('caption.counter', { count: 5, max: 10 }),
    );
    await user.press(screen.getByRole('button', { name: t('caption.save') }));
    expect(onSave).toHaveBeenCalledWith(`hi ${CAT}${CAT}`);
    expect(onClose).toHaveBeenCalled();
  });

  it('blocks saving a caption over the limit', async () => {
    const { onSave, user, input } = await renderSheet();
    await user.type(input, CAT.repeat(12));
    expect(screen.getByText(t('caption.tooLong', { over: 2 }))).toBeTruthy();
    expect(screen.getByRole('button', { name: t('caption.save') })).toBeDisabled();
    expect(onSave).not.toHaveBeenCalled();
  });

  it('hints that the shared filter would refuse the caption, without naming the word', async () => {
    const { user, input } = await renderSheet({ max: 80 });
    await user.type(input, `oh ${BLOCKED.toUpperCase()}`);
    expect(screen.getByText(t('caption.notAllowed'))).toBeTruthy();
    expect(t('caption.notAllowed')).not.toMatch(new RegExp(BLOCKED, 'i'));
    expect(screen.getByRole('button', { name: t('caption.save') })).toBeDisabled();
  });

  it('removes an existing caption', async () => {
    const { onSave, user } = await renderSheet({ caption: 'zoomies' });
    expect(screen.getByTestId('caption-input').props.value).toBe('zoomies');
    await user.press(screen.getByRole('button', { name: t('caption.remove') }));
    expect(onSave).toHaveBeenCalledWith('');
  });
});
