import { AiEffectsError, type AiEffectsApi, type AiJob, type MediaState } from '../aiEffectsApi';
import { runAiEffect, type AiPhase, type AiRunDeps } from '../runAiEffect';

const INPUT_ID = '22222222-2222-4222-8222-222222222222';
const OUTPUT_ID = '33333333-3333-4333-8333-333333333333';
const ATTEMPT = '44444444-4444-4444-8444-444444444444';
const template = { id: 'ai.watercolor', version: 1, title: 'Watercolor', cost: 1 };

const job = (over: Partial<AiJob> = {}): AiJob => ({
  id: '11111111-1111-4111-8111-111111111111',
  templateId: 'ai.watercolor',
  templateVersion: 1,
  status: 'queued',
  outcome: null,
  cost: 1,
  refunded: false,
  outputMediaId: null,
  outputUrl: null,
  ...over,
});

function setup(states: Record<string, MediaState[]>, jobs: AiJob[]) {
  let clock = 0;
  const api: jest.Mocked<AiEffectsApi> = {
    overview: jest.fn(),
    grantConsent: jest.fn(),
    createJob: jest.fn().mockResolvedValue(jobs[0]),
    getJob: jest.fn().mockResolvedValue(jobs.at(-1)),
    mediaState: jest.fn(async (id: string) => states[id]?.shift() ?? 'scanning'),
  };
  jobs.slice(1).forEach((next) => api.getJob.mockResolvedValueOnce(next));
  const phases: AiPhase[] = [];
  const deps: AiRunDeps = {
    api,
    render: jest.fn().mockResolvedValue({ bytes: new Uint8Array([1]), contentType: 'image/png' }),
    upload: jest.fn().mockResolvedValue({ id: INPUT_ID, kind: 'card_render', state: 'uploaded' }),
    wait: async (ms) => {
      clock += ms;
    },
    now: () => clock,
    pollMs: 1_000,
    timeoutMs: 10_000,
  };
  const run = () =>
    runAiEffect({ attemptId: ATTEMPT, template, basis: 'abcdef01' }, (p) => phases.push(p), deps);
  return { api, phases, run };
}

describe('runAiEffect', () => {
  it('uploads, waits for the scans, submits once and returns the look', async () => {
    const { api, phases, run } = setup(
      { [INPUT_ID]: ['scanning', 'clean'], [OUTPUT_ID]: ['clean'] },
      [job(), job(), job({ status: 'succeeded', outputMediaId: OUTPUT_ID })],
    );
    await expect(run()).resolves.toEqual({
      kind: 'done',
      previewUrl: null,
      look: {
        jobId: job().id,
        templateId: 'ai.watercolor',
        templateVersion: 1,
        title: 'Watercolor',
        outputMediaId: OUTPUT_ID,
        basis: 'abcdef01',
      },
    });
    expect(api.createJob).toHaveBeenCalledTimes(1);
    expect(api.createJob).toHaveBeenCalledWith({
      clientJobId: ATTEMPT,
      template,
      inputMediaId: INPUT_ID,
    });
    expect(phases).toEqual(['uploading', 'checking', 'generating', 'finishing']);
  });

  it('reports a refusal with its refund', async () => {
    const { run } = setup({ [INPUT_ID]: ['clean'] }, [
      job({ status: 'refused', refunded: true, outcome: 'input_moderation' }),
    ]);
    await expect(run()).resolves.toEqual({
      kind: 'refused',
      refunded: true,
      outcome: 'input_moderation',
    });
  });

  it('never submits when the input fails the content scan', async () => {
    const { api, run } = setup({ [INPUT_ID]: ['scanning', 'blocked'] }, [job()]);
    await expect(run()).rejects.toEqual(new AiEffectsError('input_rejected'));
    expect(api.createJob).not.toHaveBeenCalled();
  });

  it('treats an output that fails the scan as refused', async () => {
    const { run } = setup({ [INPUT_ID]: ['clean'], [OUTPUT_ID]: ['held'] }, [
      job({ status: 'succeeded', outputMediaId: OUTPUT_ID }),
    ]);
    await expect(run()).resolves.toMatchObject({ kind: 'refused', outcome: 'output_moderation' });
  });

  it('times out while the job is still queued', async () => {
    const { run } = setup({ [INPUT_ID]: ['clean'] }, [job()]);
    await expect(run()).resolves.toEqual({ kind: 'timeout' });
  });

  it('passes server errors through (credits, consent)', async () => {
    const { api, run } = setup({ [INPUT_ID]: ['clean'] }, [job()]);
    api.createJob.mockRejectedValue(new AiEffectsError('insufficient_credits', 402));
    await expect(run()).rejects.toEqual(new AiEffectsError('insufficient_credits', 402));
  });
});
