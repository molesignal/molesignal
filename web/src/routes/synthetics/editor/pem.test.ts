import { describe, expect, it } from 'vitest';

import { pemParts } from './pem';

const LEAF = '-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----';
const ISSUER = '-----BEGIN CERTIFICATE-----\nBBBB\n-----END CERTIFICATE-----';
const KEY = '-----BEGIN PRIVATE KEY-----\nCCCC\n-----END PRIVATE KEY-----';

describe('pemParts', () => {
  it('takes the chain and the key apart from one file', () => {
    expect(pemParts(`Bag Attributes\n${LEAF}\n${KEY}\n${ISSUER}\n`)).toEqual({
      certificates: `${LEAF}\n${ISSUER}`,
      key: KEY,
    });
  });

  it('finds nothing in a file that is not PEM', () => {
    expect(pemParts('0\u0082\u0003')).toEqual({ certificates: '', key: '' });
  });

  it('reads an RSA or EC key as a key', () => {
    const rsa = '-----BEGIN RSA PRIVATE KEY-----\nDDDD\n-----END RSA PRIVATE KEY-----';

    expect(pemParts(rsa).key).toBe(rsa);
    expect(pemParts(rsa).certificates).toBe('');
  });
});
