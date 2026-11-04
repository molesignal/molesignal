import type { TFunction } from 'i18next';

import type { ApiError } from '@/lib/http';

/**
 * How long the sign-in limit asks to wait, in the reader's language; `null` for any other error.
 * The backend answers it the same way for an account that does not exist.
 */
export function signinLimitWait(error: ApiError, t: TFunction): string | null {
  if (error.code !== 'signin_limited') return null;

  const seconds = error.retryAfter ?? 60;

  return seconds < 60
    ? t('shell:signin.limited_seconds', { count: seconds })
    : t('shell:signin.limited_minutes', { count: Math.ceil(seconds / 60) });
}
