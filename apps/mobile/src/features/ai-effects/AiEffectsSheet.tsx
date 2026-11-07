import { useRef, useState } from 'react';
import { ActivityIndicator, Pressable, ScrollView, StyleSheet, View } from 'react-native';

import { linearGradient, radius, space } from '@card-sharing/tokens';

import { t, type MessageKey } from '../../i18n';
import { Button, Sheet, Text, useTheme } from '../../ui';
import { AuthError } from '../auth';
import type { CardDraft } from '../editor/editorState';
import { newIdempotencyKey } from '../friends/idempotencyKey';
import { AiEffectsError, type AiTemplate } from './aiEffectsApi';
import { lookBasis, type AiLook } from './aiLook';
import type { AiPhase } from './runAiEffect';
import { templateArt } from './templateArt';
import type { AiEffectsController } from './useAiEffects';

export interface AiEffectsSheetProps {
  visible: boolean;
  onClose: () => void;
  controller: AiEffectsController;
  /** The card as shown now (the AI input is baked from it). */
  draft: CardDraft;
  /** A finished result; the editor applies it as the card render. */
  onApply: (look: AiLook, previewUrl: string | null) => void;
}

type Step =
  | { kind: 'pick' }
  | { kind: 'consent'; template: AiTemplate }
  | { kind: 'running'; template: AiTemplate; phase: AiPhase }
  | { kind: 'done'; template: AiTemplate; preview: boolean }
  | { kind: 'refused' | 'failed'; refunded: boolean }
  | { kind: 'timeout'; template: AiTemplate }
  | { kind: 'error'; errorKey: MessageKey; template: AiTemplate | null };

/** One explicit Apply; its id is reused only to pick the same job up again (timeout, lost answer). */
interface Attempt {
  id: string;
  templateId: string;
  basis: string;
}

const PHASE_KEYS: Record<AiPhase, MessageKey> = {
  uploading: 'ai.progress.uploading',
  checking: 'ai.progress.checking',
  generating: 'ai.progress.generating',
  finishing: 'ai.progress.finishing',
};

const ART_SIZE = 72;

/**
 * AI effects (Tier 2): curated templates with their credit cost and the balance, a one-time consent step before
 * the first upload (Apple 5.1.2(i): the photo goes to a third-party AI service, and nothing runs until the user
 * agrees and taps Apply), then progress and the result. Credits, consent and who may use AI are the server's: this
 * sheet only reflects what the server answers.
 */
