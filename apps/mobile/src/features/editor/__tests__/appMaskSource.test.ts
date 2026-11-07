import { appMaskSource } from '../appMaskSource';

it('is one shared instance, so the editor, send and outbox bake with the same masks', () => {
  expect(appMaskSource()).toBe(appMaskSource());
});

it('uses the synthetic subject in development builds until the native segmenters land', async () => {
  const mask = await appMaskSource().maskFor({
    uri: 'file:///photo.jpg',
    width: 400,
    height: 200,
    origin: 'camera',
  });
  expect(mask).toMatchObject({ width: 256, height: 128 });
});
