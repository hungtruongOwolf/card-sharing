import type { BrowserContext } from '@playwright/test';

/**
 * Replaces the web cutout segmenter for a browser context.
 *
 * The app cuts the subject out of a photo with BiRefNet_lite in a module Web Worker created from a `blob:` URL
 * (`apps/mobile/src/features/editor/web/cutoutWorker.ts`); the worker downloads transformers.js and a ~115 MB model
 * from public CDNs on the first photo. CI must not depend on that download or on WASM memory, so this init script
 * swaps that one worker for a stand-in that speaks the same protocol (`cutoutProtocol.ts`): every `segment` request
 * is answered with a centred ellipse matte, so the editor gets a deterministic subject. Every other worker is created
 * normally. The rest of the cutout pipeline (mask source, cache, editor, send) runs unchanged.
 */
export async function stubCutoutWorker(context: BrowserContext): Promise<void> {
  await context.addInitScript(() => {
    const RealWorker = window.Worker;

    class EllipseMatteWorker extends EventTarget {
      postMessage(message: unknown): void {
        const request = message as {
          type?: unknown;
          id?: unknown;
          width?: unknown;
          height?: unknown;
        };
        if (request?.type !== 'segment' || typeof request.id !== 'number') return;
        const width = Number(request.width);
        const height = Number(request.height);
        const data = new Uint8Array(width * height);
        const rx = width * 0.32;
        const ry = height * 0.36;
        for (let y = 0; y < height; y++) {
          for (let x = 0; x < width; x++) {
            const dx = (x - width / 2) / rx;
            const dy = (y - height / 2) / ry;
            if (dx * dx + dy * dy <= 1) data[y * width + x] = 255;
          }
        }
        const reply = { type: 'mask', id: request.id, width, height, data };
        setTimeout(() => this.dispatchEvent(new MessageEvent('message', { data: reply })), 20);
      }

      terminate(): void {}
    }

    function Worker(url: string | URL, options?: WorkerOptions): Worker {
      if (String(url).startsWith('blob:') && options?.type === 'module') {
        return new EllipseMatteWorker() as unknown as Worker;
      }
      return new RealWorker(url, options);
    }
    Worker.prototype = RealWorker.prototype;
    window.Worker = Worker as unknown as typeof window.Worker;
  });
}
