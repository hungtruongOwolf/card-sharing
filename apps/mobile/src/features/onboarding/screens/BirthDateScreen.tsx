import { useMemo, useRef, useState } from 'react';
import { StyleSheet, View, type TextInputInstance } from 'react-native';

import { space } from '@card-sharing/tokens';

import { t, type MessageKey } from '../../../i18n';
import { Button, Screen, Text, TextField } from '../../../ui';
import {
  datePartOrder,
  EMPTY_BIRTH_DATE,
  isBirthDateComplete,
  toIsoBirthDate,
  type BirthDateInput,
  type DatePart,
} from '../birthDate';
import { useOnboarding } from '../OnboardingProvider';
import { genericErrorMessage } from './errorMessage';
import { FormMessage } from './FormMessage';

/*
 * AC-2.4 neutral DOB screen review checklist (prd-mvp US-2, threat-model SR-24/SR-25). Re-check on every change
 * to this screen; the automated parts live in __tests__/BirthDateScreen.test.tsx.
 *  1. No pre-filled date: every field starts empty; placeholders are format hints (DD/MM/YYYY), never a date.
 *  2. No threshold hint: no copy, accessibility label or hint mentions an age, a minimum or "you must be";
 *     the screen never reads `policy.min_age` (the onboarding API does not even expose it).
 *  3. Age-blind client: no age is computed; any real date from 1900 to today is accepted and sent; the only
 *     error ("Enter a real date.") is identical for every invalid input.
 *  4. No picker anchored at a threshold year (free text fields, no wheel defaulting to a year).
 *  5. Same path for every answer: no extra confirmation, warning or different button for young dates.
 *  6. Refusal is neutral: no reason, no age, no countdown or "try again later" (RefusedScreen).
 *  7. DOB cannot be edited after submission (AC-2.3): the route is guarded by the server step, and
 *     `AGE_ALREADY_SUBMITTED` moves on without offering an edit.
 *  8. Accessibility: each field labelled, number pad, 44 pt targets, wraps at 200 % font size.
 */

const MAX_LENGTH: Record<DatePart, number> = { day: 2, month: 2, year: 4 };
const LABEL: Record<DatePart, MessageKey> = {
  day: 'onboarding.birthDate.day',
  month: 'onboarding.birthDate.month',
  year: 'onboarding.birthDate.year',
};
const PLACEHOLDER: Record<DatePart, MessageKey> = {
  day: 'onboarding.birthDate.dayPlaceholder',
  month: 'onboarding.birthDate.monthPlaceholder',
  year: 'onboarding.birthDate.yearPlaceholder',
};

export interface BirthDateScreenProps {
  /** Injected for tests; defaults to the device clock. */
  today?: () => Date;
  /** Field order locale; defaults to the device locale. */
  locale?: string;
}

export function BirthDateScreen({ today = () => new Date(), locale }: BirthDateScreenProps) {
  const { submitBirthDate } = useOnboarding();
  const order = useMemo(() => datePartOrder(locale), [locale]);
  const [value, setValue] = useState<BirthDateInput>(EMPTY_BIRTH_DATE);
  const [message, setMessage] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const refs = useRef<Partial<Record<DatePart, TextInputInstance | null>>>({});

  function change(part: DatePart, text: string) {
    const digits = text.replace(/\D/g, '').slice(0, MAX_LENGTH[part]);
    setValue((prev) => ({ ...prev, [part]: digits }));
    setMessage(null);
    const next = order[order.indexOf(part) + 1];
    if (digits.length === MAX_LENGTH[part] && next) refs.current[next]?.focus();
  }

  async function submit() {
    const iso = toIsoBirthDate(value, today());
    if (!iso) {
      setMessage(t('onboarding.birthDate.invalid'));
      return;
    }
    setSubmitting(true);
    try {
      await submitBirthDate(iso);
    } catch (err) {
      setMessage(genericErrorMessage(err));
      setSubmitting(false);
    }
  }

  return (
    <Screen title={t('onboarding.birthDate.title')} testID="onboarding-birth-date">
      <Text tone="muted">{t('onboarding.birthDate.body')}</Text>
      <View style={styles.fields}>
        {order.map((part, index) => (
          <View key={part} style={part === 'year' ? styles.year : styles.short}>
            <TextField
              ref={(input) => {
                refs.current[part] = input;
              }}
              testID={`birth-date-${part}`}
              label={t(LABEL[part])}
              placeholder={t(PLACEHOLDER[part])}
              value={value[part]}
              onChangeText={(text) => change(part, text)}
              keyboardType="number-pad"
              inputMode="numeric"
              maxLength={MAX_LENGTH[part]}
              returnKeyType={index === order.length - 1 ? 'done' : 'next'}
              onSubmitEditing={() => {
                const next = order[index + 1];
                if (next) refs.current[next]?.focus();
                else void submit();
              }}
              editable={!submitting}
            />
          </View>
        ))}
      </View>
      <FormMessage message={message} />
      <Button
        block
        label={t('onboarding.birthDate.submit')}
        disabled={!isBirthDateComplete(value)}
        loading={submitting}
        onPress={() => void submit()}
      />
    </Screen>
  );
}

const styles = StyleSheet.create({
  // Wraps onto several lines at large font sizes instead of squeezing the fields.
  fields: { flexDirection: 'row', flexWrap: 'wrap', gap: space[3] },
  short: { flexGrow: 1, flexBasis: 72 },
  year: { flexGrow: 2, flexBasis: 104 },
});
