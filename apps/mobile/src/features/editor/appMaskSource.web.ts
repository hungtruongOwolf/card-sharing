import type { MaskSource } from './maskSource';
import { createBirefnetMaskSource } from './web/birefnetMaskSource';
import { createCutoutWorker } from './web/cutoutWorker';

let shared: MaskSource | null = null;

/**
 * Web (ADR-0009, subject-cutout.md): BiRefNet_lite (MIT) in a Web Worker. Nothing loads until the first photo is
 * cut; the photo never leaves the browser.
 */
export function appMaskSource(): MaskSource {
  shared ??= createBirefnetMaskSource({ createWorker: createCutoutWorker });
  return shared;
}
