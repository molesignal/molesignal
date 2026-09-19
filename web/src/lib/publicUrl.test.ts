import { describe, expect, it } from 'vitest';

import { resolvePublicBaseUrl, resolvePublicUrl } from './publicUrl';

describe('public URL resolution', () => {
  it('prefers the configured external URL and removes trailing slashes', () => {
    expect(
      resolvePublicBaseUrl(
        ' https://molesignal.example.com/// ',
        'http://localhost:5080',
      ),
    ).toBe('https://molesignal.example.com');
  });

  it('falls back to the browser origin when no external URL is configured', () => {
    expect(resolvePublicBaseUrl('', 'http://localhost:5080/')).toBe(
      'http://localhost:5080',
    );
  });

  it('turns endpoint paths into complete URLs including host and port', () => {
    expect(
      resolvePublicUrl('/api/v1/mcp', 'http://localhost:5080'),
    ).toBe('http://localhost:5080/api/v1/mcp');
    expect(
      resolvePublicUrl(
        '/.well-known/oauth-authorization-server',
        'https://molesignal.example.com',
      ),
    ).toBe(
      'https://molesignal.example.com/.well-known/oauth-authorization-server',
    );
  });

  it('preserves an absolute HTTP URL returned by the server', () => {
    expect(
      resolvePublicUrl(
        'https://public.example.com/api/v1/oauth/token',
        'http://localhost:5080',
      ),
    ).toBe('https://public.example.com/api/v1/oauth/token');
  });
});
