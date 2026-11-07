import { readFileSync } from 'node:fs';
import { join } from 'node:path';

import appJson from '../app.json';

// The API answers 426 UPGRADE_REQUIRED to any client whose `X-App-Version` (the Expo config version) is below
// `min_app_version` in config/client.yaml, which would block every request, sign-in included.
const parse = (version: string): number[] => {
  const match = /^(\d+)\.(\d+)\.(\d+)/.exec(version);
  if (!match) throw new Error(`not a semantic version: ${version}`);
  return match.slice(1).map(Number);
};

const compare = (a: number[], b: number[]): number => {
  for (let i = 0; i < 3; i += 1) if (a[i] !== b[i]) return a[i] - b[i];
  return 0;
};

describe('app version', () => {
  it('is not below the minimum version the API accepts', () => {
    const clientYaml = readFileSync(join(__dirname, '../../../config/client.yaml'), 'utf8');
    const min = /^min_app_version:\s*["']?([^"'\s]+)/m.exec(clientYaml)?.[1];
    expect(min).toBeDefined();
    expect(compare(parse(appJson.expo.version), parse(min as string))).toBeGreaterThanOrEqual(0);
  });
});
