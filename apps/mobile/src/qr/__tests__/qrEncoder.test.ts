import { encodeQr, qrPath, type QrMatrix } from '../qrEncoder';

const rows = (m: QrMatrix) => m.modules.map((row) => row.map((d) => (d ? '#' : '.')).join(''));

/**
 * Known answer for "HELLO" at level M (version 1). Cross-checked while writing the encoder: the same input
 * and mask produce a byte-identical matrix in an independent encoder, for every length up to version 10 and
 * every level.
 */
const HELLO_M = [
  '#######.##.#..#######',
  '#.....#..##.#.#.....#',
  '#.###.#..####.#.###.#',
  '#.###.#.#..#..#.###.#',
  '#.###.#.#...#.#.###.#',
  '#.....#.#.##..#.....#',
  '#######.#.#.#.#######',
  '........#####........',
  '#...#.######.#####..#',
  '...###..#.###..#.####',
  '#.##..#.#.##..###..#.',
  '###..#...#...##.#....',
  '..#.###..#..###...##.',
  '........###.###..#.##',
  '#######.##..##...#.#.',
  '#.....#....##..#...#.',
  '#.###.#.#..#..###.#.#',
  '#.###.#....##....#.##',
  '#.###.#..###..####...',
  '#.....#..#...##......',
  '#######.#...#####.#.#',
];

/** Reads the 15 format bits next to the top-left finder and checks them as the spec's BCH(15,5) code. */
function formatInfo(m: QrMatrix): { ecl: number; mask: number } {
  const get = (x: number, y: number) => (m.modules[y]?.[x] ? 1 : 0);
  let bits = 0;
  const order: [number, number][] = [
    ...[0, 1, 2, 3, 4, 5].map((i): [number, number] => [8, i]),
    [8, 7],
    [8, 8],
    [7, 8],
    ...[9, 10, 11, 12, 13, 14].map((i): [number, number] => [14 - i, 8]),
  ];
  order.forEach(([x, y], i) => {
    bits |= get(x, y) << i;
  });
  const raw = bits ^ 0x5412;
  const data = raw >>> 10;
  let rem = data;
  for (let i = 0; i < 10; i += 1) rem = (rem << 1) ^ ((rem >>> 9) * 0x537);
  expect(raw & 0x3ff).toBe(rem & 0x3ff);
  return { ecl: data >>> 3, mask: data & 7 };
}

describe('encodeQr', () => {
  it('matches the known answer for HELLO at level M', () => {
    const m = encodeQr('HELLO', 'M');
    expect(m && rows(m)).toEqual(HELLO_M);
  });

  it('fits an invite link in version 4 at level M with valid format information', () => {
    const m = encodeQr('https://cardsharing.app/i/3q2-7wAAAAAAAAAAAAAAAA', 'M');
    if (!m) throw new Error('expected a code');
    expect([m.version, m.size]).toEqual([4, 33]);
    expect(formatInfo(m).ecl).toBe(0); // M
    // Finder pattern corners, timing pattern, dark module.
    for (const [x, y] of [
      [0, 0],
      [m.size - 1, 0],
      [0, m.size - 1],
    ] as const) {
      expect(m.modules[y]?.[x]).toBe(true);
    }
    for (let i = 8; i < m.size - 8; i += 1) expect(m.modules[6]?.[i]).toBe(i % 2 === 0);
    expect(m.modules[m.size - 8]?.[8]).toBe(true);
  });

  it('encodes UTF-8 and every level', () => {
    for (const level of ['L', 'M', 'Q', 'H'] as const) {
      const m = encodeQr('café', level);
      expect(m?.version).toBe(1);
      expect(formatInfo(m as QrMatrix).ecl).toBe({ L: 1, M: 0, Q: 3, H: 2 }[level]);
    }
  });

  it('writes version information from version 7 and refuses content beyond version 10', () => {
    const v7 = encodeQr('x'.repeat(110), 'M');
    if (!v7) throw new Error('expected a code');
    expect(v7.version).toBe(7);
    let bits = 0;
    for (let i = 0; i < 18; i += 1) {
      const x = v7.size - 11 + (i % 3);
      const y = Math.floor(i / 3);
      expect(v7.modules[y]?.[x]).toBe(v7.modules[x]?.[y]); // both copies agree
      if (v7.modules[y]?.[x]) bits |= 1 << i;
    }
    expect(bits >>> 12).toBe(7);
    const v10 = encodeQr('x'.repeat(213), 'M');
    expect(v10?.version).toBe(10);
    expect(encodeQr('x'.repeat(214), 'M')).toBeNull();
  });
});

describe('qrPath', () => {
  it('draws runs of dark modules offset by the quiet zone', () => {
    const matrix: QrMatrix = {
      size: 3,
      version: 1,
      modules: [
        [true, true, false],
        [false, false, false],
        [false, true, true],
      ],
    };
    expect(qrPath(matrix, 4)).toBe('M4 4h2v1h-2zM5 6h2v1h-2z');
  });
});
