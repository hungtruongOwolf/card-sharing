import {
  asciiBytes,
  base64Url,
  createPkcePair,
  hex,
  pkceChallenge,
  SecureRandomUnavailableError,
  sha256,
} from '../pkce';

const hashHex = (text: string) => hex(sha256(asciiBytes(text)));

describe('sha256 (FIPS 180-4 test vectors)', () => {
  it('hashes the empty string, "abc" and the two-block message', () => {
    expect(hashHex('')).toBe('e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855');
    expect(hashHex('abc')).toBe('ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad');
    expect(hashHex('abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq')).toBe(
      '248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1',
    );
  });

  it('pads at every block boundary (55, 56, 63, 64 and 65 bytes)', () => {
    // Expected values from a reference implementation (Python hashlib.sha256).
    expect(hashHex('a'.repeat(55))).toBe(
      '9f4390f8d30c2dd92ec9f095b65e2b9ae9b0a925a5258e241c9f1e910f734318',
    );
    expect(hashHex('a'.repeat(56))).toBe(
      'b35439a4ac6f0948b6d6f9e3c6af0f5f590ce20f1bde7090ef7970686ec6738a',
    );
    expect(hashHex('a'.repeat(63))).toBe(
      '7d3e74a05d7db15bce4ad9ec0658ea98e3f06eeecf16b4c6fff2da457ddc2f34',
    );
    expect(hashHex('a'.repeat(64))).toBe(
      'ffe054fe7ae0cb6dc65c3af9b61d5209f439851db43d0ba5997337df154668eb',
    );
    expect(hashHex('a'.repeat(65))).toBe(
      '635361c48bb9eab14198e76ea8ab7f1a41685d6ad62aa9146d301d4f17eb0ae0',
    );
  });

  it('hashes a million bytes', () => {
    expect(hex(sha256(new Uint8Array(1_000_000).fill(0x61)))).toBe(
      'cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0',
    );
  });
});

describe('base64Url', () => {
  it('encodes without padding (RFC 4648 section 10)', () => {
    const text = (s: string) => base64Url(asciiBytes(s));
    expect(text('')).toBe('');
    expect(text('f')).toBe('Zg');
    expect(text('fo')).toBe('Zm8');
    expect(text('foo')).toBe('Zm9v');
    expect(text('foob')).toBe('Zm9vYg');
    expect(base64Url(new Uint8Array([0xfb, 0xff]))).toBe('-_8');
  });
});

describe('PKCE S256', () => {
  it('matches RFC 7636 appendix B', () => {
    expect(pkceChallenge('dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk')).toBe(
      'E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM',
    );
  });

  it('makes a 43-character verifier from 32 random bytes and its challenge', () => {
    const random = jest.fn((bytes: Uint8Array) => bytes.fill(7));
    const pair = createPkcePair(random);
    expect(random).toHaveBeenCalledWith(expect.any(Uint8Array));
    expect(random.mock.calls[0]?.[0]).toHaveLength(32);
    expect(pair.verifier).toMatch(/^[A-Za-z0-9_-]{43}$/);
    expect(pair.challenge).toBe(pkceChallenge(pair.verifier));
  });

  it('refuses to work without a secure random source', () => {
    expect(() => createPkcePair(null)).toThrow(SecureRandomUnavailableError);
  });
});
