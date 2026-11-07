import appJson from '../app.json';

declare const __dirname: string;
const { readFileSync } = jest.requireActual('fs') as {
  readFileSync(file: string, encoding: 'utf8'): string;
};
const { join } = jest.requireActual('path') as { join(...parts: string[]): string };

// The API answers 426 UPGRADE_REQUIRED to any client whose `X-App-Version` (the Expo config version) is below
// `min_app_version` in config/client.yaml, which would block every request, sign-in included.
const parse = (version: string): [number, number, number] => {
  const match = /^(\d+)\.(\d+)\.(\d+)/.exec(version);
  if (!match) throw new Error(`not a semantic version: ${version}`);
  return [Number(match[1]), Number(match[2]), Number(match[3])];
};

const compare = (a: [number, number, number], b: [number, number, number]): number =>
  a[0] - b[0] || a[1] - b[1] || a[2] - b[2];

describe('app version', () => {
  it('is not below the minimum version the API accepts', () => {
    const clientYaml = readFileSync(join(__dirname, '../../../config/client.yaml'), 'utf8');
    const min = /^min_app_version:\s*["']?([^"'\s]+)/m.exec(clientYaml)?.[1];
    if (!min) throw new Error('config/client.yaml has no min_app_version');
    expect(compare(parse(appJson.expo.version), parse(min))).toBeGreaterThanOrEqual(0);
  });
});
