import { lookupPublicBoardOg, mediaOrigin } from '../../../../features/public-board/public-board';
import { readServiceApiConfig, visitorIp } from '../../../../features/service-api/service-api';

type Context = { params: Promise<{ token: string }> };

/** Same shared-cache lifetime as the page (api-mvp: `s-maxage=60`, purged by `Cache-Tag`). */
const CACHEABLE = 'public, max-age=0, s-maxage=60';

/**
 * `GET /b/<token>/og` (T-132, AC-24.7): the link-preview image. Answers 302 to a fresh signed URL of
 * the watermarked OG snapshot, tagged with the link's `Cache-Tag` so a revoke purges it. Every
 * unusable token is the same empty 404.
 */
export async function GET(request: Request, { params }: Context): Promise<Response> {
  const { token } = await params;
  const config = readServiceApiConfig(process.env);
  if (config === null) {
    return unavailable();
  }
  const ip = visitorIp(request.headers, config.clientIpHeader);
  const result = await lookupPublicBoardOg(token, config, ip, mediaOrigin(process.env));
  if (result.kind === 'gone') {
    return new Response(null, { status: 404, headers: { 'Cache-Control': CACHEABLE } });
  }
  if (result.kind === 'unavailable') {
    return unavailable();
  }
  return new Response(null, {
    status: 302,
    headers: {
      Location: result.url,
      'Cache-Control': CACHEABLE,
      'Cache-Tag': result.cacheTag,
    },
  });
}

function unavailable(): Response {
  return new Response(null, {
    status: 503,
    headers: { 'Cache-Control': 'private, no-store', 'Retry-After': '30' },
  });
}
