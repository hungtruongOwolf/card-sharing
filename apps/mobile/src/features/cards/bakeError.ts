/** A card could not be baked (no surface, encoder failure, unknown cut style). Free of Skia so screens can import it. */
export class BakeError extends Error {
  override readonly name = 'BakeError';
}
