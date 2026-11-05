'use client';

import { type ReactNode, useEffect, useRef, useState } from 'react';

import type { HoloHandle } from './renderer';
import { MARK_TAP } from './timing';

type Status = 'idle' | 'loading' | 'live' | 'unavailable';

/** iOS 13+ exposes a static permission request for motion sensors; other browsers do not. */
type OrientationPermission = { requestPermission?: () => Promise<'granted' | 'denied'> };

interface Props {
  /** Same-origin baked card image; drawn under the foil once CanvasKit is ready. */
  imageUrl: string;
  imageWidth: number;
  imageHeight: number;
  alt: string;
  cornerRadius: number;
  /** The server-rendered baked card (the LCP element). */
  children: ReactNode;
}

/** Degrees of device tilt that map to a full -1..1 shader tilt. */
const TILT_RANGE_DEG = 30;
/** Phone held at a typical reading angle counts as "no tilt" on the pitch axis. */
const NEUTRAL_PITCH_DEG = 45;

/**
 * Baked card that opens a live holo "enlarge" view on tap. CanvasKit is fetched only here,
 * through a dynamic import, so it never lands in the page's initial JS.
 */
export function HoloEnlarge({
  imageUrl,
  imageWidth,
  imageHeight,
  alt,
  cornerRadius,
  children,
}: Props) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [open, setOpen] = useState(false);
  const [status, setStatus] = useState<Status>('idle');

  const onOpen = () => {
    performance.clearMarks(MARK_TAP);
    performance.mark(MARK_TAP);
    // Must run inside the user gesture on iOS; a refusal just leaves pointer tilt.
    const orientation = globalThis.DeviceOrientationEvent as OrientationPermission | undefined;
    void orientation?.requestPermission?.().catch(() => undefined);
    setStatus('loading');
    setOpen(true);
    dialogRef.current?.showModal();
  };

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!open || !canvas) {
      return;
    }
    let handle: HoloHandle | undefined;
    let cancelled = false;
    const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;

    import('./renderer')
      .then(({ startHolo }) => startHolo({ canvas, imageUrl, cornerRadius, reducedMotion }))
      .then((started) => {
        if (cancelled) {
          started.stop();
          return;
        }
        handle = started;
        setStatus('live');
      })
      .catch(() => {
        if (!cancelled) {
          setStatus('unavailable');
        }
      });

    const onPointerMove = (event: PointerEvent) => {
      const box = canvas.getBoundingClientRect();
      handle?.setTilt(
        ((event.clientX - box.left) / box.width) * 2 - 1,
        ((event.clientY - box.top) / box.height) * 2 - 1,
      );
    };
    const onPointerLeave = () => handle?.setTilt(0, 0);
    const onOrientation = (event: DeviceOrientationEvent) => {
      if (event.gamma === null || event.beta === null) {
        return;
      }
      handle?.setTilt(
        event.gamma / TILT_RANGE_DEG,
        (event.beta - NEUTRAL_PITCH_DEG) / TILT_RANGE_DEG,
      );
    };
    canvas.addEventListener('pointermove', onPointerMove);
    canvas.addEventListener('pointerleave', onPointerLeave);
    window.addEventListener('deviceorientation', onOrientation);

    return () => {
      cancelled = true;
      canvas.removeEventListener('pointermove', onPointerMove);
      canvas.removeEventListener('pointerleave', onPointerLeave);
      window.removeEventListener('deviceorientation', onOrientation);
      handle?.stop();
    };
  }, [open, imageUrl, cornerRadius]);

  return (
    <>
      <button type="button" className="card-button" onClick={onOpen} aria-haspopup="dialog">
        {children}
        <span className="card-button-hint">Tap to enlarge</span>
      </button>
      <dialog
        ref={dialogRef}
        className="holo-dialog"
        aria-label="Enlarged card"
        onClose={() => {
          setOpen(false);
          setStatus('idle');
        }}
      >
        <div className="holo-stage" data-status={status}>
          {/* The baked still shows instantly; the live canvas fades in over it once ready. */}
          {/* eslint-disable-next-line @next/next/no-img-element -- see the baked card in page.tsx */}
          <img
            src={imageUrl}
            alt=""
            width={imageWidth}
            height={imageHeight}
            className="holo-still"
          />
          <canvas ref={canvasRef} className="holo-canvas" role="img" aria-label={alt} />
        </div>
        <p className="muted holo-status" role="status">
          {status === 'loading' && 'Loading the live effect…'}
          {status === 'live' && 'Move your phone or pointer to tilt the foil.'}
          {status === 'unavailable' && 'The live effect is not available on this device.'}
        </p>
        <button type="button" className="holo-close" onClick={() => dialogRef.current?.close()}>
          Close
        </button>
      </dialog>
    </>
  );
}
