import { fetchRemoteConfig, parseRemoteConfig } from '../remoteConfig';

describe('remote config', () => {
  it('keeps flags and kill switches and drops malformed parts', () => {
    expect(
      parseRemoteConfig({
        flags: { a: true, b: 'yes' },
        kill_switches: { cut_styles: ['cut.raw', 3], finishes: 'fx.holo' },
        limits: {},
      }),
    ).toEqual({
      registry: {
        flags: { a: true },
        kill_switches: { cut_styles: ['cut.raw'], finishes: [], effects: [], reactions: [] },
      },
    });
    expect(parseRemoteConfig('nope')).toBeNull();
  });

  it('fetches GET /v1/config and fails on an error status', async () => {
    const fetchFn = jest.fn(async (url: string) =>
      url.endsWith('/v1/config')
        ? new Response(JSON.stringify({ flags: {}, kill_switches: { effects: ['fx.holo'] } }))
        : new Response('', { status: 404 }),
    );
    const config = await fetchRemoteConfig('https://api.example', fetchFn);
    expect(config.registry.kill_switches?.effects).toEqual(['fx.holo']);
    expect(fetchFn).toHaveBeenCalledWith('https://api.example/v1/config', expect.anything());
    await expect(
      fetchRemoteConfig('https://api.example', async () => new Response('', { status: 503 })),
    ).rejects.toThrow(/503/);
  });
});
