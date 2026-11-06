import { isZKey } from '@card-sharing/canvas-engine';

import { cardItem, pin } from '../../../test/boardFixtures';
import { BOARD_SIZE, type Board } from '../boardModel';
import {
  A11Y_MOVE_STEP,
  addPinOp,
  applyOps,
  bringToFrontOp,
  PIN_ACTIONS,
  pinActionOp,
  removePinOp,
  updatePinOp,
} from '../boardOps';
import { createMemoryBoardRepository } from '../boardRepository';
import { UUID_V7 } from '../uuidv7';

const board = (
  pins = [pin({ zKey: 'a0' }), pin({ zKey: 'a1', x: 500, y: 500 }), pin({ zKey: 'a2' })],
): Board => ({
  id: 'b1',
  title: 'Board',
  backdrop: null,
  version: 3,
  pins,
});

describe('board ops', () => {
  it('adds a card pin on top with client ids', () => {
    const b = board();
    const op = addPinOp(b, cardItem(), { x: 10, y: 20 }, () => 0.5);
    expect(op.type).toBe('pin.add');
    if (op.type !== 'pin.add') return;
    expect(op.pin.id).toMatch(UUID_V7);
    expect(op.opId).toMatch(UUID_V7);
    expect(op.pin.zKey > 'a2').toBe(true);
    expect(op.pin.rotation).toBe(0);
    const next = applyOps(b, [op]);
    expect(next.pins).toHaveLength(4);
    expect(next.version).toBe(4);
    // Replaying the same add is a no-op (op ids make server replays safe too).
    expect(applyOps(next, [op]).pins).toHaveLength(4);
  });

  it('sends only changed fields with the base version', () => {
    const p = pin({ x: 5, version: 7 });
    expect(updatePinOp(p, { x: 5 })).toBeNull();
    const op = updatePinOp(p, { x: 6, y: p.y });
    expect(op).toMatchObject({ type: 'pin.update', pinId: p.id, baseVersion: 7, set: { x: 6 } });
    const b = applyOps(board([p]), [op!]);
    expect(b.pins[0]).toMatchObject({ x: 6, version: 8 });
  });

  it('reorders by touching one pin only', () => {
    const b = board();
    const bottom = b.pins[0]!;
    const op = bringToFrontOp(b, bottom)!;
    expect(op).toMatchObject({ type: 'pin.update', pinId: bottom.id });
    expect(Object.keys(op.type === 'pin.update' ? op.set : {})).toEqual(['zKey']);
    const next = applyOps(b, [op]);
    const keys = next.pins.map((p) => p.zKey);
    expect(next.pins.find((p) => p.id === bottom.id)!.zKey).toBe([...keys].sort().pop());
    expect(next.pins.filter((p) => p.id !== bottom.id)).toEqual(b.pins.slice(1));
    expect(
      bringToFrontOp(
        next,
        next.pins.find((p) => p.id === bottom.id)!,
      ),
    ).toBeNull();
  });

  it('maps every accessibility action to one op with valid fields', () => {
    const b = board();
    const p = b.pins[1]!;
    for (const action of PIN_ACTIONS) {
      const op = pinActionOp(b, p, action);
      expect(op).not.toBeNull();
      if (op?.type === 'pin.update') {
        if (op.set.zKey !== undefined) expect(isZKey(op.set.zKey)).toBe(true);
        if (op.set.rotation !== undefined)
          expect(Math.abs(op.set.rotation)).toBeLessThanOrEqual(180);
      }
    }
    expect(pinActionOp(b, p, 'moveRight')).toMatchObject({ set: { x: p.x + A11Y_MOVE_STEP } });
    expect(pinActionOp(b, p, 'remove')).toMatchObject({ type: 'pin.remove', pinId: p.id });
    expect(pinActionOp(b, b.pins[0]!, 'sendToBack')).toBeNull();
    // Pin centres stay on the board (the server stores them as fractions 0..1).
    expect(pinActionOp(b, { ...p, x: 0 }, 'moveLeft')).toBeNull();
    const far = addPinOp(b, cardItem(), { x: -500, y: 99_999 });
    expect(far.type === 'pin.add' && [far.pin.x, far.pin.y]).toEqual([0, BOARD_SIZE.height]);
  });

  it('removes a pin and the repository applies ops', async () => {
    const repo = createMemoryBoardRepository();
    const created = await repo.create('Mine');
    const add = addPinOp(created, cardItem(), { x: 0, y: 0 });
    const withPin = (await repo.apply(created.id, [add]))!;
    expect(withPin.pins).toHaveLength(1);
    const after = await repo.apply(created.id, [removePinOp(withPin.pins[0]!)]);
    expect(after!.pins).toHaveLength(0);
    expect(await repo.apply('missing', [add])).toBeNull();
  });
});
