'use client';

import { useId } from 'react';

import { type CaptchaProvider, FAKE_CAPTCHA_TOKEN } from './captcha';

interface Props {
  provider: CaptchaProvider;
  /** The provider's token once solved, `null` while unsolved. */
  onAnswer: (answer: string | null) => void;
}

/** Development stand-in: a checkbox, clearly labelled, whose token the API's fake verifier accepts. */
function FakeCaptcha({ onAnswer }: Pick<Props, 'onAnswer'>) {
  const id = useId();
  return (
    <div className="captcha-fake">
      <input
        id={id}
        type="checkbox"
        onChange={(event) => onAnswer(event.target.checked ? FAKE_CAPTCHA_TOKEN : null)}
      />
      <label htmlFor={id}>I am not a robot (development check)</label>
    </div>
  );
}

/** Captcha widget registry, keyed by `CaptchaProvider`; the API verifies the token. */
export function CaptchaField({ provider, onAnswer }: Props) {
  switch (provider) {
    case 'fake':
      return <FakeCaptcha onAnswer={onAnswer} />;
  }
}
