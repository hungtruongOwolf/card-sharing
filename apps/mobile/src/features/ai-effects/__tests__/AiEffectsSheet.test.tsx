import { screen, userEvent, waitFor } from '@testing-library/react-native';

import { registry } from '@card-sharing/effects-registry';

import { t } from '../../../i18n';
import { renderThemed } from '../../../test/render';
import { styleChoice, type CardDraft } from '../../editor/editorState';
import { AiEffectsError, type AiOverview } from '../aiEffectsApi';
import { AiEffectsSheet } from '../AiEffectsSheet';
import type { AiRunOutcome } from '../runAiEffect';
import type { AiEffectsController } from '../useAiEffects';

const draft: CardDraft = {
  cut: styleChoice(registry.get('cut.diecut')!),
  finish: null,
  effects: [],
  wholePhoto: false,
  caption: '',
};

const overview = (over: Partial<AiOverview> = {}): AiOverview => ({
  available: true,
  consent: { requiredVersion: 2, granted: true },
  credits: 5,
  visibleBadge: true,
  templates: [
    { id: 'ai.watercolor', version: 1, title: 'Watercolor', cost: 1 },
    { id: 'ai.vintage_photo', version: 1, title: 'Vintage photo', cost: 2 },
  ],
  ...over,
});

const done: AiRunOutcome = {
  kind: 'done',
  previewUrl: null,
  look: {
    jobId: '11111111-1111-4111-8111-111111111111',
    templateId: 'ai.watercolor',
    templateVersion: 1,
    title: 'Watercolor',
    outputMediaId: '33333333-3333-4333-8333-333333333333',
    basis: 'abcdef01',
  },
};

function controller(over: Partial<AiEffectsController> = {}): jest.Mocked<AiEffectsController> {
  return {
    overview: overview(),
    refresh: jest.fn().mockResolvedValue(undefined),
    grantConsent: jest.fn().mockResolvedValue(undefined),
    run: jest.fn().mockResolvedValue(done),
    hide: jest.fn(),
    ...over,
  } as jest.Mocked<AiEffectsController>;
}

async function renderSheet(ctl: AiEffectsController, onApply = jest.fn(), onClose = jest.fn()) {
  await renderThemed(
    <AiEffectsSheet visible onClose={onClose} controller={ctl} draft={draft} onApply={onApply} />,
  );
  return { onApply, onClose };
}

const apply = (cost: number) =>
  screen.getByRole('button', { name: t('ai.apply', { count: cost }) });

describe('AiEffectsSheet', () => {
  it('shows the balance and templates, and applies the chosen one only on tap', async () => {
    const ctl = controller();
    const { onApply } = await renderSheet(ctl);
    const user = userEvent.setup();
    expect(screen.getByText(t('ai.credits', { count: 5 }))).toBeTruthy();
    expect(screen.getByText('Watercolor')).toBeTruthy();
    expect(ctl.run).not.toHaveBeenCalled();

    await user.press(apply(1));
    await screen.findByTestId('ai-done');
    expect(ctl.run).toHaveBeenCalledWith(
      expect.objectContaining({ template: expect.objectContaining({ id: 'ai.watercolor' }) }),
      draft,
      expect.any(Function),
    );
    expect(onApply).toHaveBeenCalledWith(done.kind === 'done' && done.look, null);
    expect(screen.getByText(t('ai.done.noPreview'))).toBeTruthy();
  });

  it('asks for consent before the first upload and sends the version it showed', async () => {
    const ctl = controller({
      overview: overview({ consent: { requiredVersion: 2, granted: false } }),
    });
    await renderSheet(ctl);
    const user = userEvent.setup();
    await user.press(apply(1));
    expect(screen.getByTestId('ai-consent')).toBeTruthy();
    expect(screen.getByText(t('ai.consent.body'))).toBeTruthy();
    expect(ctl.run).not.toHaveBeenCalled();

    await user.press(screen.getByRole('button', { name: t('ai.consent.decline') }));
    expect(ctl.grantConsent).not.toHaveBeenCalled();
    expect(ctl.run).not.toHaveBeenCalled();

    await user.press(apply(1));
    await user.press(screen.getByRole('button', { name: t('ai.consent.agree') }));
    await screen.findByTestId('ai-done');
    expect(ctl.grantConsent).toHaveBeenCalledWith(2);
    expect(ctl.run).toHaveBeenCalledTimes(1);
  });

  it('disables a template the balance does not cover', async () => {
    await renderSheet(controller({ overview: overview({ credits: 1 }) }));
    const user = userEvent.setup();
    await user.press(screen.getByTestId('ai-template-ai.vintage_photo'));
    expect(apply(2).props.accessibilityState?.disabled).toBe(true);
    expect(screen.getByText(t('ai.notEnough'))).toBeTruthy();
  });

  it('goes back to consent when the server says CONSENT_REQUIRED', async () => {
    const ctl = controller({
      run: jest.fn().mockRejectedValue(new AiEffectsError('consent_required', 403)),
    });
    await renderSheet(ctl);
    await userEvent.setup().press(apply(1));
    await screen.findByTestId('ai-consent');
    expect(ctl.refresh).toHaveBeenCalled();
  });

  it('says when credits ran out (402) and refreshes the balance', async () => {
    const ctl = controller({
      run: jest.fn().mockRejectedValue(new AiEffectsError('insufficient_credits', 402)),
    });
    await renderSheet(ctl);
    await userEvent.setup().press(apply(1));
    expect(await screen.findByText(t('ai.error.credits'))).toBeTruthy();
    expect(ctl.refresh).toHaveBeenCalled();
  });

  it('hides AI effects when the policy does not allow them', async () => {
    const ctl = controller({
      run: jest.fn().mockRejectedValue(new AiEffectsError('policy_disallowed', 403)),
    });
    const { onClose } = await renderSheet(ctl);
    const user = userEvent.setup();
    await user.press(apply(1));
    expect(await screen.findByText(t('ai.error.notAllowed'))).toBeTruthy();
    const closeButtons = screen.getAllByRole('button', { name: t('common.close') });
    await user.press(closeButtons.at(-1)!);
    expect(ctl.hide).toHaveBeenCalled();
    expect(onClose).toHaveBeenCalled();
  });

  it('tells the user a refused run was refunded', async () => {
    const ctl = controller({
      run: jest.fn().mockResolvedValue({ kind: 'refused', refunded: true, outcome: 'x' }),
    });
    await renderSheet(ctl);
    await userEvent.setup().press(apply(1));
    await screen.findByTestId('ai-refused');
    expect(screen.getByText(t('ai.refunded'))).toBeTruthy();
  });

  it('resumes the same job after a timeout (same attempt id, not charged twice)', async () => {
    const run = jest.fn().mockResolvedValueOnce({ kind: 'timeout' }).mockResolvedValueOnce(done);
    await renderSheet(controller({ run }));
    const user = userEvent.setup();
    await user.press(apply(1));
    await screen.findByTestId('ai-timeout');
    await user.press(screen.getByRole('button', { name: t('ai.timeout.check') }));
    await waitFor(() => expect(run).toHaveBeenCalledTimes(2));
    expect(run.mock.calls[1][0].attemptId).toBe(run.mock.calls[0][0].attemptId);
  });
});
