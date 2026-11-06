'use client';

import { type FormEvent, useId, useRef, useState } from 'react';

import type { CaptchaProvider } from '../captcha/captcha';
import { CaptchaField } from '../captcha/CaptchaField';

import { MAX_REPORT_NOTE, REPORT_REASONS, type ReportOutcome } from './report-shared';

interface Props {
  token: string;
  captcha: CaptchaProvider;
}

type State = 'idle' | 'sending' | ReportOutcome;

const MESSAGES: Record<ReportOutcome, string> = {
  accepted: 'Thanks. Our team will review this board.',
  invalid: 'Please choose a reason and try again.',
  captcha: 'Please complete the check and try again.',
  gone: 'This board is no longer available.',
  limited: 'You have sent several reports. Please try again later.',
  unavailable: 'We could not send the report. Please try again in a minute.',
};

const OUTCOMES: ReadonlySet<string> = new Set(Object.keys(MESSAGES));

/**
 * "Report this board" for guests (T-142, AC-26.3): a modal dialog with the AC-26.1 reasons, an
 * optional note and the captcha, posted to this site's `/b/<token>/report`.
 */
export function ReportBoard({ token, captcha }: Props) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  const noteId = useId();
  const [reason, setReason] = useState<string | null>(null);
  const [note, setNote] = useState('');
  const [answer, setAnswer] = useState<string | null>(null);
  const [state, setState] = useState<State>('idle');
  // Remounts the captcha after each attempt: an answer is single-use.
  const [attempt, setAttempt] = useState(0);

  const onSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (state === 'sending') return;
    if (reason === null || answer === null) {
      setState(reason === null ? 'invalid' : 'captcha');
      return;
    }
    setState('sending');
    let outcome: ReportOutcome = 'unavailable';
    try {
      const response = await fetch(`/b/${token}/report`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ reason, note: note.trim() === '' ? null : note, captcha: answer }),
        credentials: 'omit',
      });
      const body: unknown = await response.json().catch(() => null);
      const value = (body as { outcome?: unknown } | null)?.outcome;
      if (typeof value === 'string' && OUTCOMES.has(value)) outcome = value as ReportOutcome;
    } catch {
      outcome = 'unavailable';
    }
    setState(outcome);
    setAnswer(null);
    setAttempt((n) => n + 1);
  };

  const done = state === 'accepted' || state === 'gone';

  return (
    <>
      <button
        type="button"
        className="report-open"
        aria-haspopup="dialog"
        onClick={() => dialogRef.current?.showModal()}
      >
        Report this board
      </button>
      <dialog ref={dialogRef} className="enlarge-dialog report-dialog" aria-labelledby={titleId}>
        <h2 id={titleId} className="enlarge-title">
          Report this board
        </h2>
        {done ? null : (
          <form onSubmit={onSubmit} className="report-form">
            <fieldset>
              <legend>What is wrong?</legend>
              {REPORT_REASONS.map((option) => (
                <label key={option.value} className="report-reason">
                  <input
                    type="radio"
                    name="reason"
                    value={option.value}
                    checked={reason === option.value}
                    onChange={() => setReason(option.value)}
                  />
                  {option.label}
                </label>
              ))}
            </fieldset>
            <label htmlFor={noteId}>Anything else (optional)</label>
            <textarea
              id={noteId}
              maxLength={MAX_REPORT_NOTE}
              rows={3}
              value={note}
              onChange={(event) => setNote(event.target.value)}
            />
            <CaptchaField key={attempt} provider={captcha} onAnswer={setAnswer} />
            <button type="submit" className="enlarge-close" disabled={state === 'sending'}>
              {state === 'sending' ? 'Sending…' : 'Send report'}
            </button>
          </form>
        )}
        <p className="muted" role="status">
          {state === 'idle' || state === 'sending' ? '' : MESSAGES[state]}
        </p>
        <button type="button" className="enlarge-close" onClick={() => dialogRef.current?.close()}>
          Close
        </button>
      </dialog>
    </>
  );
}
