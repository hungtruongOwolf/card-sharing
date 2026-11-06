import { headers } from 'next/headers';
import { cache } from 'react';

import {
  type BoardLookup,
  lookupPublicBoard,
  mediaOrigin,
} from '../../../features/public-board/public-board';
import { readServiceApiConfig, visitorIp } from '../../../features/service-api/service-api';

/**
 * One API call per request: `generateMetadata` and the page share this lookup (React `cache` is
 * scoped to a single server render).
 */
export const loadBoard = cache(async (token: string): Promise<BoardLookup> => {
  const config = readServiceApiConfig(process.env);
  if (config === null) {
    return { kind: 'unavailable' };
  }
  const ip = visitorIp(await headers(), config.clientIpHeader);
  return lookupPublicBoard(token, config, ip, mediaOrigin(process.env));
});
