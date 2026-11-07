import {
  FRAME_HEIGHT,
  FRAME_WIDTH,
  frameHeight,
  mountPhoneFrame,
  shouldFrame,
  topLevel,
} from '../phoneFrame';

describe('shouldFrame', () => {
  it('frames a top-level desktop-width window', () => {
    expect(shouldFrame({ innerWidth: 1280, innerHeight: 800, isTopLevel: true })).toBe(true);
  });

  it('renders the app full screen on a phone browser', () => {
    expect(shouldFrame({ innerWidth: 390, innerHeight: 844, isTopLevel: true })).toBe(false);
  });

  it('never frames inside the frame (no recursion)', () => {
    expect(shouldFrame({ innerWidth: 1280, innerHeight: 800, isTopLevel: false })).toBe(false);
  });
});

describe('frameHeight', () => {
  it('uses the design height when the window is tall enough', () => {
    expect(frameHeight(1200)).toBe(FRAME_HEIGHT);
  });

  it('shrinks to fit a short window, with a margin', () => {
    expect(frameHeight(700)).toBe(668);
  });

  it('keeps a usable minimum', () => {
    expect(frameHeight(100)).toBe(320);
  });
});

describe('topLevel', () => {
  it('treats a cross-origin parent (access throws) as embedded', () => {
    const win = {
      innerWidth: 1000,
      innerHeight: 800,
      self: {},
      get top(): never {
        throw new Error('cross-origin');
      },
    } as unknown as Window;
    expect(topLevel(win).isTopLevel).toBe(false);
  });

  it('is top level when self is top', () => {
    const win = { innerWidth: 1000, innerHeight: 800 } as { self?: unknown; top?: unknown };
    win.self = win;
    win.top = win;
    expect(topLevel(win as unknown as Window).isTopLevel).toBe(true);
  });
});

describe('mountPhoneFrame', () => {
  type FakeElement = { style: Record<string, string>; [key: string]: unknown };
  const element = (): FakeElement => ({ style: {} });

  it('hides the app root and shows a phone-size iframe of the same URL', () => {
    const root = element();
    const appended: FakeElement[] = [];
    const body = { ...element(), appendChild: (child: FakeElement) => appended.push(child) };
    const doc = {
      body,
      getElementById: (id: string) => (id === 'root' ? root : null),
      createElement: () => element(),
    } as unknown as Document;
    let onResize: (() => void) | undefined;
    const win = {
      innerHeight: 1000,
      addEventListener: (_: string, listener: () => void) => {
        onResize = listener;
      },
    };

    const frame = mountPhoneFrame(
      doc,
      win as unknown as Window,
      'http://localhost:8081/feed',
      'app',
    );

    expect(root.style.display).toBe('none');
    expect(appended).toEqual([frame]);
    expect(frame.src).toBe('http://localhost:8081/feed');
    expect(frame.title).toBe('app');
    expect(frame.style.width).toBe(`${FRAME_WIDTH}px`);
    expect(frame.style.height).toBe(`${FRAME_HEIGHT}px`);

    win.innerHeight = 700;
    onResize?.();
    expect(frame.style.height).toBe('668px');
  });
});
