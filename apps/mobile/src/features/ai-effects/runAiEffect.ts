import type { BakedImage } from '../cards/bake';
import type { UploadedMedia } from '../sharing/cardsApi';
import {
  AiEffectsError,
  type AiEffectsApi,
  type AiJob,
  type AiTemplate,
  type MediaState,
} from './aiEffectsApi';
import type { AiLook } from './aiLook';

/** What the progress view says while a run is in flight. */
export type AiPhase = 'uploading' | 'checking' | 'generating' | 'finishing';

export type AiRunOutcome =
  | { readonly kind: 'done'; readonly look: AiLook; readonly previewUrl: string | null }
  /** The server refused (moderation, provider) or failed; `refunded` says whether the credits came back. */
  | {
      readonly kind: 'refused' | 'failed';
      readonly refunded: boolean;
      readonly outcome: string | null;
    }
  /** Still running after the wait budget: the same attempt id picks the same job up again (not charged twice). */
  | { readonly kind: 'timeout' };

export interface AiRunDeps {
  api: AiEffectsApi;
  /** Bakes the card as it is now (the same bytes every time for the same draft, so re-uploads dedupe). */
  render: () => Promise<BakedImage>;
  /** `POST /v1/media?kind=card_render`. */
  upload: (image: BakedImage) => Promise<UploadedMedia>;
  wait?: (ms: number) => Promise<void>;
  now?: () => number;
  pollMs?: number;
  /** Total wait budget for one run (scans plus generation). */
  timeoutMs?: number;
}

export interface AiRunRequest {
  /** One id per explicit Apply; reused to resume the same job after a timeout or a lost answer. */
  attemptId: string;
  template: AiTemplate;
  /** {@link lookBasis} of the draft `render` bakes. */
  basis: string;
}

export const AI_POLL_MS = 1_500;
export const AI_TIMEOUT_MS = 120_000;

const BAD_MEDIA: readonly MediaState[] = ['held', 'blocked', 'failed', 'deleted'];
const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

/**
 * One AI effect run, only ever started by an explicit tap (Apple 5.1.2(i)): bake the card, upload it as my
 * `card_render`, wait for the content scan, submit the job (credits are reserved by the server), poll it, then wait
 * for the output upload to pass the same scan. Nothing is decided here that the server does not check again.
 */
export async function runAiEffect(
  { attemptId, template, basis }: AiRunRequest,
  onPhase: (phase: AiPhase) => void,
  {
    api,
    render,
    upload,
    wait = sleep,
    now = Date.now,
    pollMs = AI_POLL_MS,
    timeoutMs = AI_TIMEOUT_MS,
  }: AiRunDeps,
): Promise<AiRunOutcome> {
  const deadline = now() + timeoutMs;
  /** Polls an upload until its content scan settles or the wait budget runs out. */
  const scan = async (mediaId: string): Promise<'clean' | 'rejected' | 'timeout'> => {
    for (;;) {
      const state = await api.mediaState(mediaId);
      if (state === 'clean') return 'clean';
      if (BAD_MEDIA.includes(state)) return 'rejected';
      if (now() >= deadline) return 'timeout';
      await wait(pollMs);
    }
  };

  onPhase('uploading');
  const input = await upload(await render());
  onPhase('checking');
  const inputScan = await scan(input.id);
  if (inputScan === 'rejected') throw new AiEffectsError('input_rejected');
  if (inputScan === 'timeout') return { kind: 'timeout' };

  onPhase('generating');
  let job: AiJob = await api.createJob({
    clientJobId: attemptId,
    template,
    inputMediaId: input.id,
  });
  while (job.status === 'queued') {
    if (now() >= deadline) return { kind: 'timeout' };
    await wait(pollMs);
    job = await api.getJob(job.id);
  }
  if (job.status !== 'succeeded') {
    return { kind: job.status, refunded: job.refunded, outcome: job.outcome };
  }

  onPhase('finishing');
  const outputMediaId = job.outputMediaId!;
  const outputScan = await scan(outputMediaId);
  // The output failed the content checks after generation: nothing to show.
  if (outputScan === 'rejected') {
    return { kind: 'refused', refunded: job.refunded, outcome: 'output_moderation' };
  }
  if (outputScan === 'timeout') return { kind: 'timeout' };
  return {
    kind: 'done',
    look: {
      jobId: job.id,
      templateId: job.templateId,
      templateVersion: job.templateVersion,
      title: template.title,
      outputMediaId,
      basis,
    },
    previewUrl: job.outputUrl,
  };
}
