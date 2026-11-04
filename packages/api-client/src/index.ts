// Types generated from the Rust API's OpenAPI document — never hand-edit schema.d.ts.
// Regenerate with `pnpm api:client` from the repo root (CI fails on drift).
import type { components } from './schema';

export type { components, operations, paths } from './schema';

/** RFC 9457 error body returned by every failing API call. */
export type ProblemDetails = components['schemas']['ProblemDetails'];
