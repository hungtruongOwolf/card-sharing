/**
 * English message catalog (source locale), ICU MessageFormat syntax — see ../format.ts.
 *
 * Rules:
 *  - Every user-facing string, including accessibility labels and hints, lives here. The ESLint rule
 *    `card-sharing/no-hardcoded-strings` rejects literals in JSX.
 *  - Never concatenate messages; pass values (`{name}`) and let plural/select pick the wording.
 *  - Keys are `area.element.purpose`; keep them stable, translations key off them.
 *  - Use the typographic apostrophe ’ in copy (a plain ' is ICU syntax when it precedes { } #).
 */
export const en = {
  'app.name': 'card-sharing',

  'common.close': 'Close',
  'common.dismiss': 'Dismiss',
  'common.undo': 'Undo',

  'sheet.backdrop.label': 'Close sheet',

  'toast.dismiss.hint': 'Dismisses the message',
  'toast.a11y.info': '{message}',
  'toast.a11y.success': 'Success: {message}',
  'toast.a11y.error': 'Error: {message}',

  'textField.error.a11y': 'Error: {message}',

  'home.hint': 'Drag the card to tilt it',
  'home.editCaption': 'Edit caption',
  'home.caption.label': 'Caption',
  'home.caption.placeholder': 'Say something about it',
  'home.caption.hint': 'Up to {max, number} characters',
  'home.caption.tooLong':
    '{over, plural, one {# character too long} other {# characters too long}}',
  'home.caption.save': 'Save caption',
  'home.caption.saved': 'Caption saved',
  'home.send': 'Send to friends',
  'home.sent': 'Sent to {count, plural, =0 {nobody yet} one {# friend} other {# friends}}',

  'holo.label': 'Holographic sample card',
  'holo.unavailable': 'Card effect unavailable',
} as const;

export type MessageKey = keyof typeof en;
export type Catalog = Record<MessageKey, string>;
