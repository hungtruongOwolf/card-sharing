import { forceLtrProps } from '../forceLtr';

describe('forceLtrProps', () => {
  it('uses the direction style on native', () => {
    expect(forceLtrProps('ios')).toEqual({ style: { direction: 'ltr' } });
    expect(forceLtrProps('android')).toEqual({ style: { direction: 'ltr' } });
  });

  it('uses the dir prop on the web, never the direction style', () => {
    const props = forceLtrProps('web');
    expect(props).toEqual({ dir: 'ltr' });
    expect(props.style).toBeUndefined();
  });
});
