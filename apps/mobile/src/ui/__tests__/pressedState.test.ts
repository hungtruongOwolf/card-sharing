import { pressedStateProps } from '../pressedState';

describe('pressedStateProps', () => {
  it('announces a toggle button as pressed or not on the web', () => {
    expect(pressedStateProps(true, 'web')).toEqual({ 'aria-pressed': true });
    expect(pressedStateProps(false, 'web')).toEqual({ 'aria-pressed': false });
  });

  it('adds nothing on native, where accessibilityState carries the state', () => {
    expect(pressedStateProps(true, 'ios')).toEqual({});
    expect(pressedStateProps(true, 'android')).toEqual({});
  });
});
