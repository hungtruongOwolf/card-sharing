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
  'holo.lab.open': 'Open holo lab',
  'holo.lab.title': 'Holo lab',
  'holo.lab.hint': 'Tilt the phone to move the foil. Press a card to squish it.',
  'holo.lab.cardLabel': 'Holographic test card',
  'holo.lab.summary':
    '{live, number} live, {baked, number} baked · shader fill {fill, number} megapixels per frame',
  'holo.lab.reducedMotion': 'Reduced motion is on: static sheen, no tilt, no springs.',
  'holo.lab.noGyro': 'No gravity sensor on this device, so tilt stays at rest.',
  'holo.lab.bakeMs': 'Last bake took {ms, number} ms',
  'holo.lab.knob.cards': 'Cards',
  'holo.lab.knob.maxLive': 'Live cards at most (maxLive)',
  'holo.lab.knob.scale': 'Live resolution scale',
  'holo.lab.knob.columns': 'Columns',
  'holo.lab.option': '{value, number}',
  'holo.lab.toggle': '{name}: {state, select, on {on} other {off}}',
  'holo.lab.toggle.ambient': 'Ambient animation',
  'holo.lab.toggle.reducedMotion': 'Force reduced motion',
  'holo.lab.toggle.skiaDebug': 'Skia debug',
  'holo.lab.toggle.capture': 'Frame-time capture',

  'perf.overlay.title': 'Frame time',
  'perf.overlay.waiting': 'Collecting the first second…',
  'perf.overlay.live':
    'Last second: {fps, number} fps · p95 {p95, number} ms · dropped {dropped, number}%',
  'perf.overlay.session':
    'Session {seconds, number} s, {frames, number} frames: p50 {p50, number} ms · p95 {p95, number} ms · p99 {p99, number} ms · max {max, number} ms · dropped {dropped, number}%',
  'perf.overlay.reset': 'Reset',

  'onboarding.loading': 'Loading',
  'onboarding.error.title': 'Something went wrong',
  'onboarding.error.body': 'We couldn’t reach the server. Check your connection and try again.',
  'onboarding.error.retry': 'Try again',
  'onboarding.error.generic': 'That didn’t work. Please try again.',
  'onboarding.error.rateLimited': 'Too many tries. Please wait a bit and try again.',
  'onboarding.continue': 'Continue',

  'onboarding.signIn.title': 'Make cards of the things you love',
  'onboarding.signIn.body': 'Snap it, cut it out, send it to your closest friends.',
  'onboarding.signIn.apple': 'Continue with Apple',
  'onboarding.signIn.google': 'Continue with Google',
  'onboarding.signIn.unavailable': 'This sign-in option isn’t available yet.',
  'onboarding.signIn.failed': 'We couldn’t sign you in. Please try again.',

  'onboarding.birthDate.title': 'When’s your birthday?',
  'onboarding.birthDate.body':
    'You can’t change this later, so please enter your real date of birth.',
  'onboarding.birthDate.day': 'Day',
  'onboarding.birthDate.month': 'Month',
  'onboarding.birthDate.year': 'Year',
  'onboarding.birthDate.dayPlaceholder': 'DD',
  'onboarding.birthDate.monthPlaceholder': 'MM',
  'onboarding.birthDate.yearPlaceholder': 'YYYY',
  'onboarding.birthDate.invalid': 'Enter a real date.',
  'onboarding.birthDate.submit': 'Continue',

  'onboarding.refused.title': 'We can’t set up an account',
  'onboarding.refused.body': 'Sorry, we’re not able to create an account for you.',
  'onboarding.refused.back': 'Back to start',

  'onboarding.displayName.title': 'What should friends call you?',
  'onboarding.displayName.label': 'Display name',
  'onboarding.displayName.hint':
    '{min, number} to {max, number} characters. Friends see this name.',
  'onboarding.displayName.tooShort': 'Use at least {min, number} characters.',
  'onboarding.displayName.tooLong': 'Use at most {max, number} characters.',
  'onboarding.displayName.rejected': 'Please choose a different name.',

  'onboarding.handle.title': 'Pick your handle',
  'onboarding.handle.label': 'Handle',
  'onboarding.handle.hint':
    '{min, number} to {max, number} characters: lowercase letters, numbers, dots and underscores.',
  'onboarding.handle.invalid':
    'Use {min, number} to {max, number} lowercase letters, numbers, dots or underscores.',
  'onboarding.handle.taken': 'That handle is taken. Try another one.',
  'onboarding.handle.rejected': 'Please choose a different handle.',

  'onboarding.camera.title': 'Allow the camera',
  'onboarding.camera.body':
    'The camera lets you snap things and turn them into cards. Photos stay on your phone until you send a card.',
  'onboarding.camera.allow': 'Allow camera',
  'onboarding.camera.deniedTitle': 'Camera is off',
  'onboarding.camera.deniedBody':
    'To snap cards, turn on camera access for this app in Settings. You can still make cards from photos you already have.',
  'onboarding.camera.openSettings': 'Open Settings',
  'onboarding.camera.library': 'Choose from your photos',
  'onboarding.camera.notNow': 'Not now',

  // Effect registry copy (packages/effects-registry entries reference these keys; a test checks them).
  'effects.cut.none.name': 'Photo',
  'effects.cut.none.description': 'The whole photo with soft rounded corners.',
  'effects.cut.none.a11y': 'as a photo',
  'effects.cut.raw.name': 'Cutout',
  'effects.cut.raw.description': 'Just the subject, no border.',
  'effects.cut.raw.a11y': 'cut out without a border',
  'effects.cut.diecut.name': 'Sticker',
  'effects.cut.diecut.description': 'The subject with a die-cut sticker border.',
  'effects.cut.diecut.a11y': 'as a sticker with a border',
  'effects.fx.holo.name': 'Holo',
  'effects.fx.holo.description': 'Rainbow foil that shifts as you tilt your phone.',
  'effects.fx.holo.a11y': 'with a holographic foil',
  'effects.fx.paperLift.name': 'Paper lift',
  'effects.fx.paperLift.description': 'A soft shadow, as if the sticker lifts off the page.',
  'effects.fx.paperLift.a11y': 'lifted off the page',
} as const;

export type MessageKey = keyof typeof en;
export type Catalog = Record<MessageKey, string>;
