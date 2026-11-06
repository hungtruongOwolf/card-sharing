'use client';

import { useId } from 'react';

import type { CaptchaProvider } from './captcha';

interface Props {
  provider: CaptchaProvider;
  /** The provider's answer once solved, `null` while unsolved. */
  onAnswer: (answer: string | null) => void;
}

/** Same value as `FAKE_CAPTCHA_ANSWER` in captcha.ts (that module is server-side). */
const FAKE_ANSWER = 'fake-captcha-pass';

/** Development stand-in: a checkbox, clearly labelled, that the fake verifier accepts. */
function FakeCaptcha({ onAnswer }: Pick<Props, 'onAnswer'>) {
  const id = useId();
  return (
    <div className="captcha-fake">
      <input
        id={id}
        type="checkbox"
        onChange={(event) => onAnswer(event.target.checked ? FAKE_ANSWER : null)}
      />
      <label htmlFor={id}>I am not a robot (development check)</label>
    </div>
  );
}

/** Captcha widget registry, keyed like the server verifiers in captcha.ts. */
export function CaptchaField({ provider, onAnswer }: Props) {
  switch (provider) {
    case 'fake':
      return <FakeCaptcha onAnswer={onAnswer} />;
  }
}
