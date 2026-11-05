import { useState } from 'react';

import { t } from '../../i18n';
import { Button, Sheet, TextField } from '../../ui';
import { captionInputCap, checkCaption } from './captionRules';

export interface CaptionSheetProps {
  visible: boolean;
  /** The saved caption (already sanitized). */
  caption: string;
  max: number;
  onSave: (caption: string) => void;
  onClose: () => void;
}

/**
 * Caption editor sheet (T-69): grapheme counter, the shared filter's hint, Save disabled while the caption
 * breaks a rule. Saves the sanitized text (bidi controls and invisible characters stripped before send).
 */
export function CaptionSheet({ visible, caption, max, onSave, onClose }: CaptionSheetProps) {
  const [draft, setDraft] = useState(caption);
  // Each opening starts from the saved caption (state adjusted during render, not in an effect).
  const [wasVisible, setWasVisible] = useState(visible);
  if (visible !== wasVisible) {
    setWasVisible(visible);
    if (visible) setDraft(caption);
  }

  const check = checkCaption(draft, max);
  const error =
    check.issue === 'tooLong'
      ? t('caption.tooLong', { over: check.length - max })
      : check.issue === 'notAllowed'
        ? t('caption.notAllowed')
        : undefined;

  return (
    <Sheet visible={visible} onClose={onClose} title={t('caption.title')} testID="caption-sheet">
      <TextField
        label={t('caption.label')}
        placeholder={t('caption.placeholder')}
        value={draft}
        onChangeText={setDraft}
        maxLength={captionInputCap(max)}
        hint={t('caption.counter', { count: check.length, max })}
        error={error}
        autoFocus
        returnKeyType="done"
        submitBehavior="blurAndSubmit"
        testID="caption-input"
      />
      <Button
        block
        label={t('caption.save')}
        disabled={check.issue !== null}
        onPress={() => {
          onSave(check.text);
          onClose();
        }}
      />
      {caption ? (
        <Button
          block
          variant="ghost"
          label={t('caption.remove')}
          onPress={() => {
            onSave('');
            onClose();
          }}
        />
      ) : null}
    </Sheet>
  );
}
