import type { SessionSync } from './sessionSync';

/** Native: one app instance per device, nothing to coordinate. The web build resolves `appSessionSync.web.ts`. */
export function appSessionSync(): SessionSync | undefined {
  return undefined;
}
