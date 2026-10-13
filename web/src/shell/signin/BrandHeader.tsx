import type * as React from 'react';
import { useTranslation } from 'react-i18next';

import { LogoMark } from '@/shell/LogoMark';

/**
 * Who this is, in one sentence. The mark draws its pulse once as the page
 * opens; the statement and the promise rise in behind it. Nothing else belongs
 * above a sign-in form.
 */
export function BrandHeader() {
  const { t } = useTranslation(['shell', 'common']);

  return (
    <header className="flex flex-col items-center text-center" data-testid="signin-brand">
      <div className="flex items-center gap-3" data-testid="signin-logo">
        <LogoMark size={36} intro />
        <span className="font-sans text-[21px] font-strong tracking-tight text-tx-0">
          {t('common:app_name')}
        </span>
      </div>

      <p
        className="ms-auth-rise mt-8 text-balance font-sans text-[26px] font-display leading-[1.3] tracking-[-0.01em] text-tx-0 sm:text-[30px]"
        style={{ '--i': 0 } as React.CSSProperties}
      >
        <span className="block">
          <span>{t('shell:signin.tagline_a')}</span>
          <span className="ms-auth-gap">{t('shell:signin.tagline_b')}</span>
        </span>
        <span className="ms-auth-gradient inline-block">{t('shell:signin.tagline_c')}</span>
      </p>

      <p
        className="ms-auth-rise mt-3 font-sans text-[13px] leading-relaxed text-tx-2"
        style={{ '--i': 1 } as React.CSSProperties}
      >
        {t('shell:signin.promise')}
      </p>
    </header>
  );
}
