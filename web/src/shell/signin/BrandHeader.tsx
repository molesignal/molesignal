import type * as React from 'react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';
import { LogoMark } from '@/shell/LogoMark';

/** Fullwidth punctuation a line can end on: its glyph sits at the start of its box. */
const FULLWIDTH_END = /[，。、；：！？]$/u;

/** The class that balances a centred line ending on such a mark (`signin.css`). */
const endMark = (text: string) => (FULLWIDTH_END.test(text) ? 'ms-auth-end-mark' : undefined);

/**
 * Who this is, in one sentence. The mark draws its pulse once as the page
 * opens; the statement and the promise rise in behind it. Nothing else belongs
 * above a sign-in form.
 */
export function BrandHeader() {
  const { t } = useTranslation(['shell', 'common']);
  const taglineEnd = t('shell:signin.tagline_b');
  const taglineLast = t('shell:signin.tagline_c');
  const promise = t('shell:signin.promise');

  return (
    <header className="flex flex-col items-center text-center" data-testid="signin-brand">
      <div className="flex items-center gap-3" data-testid="signin-logo">
        <LogoMark size={36} intro />
        <span className="font-sans text-[21px] font-strong tracking-tight text-tx-0">
          {t('common:app_name')}
        </span>
      </div>

      <p
        className="ms-auth-rise mt-8 text-balance font-sans text-[26px] font-display leading-[34px] tracking-[-0.01em] text-tx-0 sm:text-[30px] sm:leading-[39px]"
        style={{ '--i': 0 } as React.CSSProperties}
      >
        <span className={cn('block', endMark(taglineEnd))}>
          <span>{t('shell:signin.tagline_a')}</span>
          <span className="ms-auth-gap">{taglineEnd}</span>
        </span>
        <span className={cn('ms-auth-gradient inline-block', endMark(taglineLast))}>{taglineLast}</span>
      </p>

      <p
        className={cn('ms-auth-rise mt-3 font-sans text-[13px] leading-[21px] text-tx-2', endMark(promise))}
        style={{ '--i': 1 } as React.CSSProperties}
      >
        {promise}
      </p>
    </header>
  );
}
