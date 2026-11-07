/**
 * Minimal Server-Sent Events parser for `GET /v1/events` (ADR-0010). Only `event:` and `data:` fields matter;
 * comments (the `: hb` heartbeat), `id:` and `retry:` lines are ignored. Untrusted input: frames whose data is not
 * a JSON object come back with `data: null`.
 */

export interface SseFrame {
  readonly event: string;
  readonly data: Record<string, unknown> | null;
}

/** Splits `buffer` into complete frames and the unfinished rest (kept for the next chunk). */
export function parseSse(buffer: string): { frames: SseFrame[]; rest: string } {
  const normalized = buffer.replace(/\r\n?/g, '\n');
  const frames: SseFrame[] = [];
  let start = 0;
  for (;;) {
    const end = normalized.indexOf('\n\n', start);
    if (end < 0) break;
    const block = normalized.slice(start, end);
    start = end + 2;
    let event = 'message';
    const data: string[] = [];
    for (const line of block.split('\n')) {
      if (line === '' || line.startsWith(':')) continue;
      const colon = line.indexOf(':');
      const field = colon < 0 ? line : line.slice(0, colon);
      const value = colon < 0 ? '' : line.slice(colon + 1).replace(/^ /, '');
      if (field === 'event') event = value;
      else if (field === 'data') data.push(value);
    }
    if (data.length === 0 && event === 'message') continue; // a comment-only block
    frames.push({ event, data: parseObject(data.join('\n')) });
  }
  return { frames, rest: normalized.slice(start) };
}

function parseObject(text: string): Record<string, unknown> | null {
  try {
    const value: unknown = JSON.parse(text);
    return value !== null && typeof value === 'object' && !Array.isArray(value)
      ? (value as Record<string, unknown>)
      : null;
  } catch {
    return null;
  }
}

/** UTF-8 decoding of stream chunks: `TextDecoder` where the runtime has it, else ASCII (event data is ids only). */
export function createChunkDecoder(): (chunk: Uint8Array) => string {
  if (typeof TextDecoder !== 'undefined') {
    const decoder = new TextDecoder();
    return (chunk) => decoder.decode(chunk, { stream: true });
  }
  return (chunk) => {
    let out = '';
    for (const byte of chunk) out += byte < 0x80 ? String.fromCharCode(byte) : '�';
    return out;
  };
}
