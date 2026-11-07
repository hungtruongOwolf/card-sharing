import { createSyntheticMaskSource, noMaskSource, type MaskSource } from './maskSource';

let shared: MaskSource | null = null;

/**
 * The app's one mask source, shared by the editor, the send screen and the outbox so a sent card is baked with the
 * mask the user styled. Native, until the on-device segmenters land (T-54/T-55): a synthetic subject in development
 * builds and whole-photo cards in release builds. The web target resolves `appMaskSource.web.ts` (BiRefNet_lite).
 */
export function appMaskSource(): MaskSource {
  shared ??= __DEV__ ? createSyntheticMaskSource() : noMaskSource;
  return shared;
}
