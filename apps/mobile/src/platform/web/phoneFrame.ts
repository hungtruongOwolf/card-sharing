/**
 * Phone-size frame for the web target (ADR-0009). On a desktop-width window the page shows the app inside a
 * centred phone-size iframe of the same URL; inside the iframe (and on a phone browser) the app renders full
 * screen. An iframe rather than a styled container keeps every screen correct without changes: the app's window
 * is the frame, so `useWindowDimensions`, modals (rendered into `document.body`) and fixed positions all stay
 * inside it. The outer page never loads CanvasKit or renders React.
 */

/** Design size of the frame, about a large current phone in CSS px. */
export const FRAME_WIDTH = 430;
export const FRAME_HEIGHT = 932;
/** Windows narrower than this are phones (or a narrow desktop window): no frame, full screen. */
export const FRAME_MIN_WINDOW_WIDTH = 600;
/** Space kept around the frame when the window is shorter than the frame. */
const FRAME_MARGIN = 16;

export interface FrameWindow {
  readonly innerWidth: number;
  readonly innerHeight: number;
  /** False inside an iframe (`window.self !== window.top`). */
  readonly isTopLevel: boolean;
}

/** Whether this page should draw the phone frame instead of the app. */
export function shouldFrame(win: FrameWindow): boolean {
  return win.isTopLevel && win.innerWidth >= FRAME_MIN_WINDOW_WIDTH;
}

/** The frame height for a window: the design height, shrunk to fit short windows. */
export function frameHeight(innerHeight: number): number {
  return Math.max(320, Math.min(FRAME_HEIGHT, innerHeight - 2 * FRAME_MARGIN));
}

export function topLevel(win: Window): FrameWindow {
  let isTopLevel = true;
  try {
    isTopLevel = win.self === win.top;
  } catch {
    // A cross-origin parent throws on access: we are embedded.
    isTopLevel = false;
  }
  return { innerWidth: win.innerWidth, innerHeight: win.innerHeight, isTopLevel };
}

/**
 * Replaces the page with the frame: a dark backdrop and an iframe of `src` (the same URL, so deep links open
 * the same screen). Returns the iframe.
 */
export function mountPhoneFrame(
  doc: Document,
  win: Pick<Window, 'innerHeight' | 'addEventListener'>,
  src: string,
  title: string,
): HTMLIFrameElement {
  const body = doc.body;
  body.style.margin = '0';
  body.style.minHeight = '100vh';
  body.style.display = 'flex';
  body.style.alignItems = 'center';
  body.style.justifyContent = 'center';
  body.style.background = '#16131a';
  const root = doc.getElementById('root');
  if (root) root.style.display = 'none';

  const frame = doc.createElement('iframe');
  frame.src = src;
  frame.title = title;
  frame.style.width = `${FRAME_WIDTH}px`;
  frame.style.border = '0';
  frame.style.borderRadius = '44px';
  frame.style.boxShadow = '0 24px 80px rgba(0, 0, 0, 0.55), 0 0 0 10px #2a2630';
  frame.style.background = '#000';
  // Camera capture and clipboard work inside the frame as on a phone browser.
  frame.allow = 'camera; clipboard-write';
  const fit = () => {
    frame.style.height = `${frameHeight(win.innerHeight)}px`;
  };
  fit();
  win.addEventListener('resize', fit);
  body.appendChild(frame);
  return frame;
}
