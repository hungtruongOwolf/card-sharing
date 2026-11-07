/**
 * ADR-0010 "reply with a card": the card being answered travels as route params from the story viewer through
 * capture and the editor to the send screen. Only ids travel (never content); the server decides whether the reply
 * is allowed (404 when I may no longer see the original) and sends it to the original sender only.
 */

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

export interface ReplyTarget {
  /** The friend's card I am answering. */
  readonly cardId: string;
  /** Its sender: the only recipient of the reply. */
  readonly senderId: string;
}

/** Route params for a reply target (merged into the capture, editor and send params). */
export function replyParams(target: ReplyTarget | null): Record<string, string> {
  return target ? { replyTo: target.cardId, replyToSender: target.senderId } : {};
}

/** The reply target in route params, or null (absent or malformed: a normal send). */
export function parseReplyTarget(params: Readonly<Record<string, unknown>>): ReplyTarget | null {
  const cardId = params.replyTo;
  const senderId = params.replyToSender;
  if (typeof cardId !== 'string' || typeof senderId !== 'string') return null;
  if (!UUID.test(cardId) || !UUID.test(senderId)) return null;
  return { cardId, senderId };
}
