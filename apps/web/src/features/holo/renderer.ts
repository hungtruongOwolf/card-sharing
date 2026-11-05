/**
 * Live holo renderer for the web preview (T-126 spike). Client-only and loaded lazily:
 * importing this module pulls in the CanvasKit loader (~120 KB JS) and the WASM (~7 MB raw),
 * so it must only be reached through a dynamic `import()` after a user interaction.
 */
import CanvasKitInit, { type CanvasKit } from 'canvaskit-wasm';

import { HOLO_SKSL, holoUniformArray } from '@card-sharing/shaders';

import { CANVASKIT_BASE_PATH } from './canvaskit-version';
import { MARK_READY, MARK_TAP, MEASURE_FIRST_FRAME, MEASURE_LOAD } from './timing';

export interface HoloOptions {
  canvas: HTMLCanvasElement;
  imageUrl: string;
  /** Card corner radius in CSS px. */
  cornerRadius: number;
  /** Freeze the ambient animation (prefers-reduced-motion). */
  reducedMotion: boolean;
  /** Overlay strength of the foil over the baked image, 0..1. */
  intensity?: number;
}

export interface HoloHandle {
  /** Updates the tilt, each axis in -1..1. */
  setTilt(x: number, y: number): void;
  stop(): void;
}

let canvasKit: Promise<CanvasKit> | undefined;

function loadCanvasKit(): Promise<CanvasKit> {
  canvasKit ??= CanvasKitInit({ locateFile: (file) => `${CANVASKIT_BASE_PATH}${file}` });
  return canvasKit;
}

async function fetchBytes(url: string): Promise<ArrayBuffer> {
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(`image fetch failed: ${response.status}`);
  }
  return response.arrayBuffer();
}

function measure(name: string, start: string) {
  try {
    performance.measure(name, start);
  } catch {
    // The start mark is missing (e.g. cleared); timing is best-effort only.
  }
}

/** Loads CanvasKit, compiles the shared holo SkSL and starts drawing; resolves after the first frame. */
export async function startHolo(options: HoloOptions): Promise<HoloHandle> {
  const { canvas, imageUrl, cornerRadius, reducedMotion, intensity = 0.55 } = options;
  const [ck, bytes] = await Promise.all([loadCanvasKit(), fetchBytes(imageUrl)]);
  performance.mark(MARK_READY);
  measure(MEASURE_LOAD, MARK_TAP);

  const dpr = Math.min(window.devicePixelRatio || 1, 2);
  canvas.width = Math.round(canvas.clientWidth * dpr);
  canvas.height = Math.round(canvas.clientHeight * dpr);

  const surface = ck.MakeWebGLCanvasSurface(canvas);
  const image = ck.MakeImageFromEncoded(bytes);
  const effect = ck.RuntimeEffect.Make(HOLO_SKSL);
  if (!surface || !image || !effect) {
    image?.delete();
    effect?.delete();
    surface?.dispose();
    throw new Error('CanvasKit could not create the surface, image or shader');
  }
  // MakeWebGLCanvasSurface silently falls back to a CPU surface; record which one we got.
  canvas.dataset.backend = surface.reportBackendTypeIsGPU() ? 'webgl' : 'cpu';

  const width = canvas.width;
  const height = canvas.height;
  const rrect = ck.RRectXY(
    ck.LTRBRect(0, 0, width, height),
    cornerRadius * dpr,
    cornerRadius * dpr,
  );
  const src = ck.LTRBRect(0, 0, image.width(), image.height());
  const dst = ck.LTRBRect(0, 0, width, height);
  const imagePaint = new ck.Paint();
  imagePaint.setAntiAlias(true);
  const foilPaint = new ck.Paint();
  foilPaint.setAntiAlias(true);
  foilPaint.setBlendMode(ck.BlendMode.Overlay);
  foilPaint.setAlphaf(intensity);

  const start = performance.now();
  let tiltX = 0;
  let tiltY = 0;
  let frame = 0;
  let firstFrame = true;
  let stopped = false;

  const draw = () => {
    if (stopped) {
      return;
    }
    const sk = surface.getCanvas();
    sk.clear(ck.TRANSPARENT);
    sk.save();
    sk.clipRRect(rrect, ck.ClipOp.Intersect, true);
    sk.drawImageRectOptions(image, src, dst, ck.FilterMode.Linear, ck.MipmapMode.None, imagePaint);
    const shader = effect.makeShader(
      holoUniformArray({
        width,
        height,
        time: reducedMotion ? 0 : (performance.now() - start) / 1000,
        tiltX,
        tiltY,
      }),
    );
    foilPaint.setShader(shader);
    sk.drawRRect(rrect, foilPaint);
    sk.restore();
    surface.flush();
    shader.delete();
    if (firstFrame) {
      firstFrame = false;
      measure(MEASURE_FIRST_FRAME, MARK_TAP);
    }
    frame = requestAnimationFrame(draw);
  };
  draw();

  return {
    setTilt(x, y) {
      tiltX = x;
      tiltY = y;
    },
    stop() {
      stopped = true;
      cancelAnimationFrame(frame);
      foilPaint.delete();
      imagePaint.delete();
      effect.delete();
      image.delete();
      surface.dispose();
    },
  };
}