export function AiEffectsSheet({
  visible,
  onClose,
  controller,
  draft,
  onApply,
}: AiEffectsSheetProps) {
  const theme = useTheme();
  const overview = controller.overview;
  const [step, setStep] = useState<Step>({ kind: 'pick' });
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const attempt = useRef<Attempt | null>(null);
  /** The policy does not allow AI effects for this account: closing hides the entry. */
  const [hideOnClose, setHideOnClose] = useState(false);

  const templates = overview?.templates ?? [];
  const selected = templates.find((template) => template.id === selectedId) ?? templates[0] ?? null;
  const credits = overview?.credits ?? 0;
  const busy = step.kind === 'running';

  const close = () => {
    // Closing while running is fine: the run carries on and applies its result when it finishes.
    if (!busy) setStep({ kind: 'pick' });
    if (hideOnClose) controller.hide();
    onClose();
  };

  const start = async (template: AiTemplate) => {
    const basis = lookBasis(draft);
    const previous = attempt.current;
    const id =
      previous && previous.templateId === template.id && previous.basis === basis
        ? previous.id
        : newIdempotencyKey();
    attempt.current = { id, templateId: template.id, basis };
    setStep({ kind: 'running', template, phase: 'uploading' });
    try {
      const outcome = await controller.run({ attemptId: id, template, basis }, draft, (phase) =>
        setStep({ kind: 'running', template, phase }),
      );
      if (outcome.kind === 'timeout') {
        setStep({ kind: 'timeout', template });
        return;
      }
      attempt.current = null;
      if (outcome.kind === 'done') {
        onApply(outcome.look, outcome.previewUrl);
        setStep({ kind: 'done', template, preview: outcome.previewUrl !== null });
      } else {
        setStep({ kind: outcome.kind, refunded: outcome.refunded });
      }
      void controller.refresh();
    } catch (error) {
      await failed(error, template);
    }
  };

  const failed = async (error: unknown, template: AiTemplate) => {
    const kind = error instanceof AiEffectsError ? error.kind : null;
    switch (kind) {
      case 'consent_required':
        // Never agreed, withdrawn elsewhere, or the text changed: ask again with the current version.
        await controller.refresh();
        setStep({ kind: 'consent', template });
        return;
      case 'policy_disallowed':
        attempt.current = null;
        setHideOnClose(true);
        setStep({ kind: 'error', errorKey: 'ai.error.notAllowed', template: null });
        return;
      case 'insufficient_credits':
        attempt.current = null;
        void controller.refresh();
        setStep({ kind: 'error', errorKey: 'ai.error.credits', template: null });
        return;
      case 'invalid':
        // A template switched off or bumped since the list loaded.
        attempt.current = null;
        void controller.refresh();
        setStep({ kind: 'error', errorKey: 'ai.error.stale', template: null });
        return;
      case 'input_rejected':
        attempt.current = null;
        setStep({ kind: 'error', errorKey: 'ai.error.inputRejected', template: null });
        return;
      case 'not_allowed':
        setStep({ kind: 'error', errorKey: 'ai.error.account', template: null });
        return;
      case 'rate_limited':
        setStep({ kind: 'error', errorKey: 'ai.error.rateLimited', template });
        return;
      default:
        if (error instanceof AuthError && error.kind !== 'unavailable') throw error;
        setStep({ kind: 'error', errorKey: 'ai.error.unavailable', template });
    }
  };

  const apply = (template: AiTemplate) => {
    if (!overview) return;
    if (!overview.consent.granted) setStep({ kind: 'consent', template });
    else void start(template);
  };

  const agree = async (template: AiTemplate) => {
    if (!overview) return;
    setStep({ kind: 'running', template, phase: 'uploading' });
    try {
      await controller.grantConsent(overview.consent.requiredVersion);
    } catch (error) {
      await failed(error, template);
      return;
    }
    void controller.refresh();
    await start(template);
  };

  const body = () => {
    switch (step.kind) {
      case 'consent':
        return (
          <View style={styles.section} testID="ai-consent">
            <Text variant="title.md" accessibilityRole="header">
              {t('ai.consent.title')}
            </Text>
            <Text>{t('ai.consent.body')}</Text>
            <Text tone="muted">{t('ai.consent.label')}</Text>
            <Button block label={t('ai.consent.agree')} onPress={() => void agree(step.template)} />
            <Button
              block
              variant="ghost"
              label={t('ai.consent.decline')}
              onPress={() => setStep({ kind: 'pick' })}
            />
          </View>
        );
      case 'running':
        return (
          <View style={styles.section} testID="ai-running" accessibilityLiveRegion="polite">
            <ActivityIndicator color={theme.color.primary.base} />
            <Text variant="label.lg">{t(PHASE_KEYS[step.phase])}</Text>
            <Text tone="muted">{t('ai.progress.hint')}</Text>
          </View>
        );
      case 'done':
        return (
          <View style={styles.section} testID="ai-done">
            <Text variant="title.md" accessibilityRole="header">
              {t('ai.done.title', { name: step.template.title })}
            </Text>
            {step.preview ? null : <Text tone="muted">{t('ai.done.noPreview')}</Text>}
            <Button block label={t('ai.done.ok')} onPress={close} />
          </View>
        );
      case 'refused':
      case 'failed':
        return (
          <View style={styles.section} testID={`ai-${step.kind}`}>
            <Text variant="title.md" accessibilityRole="header">
              {t(step.kind === 'refused' ? 'ai.refused.title' : 'ai.failed.title')}
            </Text>
            <Text tone="muted">
              {t(step.kind === 'refused' ? 'ai.refused.body' : 'ai.failed.body')}
            </Text>
            {step.refunded ? <Text>{t('ai.refunded')}</Text> : null}
            <Button block label={t('ai.tryAnother')} onPress={() => setStep({ kind: 'pick' })} />
          </View>
        );
      case 'timeout':
        return (
          <View style={styles.section} testID="ai-timeout">
            <Text variant="title.md" accessibilityRole="header">
              {t('ai.timeout.title')}
            </Text>
            <Text tone="muted">{t('ai.timeout.body')}</Text>
            <Button block label={t('ai.timeout.check')} onPress={() => void start(step.template)} />
          </View>
        );
      case 'error':
        return (
          <View style={styles.section} testID="ai-error">
            <Text accessibilityRole="alert">{t(step.errorKey)}</Text>
            {step.template ? (
              <Button block label={t('ai.retry')} onPress={() => void start(step.template!)} />
            ) : null}
            <Button
              block
              variant="ghost"
              label={t(hideOnClose ? 'common.close' : 'ai.back')}
              onPress={hideOnClose ? close : () => setStep({ kind: 'pick' })}
            />
          </View>
        );
      case 'pick':
        return (
          <View style={styles.section} testID="ai-pick">
            <Text tone="muted" testID="ai-credits">
              {t('ai.credits', { count: credits })}
            </Text>
            <ScrollView
              horizontal
              showsHorizontalScrollIndicator={false}
              contentContainerStyle={styles.tiles}
              accessibilityRole="radiogroup"
            >
              {templates.map((template) => {
                const art = templateArt(template.id);
                const isSelected = template.id === selected?.id;
                return (
                  <Pressable
                    key={template.id}
                    testID={`ai-template-${template.id}`}
                    accessibilityRole="radio"
                    accessibilityState={{ checked: isSelected }}
                    accessibilityLabel={t('ai.template.a11y', {
                      name: template.title,
                      cost: template.cost,
                    })}
                    onPress={() => setSelectedId(template.id)}
                    style={styles.tile}
                  >
                    <View
                      style={[
                        styles.art,
                        {
                          backgroundColor: theme.color.primary.base,
                          backgroundImage: linearGradient(art.gradient),
                          borderColor: isSelected ? theme.color.fg.ink : 'transparent',
                        },
                      ]}
                    >
                      <Text variant="display.md" tone="onGradient">
                        {art.glyph}
                      </Text>
                    </View>
                    <Text variant="label.md" numberOfLines={2} style={styles.tileText}>
                      {template.title}
                    </Text>
                    <Text variant="label.sm" tone="muted">
                      {t('ai.cost', { count: template.cost })}
                    </Text>
                  </Pressable>
                );
              })}
            </ScrollView>
            {selected && credits < selected.cost ? (
              <Text tone="danger">{t('ai.notEnough')}</Text>
            ) : null}
            {selected ? (
              <Button
                block
                testID="ai-apply"
                label={t('ai.apply', { count: selected.cost })}
                disabled={credits < selected.cost}
                onPress={() => apply(selected)}
              />
            ) : (
              <Text tone="muted">{t('ai.empty')}</Text>
            )}
            <Text variant="label.sm" tone="muted">
              {t('ai.disclosure')}
            </Text>
          </View>
        );
    }
  };

  return (
    <Sheet visible={visible} onClose={close} title={t('ai.title')} testID="ai-sheet">
      {body()}
    </Sheet>
  );
}

const styles = StyleSheet.create({
  section: { gap: space[3] },
  tiles: { gap: space[3], paddingVertical: space[1] },
  tile: { width: ART_SIZE + space[4], alignItems: 'center', gap: space[1] },
  art: {
    width: ART_SIZE,
    height: ART_SIZE,
    borderRadius: radius.lg,
    borderWidth: 2,
    alignItems: 'center',
    justifyContent: 'center',
  },
  tileText: { textAlign: 'center' },
});
