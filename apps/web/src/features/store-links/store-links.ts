import { type Env, httpUrl } from '../service-api/service-api.ts';

export type StoreLink = { readonly label: string; readonly href: string };

/**
 * "Get the app" links from server env (`APP_STORE_URL`, `PLAY_STORE_URL`, https only). Without any,
 * one "Get the app" button points to the home page.
 */
export function storeLinks(env: Env): StoreLink[] {
  const links = [
    { label: 'App Store', href: env.APP_STORE_URL },
    { label: 'Google Play', href: env.PLAY_STORE_URL },
  ].filter(
    (link): link is StoreLink =>
      link.href !== undefined && httpUrl(link.href)?.protocol === 'https:',
  );
  return links.length > 0 ? links : [{ label: 'Get the app', href: '/' }];
}
