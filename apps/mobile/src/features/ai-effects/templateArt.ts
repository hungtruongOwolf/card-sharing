import { type GradientName } from '@card-sharing/tokens';

/**
 * Preview art for curated AI templates, keyed by template id (config over code: a template the app does not know
 * yet still shows, with the default art). The server sends no preview images yet; when pre-rendered previews land
 * on the CDN (genai-effects.md "Cache"), they replace this art.
 */
export interface TemplateArt {
  gradient: GradientName;
  /** Decorative glyph drawn on the gradient (the tile's label is the template title). */
  glyph: string;
}

export const DEFAULT_TEMPLATE_ART: TemplateArt = { gradient: 'primary', glyph: '✧' };

export const TEMPLATE_ART: Readonly<Record<string, TemplateArt>> = {
  'ai.watercolor': { gradient: 'story', glyph: '❀' },
  'ai.neon_night': { gradient: 'primary', glyph: '☾' },
  'ai.vintage_photo': { gradient: 'story', glyph: '◐' },
};

export const templateArt = (templateId: string): TemplateArt =>
  TEMPLATE_ART[templateId] ?? DEFAULT_TEMPLATE_ART;
