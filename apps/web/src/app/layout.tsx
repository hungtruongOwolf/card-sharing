import type { Metadata, Viewport } from 'next';
import { connection } from 'next/server';
import type { ReactNode } from 'react';

import { themes } from '@card-sharing/tokens';

import { SITE_NAME, siteUrl } from '../site';

import './globals.css';

export const metadata: Metadata = {
  metadataBase: siteUrl(),
  title: { default: SITE_NAME, template: `%s · ${SITE_NAME}` },
  description: 'Make and share little cards and boards with friends.',
};

export const viewport: Viewport = {
  themeColor: [
    { media: '(prefers-color-scheme: light)', color: themes.light.background },
    { media: '(prefers-color-scheme: dark)', color: themes.dark.background },
  ],
};

export default async function RootLayout({ children }: { children: ReactNode }) {
  // Nonce-based CSP (src/proxy.ts) needs every page rendered per request.
  await connection();
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
