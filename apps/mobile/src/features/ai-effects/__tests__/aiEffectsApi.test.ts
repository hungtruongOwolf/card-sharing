import {
  AiEffectsError,
  createAiEffectsApi,
  parseJob,
  parseMediaState,
  parseOverview,
  type AiEffectsErrorKind,
} from '../aiEffectsApi';

const JOB_ID = '11111111-1111-4111-8111-111111111111';
const INPUT_ID = '22222222-2222-4222-8222-222222222222';
const OUTPUT_ID = '33333333-3333-4333-8333-333333333333';

const overviewBody = {
  available: true,
  consent: { required_version: 1, granted: false },
  credits: 5,
  ai_label: { visible_badge: true, c2pa: true },
  templates: [
    { id: 'ai.watercolor', version: 1, title: 'Watercolor', cost: 1 },
    { id: 'BAD ID', version: 1, title: 'Bad', cost: 1 },
    { id: 'ai.vintage_photo', version: 1, title: 'Vintage photo', cost: 2 },
  ],
};

const jobBody = (job: Record<string, unknown> = {}) => ({
  job: {
    id: JOB_ID,
    template_id: 'ai.watercolor',
    template_version: 1,
    input_media_id: INPUT_ID,
    status: 'queued',
    outcome: null,
    cost: 1,
    refunded: false,
    output_media_id: null,
    ai_generated: true,
    c2pa_recorded: false,
    created_at: '2026-11-07T10:00:00Z',
    ...job,
  },
});

const json = (status: number, body: unknown) =>
  new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json' },
  });

const newJob = {
  clientJobId: JOB_ID,
  template: { id: 'ai.watercolor', version: 1 },
  inputMediaId: INPUT_ID,
};

describe('parseOverview', () => {
  it('keeps valid templates and reads consent, credits and the badge rule', () => {
    expect(parseOverview(overviewBody)).toEqual({
      available: true,
      consent: { requiredVersion: 1, granted: false },
      credits: 5,
      visibleBadge: true,
      templates: [
        { id: 'ai.watercolor', version: 1, title: 'Watercolor', cost: 1 },
        { id: 'ai.vintage_photo', version: 1, title: 'Vintage photo', cost: 2 },
      ],
    });
  });

  it('lists no templates when AI effects are not available', () => {
    expect(parseOverview({ ...overviewBody, available: false })?.templates).toEqual([]);
  });

  it('rejects malformed bodies and shows the badge when the rule is unreadable', () => {
    expect(parseOverview({ ...overviewBody, credits: -1 })).toBeNull();
    expect(parseOverview({ ...overviewBody, consent: null })).toBeNull();
    expect(parseOverview({ ...overviewBody, ai_label: undefined })?.visibleBadge).toBe(true);
  });
});

describe('parseJob', () => {
  it('reads a queued and a succeeded job', () => {
    expect(parseJob(jobBody())).toMatchObject({
      id: JOB_ID,
      status: 'queued',
      outputMediaId: null,
    });
    expect(parseJob(jobBody({ status: 'succeeded', output_media_id: OUTPUT_ID }))).toMatchObject({
      status: 'succeeded',
      outputMediaId: OUTPUT_ID,
      outputUrl: null,
    });
  });

  it('drops a success without an output, unknown statuses and unsafe preview URLs', () => {
    expect(parseJob(jobBody({ status: 'succeeded' }))).toBeNull();
    expect(parseJob(jobBody({ status: 'running' }))).toBeNull();
    const unsafe = jobBody({
      status: 'succeeded',
      output_media_id: OUTPUT_ID,
      output_url: 'javascript:alert(1)',
    });
    expect(parseJob(unsafe)?.outputUrl).toBeNull();
  });

  it('reads media states, unknown ones as unknown', () => {
    expect(parseMediaState({ media: { state: 'clean' } })).toBe('clean');
    expect(parseMediaState({ media: { state: 'new' } })).toBe('unknown');
    expect(parseMediaState({})).toBeNull();
  });
});

describe('createAiEffectsApi', () => {
  it('sends the job with the client job id as Idempotency-Key', async () => {
    const fetcher = jest.fn().mockResolvedValue(json(202, jobBody()));
    await createAiEffectsApi(fetcher).createJob(newJob);
    const [path, init] = fetcher.mock.calls[0];
    expect(path).toBe('/v1/ai-effects/jobs');
    expect(init.headers['Idempotency-Key']).toBe(JOB_ID);
    expect(JSON.parse(init.body)).toEqual({
      client_job_id: JOB_ID,
      template_id: 'ai.watercolor',
      template_version: 1,
      input_media_id: INPUT_ID,
    });
  });

  it.each<[number, string, AiEffectsErrorKind]>([
    [403, 'CONSENT_REQUIRED', 'consent_required'],
    [402, 'INSUFFICIENT_CREDITS', 'insufficient_credits'],
    [403, 'POLICY_DISALLOWED', 'policy_disallowed'],
    [403, 'ACCOUNT_SUSPENDED', 'not_allowed'],
    [400, 'VALIDATION_FAILED', 'invalid'],
    [429, 'RATE_LIMITED', 'rate_limited'],
    [503, 'TEMPORARILY_UNAVAILABLE', 'unavailable'],
  ])('maps %i %s to %s', async (status, code, kind) => {
    const api = createAiEffectsApi(jest.fn().mockResolvedValue(json(status, { code })));
    await expect(api.createJob(newJob)).rejects.toEqual(new AiEffectsError(kind, status));
  });

  it('puts the consent version and never puts an unvalidated id in a path', async () => {
    const fetcher = jest.fn().mockResolvedValue(new Response(null, { status: 204 }));
    const api = createAiEffectsApi(fetcher);
    await api.grantConsent(1);
    expect(fetcher).toHaveBeenCalledWith(
      '/v1/ai-effects/consent',
      expect.objectContaining({ method: 'PUT', body: JSON.stringify({ version: 1 }) }),
    );
    await expect(api.getJob('../me')).rejects.toBeInstanceOf(AiEffectsError);
    expect(fetcher).toHaveBeenCalledTimes(1);
  });
});
