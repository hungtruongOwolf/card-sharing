/**
 * The stable `code` of an RFC 9457 problem body (api-mvp "Errors"), or undefined when the body is not one.
 * Reads a clone, so the caller can still consume the response.
 */
export async function problemCode(response: Response): Promise<string | undefined> {
  try {
    const body: unknown = await response.clone().json();
    if (typeof body === 'object' && body !== null && 'code' in body) {
      const { code } = body as { code: unknown };
      return typeof code === 'string' ? code : undefined;
    }
  } catch {
    // Not a problem+json body.
  }
  return undefined;
}
