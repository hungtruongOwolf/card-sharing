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

  it('reads the caption limit only when it is a sane positive integer', () => {
    const limit = (value: unknown) =>
      parseRemoteConfig({ limits: { caption_max_chars: value } })?.captionMaxGraphemes;
    expect(limit(80)).toBe(80);
    for (const bad of [0, -1, 2.5, 1e9, '80', null]) expect(limit(bad)).toBeUndefined();
  });

  it('reads the served text filter hints, all or nothing', () => {
    const textFilter = {
      version: 1,
      max_marks_per_grapheme: 4,
      max_chars_per_grapheme: 16,
      lists: { demo: { words: ['darn'], contains: ['frack'] } },
    };
    expect(parseRemoteConfig({ text_filter: textFilter })?.textFilter).toEqual({
      maxMarksPerGrapheme: 4,
      maxCharsPerGrapheme: 16,
      lists: { demo: { words: ['darn'], contains: ['frack'] } },
    });
    for (const bad of [undefined, { ...textFilter, version: 9 }, { ...textFilter, lists: 'x' }]) {
      const config = parseRemoteConfig({ text_filter: bad });
      expect(config).not.toBeNull();
      expect(config?.textFilter).toBeUndefined();
    }
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
