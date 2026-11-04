import '@/i18n';

import { AxiosError, AxiosHeaders } from 'axios';
import { beforeEach, describe, expect, it } from 'vitest';

import i18n from '@/i18n';
import { toApiError } from '@/lib/http';

import { signinLimitWait } from './signinLimit';

function limited(retryAfter?: number) {
  return {
    status: 429,
    message: 'too many failed password attempts; try again in 14 minutes',
    code: 'signin_limited',
    ...(retryAfter === undefined ? {} : { retryAfter }),
  };
}

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
});

describe('signinLimitWait', () => {
  it('says the wait in minutes, rounded up', () => {
    expect(signinLimitWait(limited(841), i18n.t)).toBe(
      'Too many failed attempts. Try again in 15 minutes.',
    );
    expect(signinLimitWait(limited(60), i18n.t)).toBe(
      'Too many failed attempts. Try again in 1 minute.',
    );
  });

  it('says a short wait in seconds', () => {
    expect(signinLimitWait(limited(42), i18n.t)).toBe(
      'Too many failed attempts. Try again in 42 seconds.',
    );
  });

  it('falls back to a minute when the answer did not say', () => {
    expect(signinLimitWait(limited(), i18n.t)).toBe(
      'Too many failed attempts. Try again in 1 minute.',
    );
  });

  it('is written in Chinese too', async () => {
    await i18n.changeLanguage('zh-cn');

    expect(signinLimitWait(limited(900), i18n.t)).toBe('尝试次数太多，请 15 分钟后再试。');
  });

  it('leaves other errors alone', () => {
    const locked = {
      status: 429,
      message: 'too many wrong verification codes; try again in 60 minutes',
    };

    expect(signinLimitWait(locked, i18n.t)).toBeNull();
    expect(signinLimitWait({ status: 401, message: 'invalid credentials' }, i18n.t)).toBeNull();
  });
});

describe('toApiError', () => {
  function failedWith(headers: Record<string, string>, data: unknown) {
    return new AxiosError('failed', 'ERR', undefined, undefined, {
      status: 429,
      statusText: 'Too Many Requests',
      headers: new AxiosHeaders(headers),
      config: { headers: new AxiosHeaders() },
      data,
    });
  }

  it('keeps Retry-After and the code', () => {
    const error = toApiError(
      failedWith(
        { 'Retry-After': '840' },
        { error: 'resource_exhausted', code: 'signin_limited', message: 'wait' },
      ),
    );

    expect(error.retryAfter).toBe(840);
    expect(error.code).toBe('signin_limited');
  });

  it('has no retryAfter when the answer has none or it is not a number', () => {
    expect(toApiError(failedWith({}, { message: 'wait' })).retryAfter).toBeUndefined();

    expect(
      toApiError(failedWith({ 'Retry-After': 'soon' }, { message: 'wait' })).retryAfter,
    ).toBeUndefined();
  });
});
