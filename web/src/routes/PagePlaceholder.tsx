import { Construction } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { EmptyState } from '@/shell/EmptyState';
import { PageBody, PageHeader } from '@/shell/PageHeader';
import { Kbd } from '@/shell/ui/kbd';

interface PagePlaceholderProps {
  title: string;
  subtitle?: string | undefined;
  hint?: React.ReactNode | undefined;
}

/**
 * Shared placeholder used by Phase-1 route stubs. Each page exposes the
 * final chrome (PageHeader + PageBody) so the Topbar / Sidebar
 * are exercised end-to-end, while the body content lands later with the
 * feature module that owns it.
 *
 * Phase 6 M2: routes through the shared EmptyState component for the
 * `backend-pending` strategy so the placeholder shares one grammar with
 * every other "this page exists but isn't fully wired" surface
 * (RUM / Quota / AI Toolsets etc.).
 */
export function PagePlaceholder({ title, subtitle, hint }: PagePlaceholderProps) {
  const { t } = useTranslation('shell');
  return (
    <>
      <PageHeader title={title} subtitle={subtitle} />
      <PageBody>
        <EmptyState
          strategy="backend-pending"
          icon={Construction}
          title={title}
          description={
            typeof hint === 'string'
              ? hint
              : t('placeholder.default_hint', {
                  defaultValue: 'This view is a Phase-1 placeholder. The feature module that owns it lands later.',
                })
          }
        />
        {React.isValidElement(hint) && (
          <div className="mx-auto mt-2 max-w-md text-center font-sans text-xs text-tx-3">{hint}</div>
        )}
        <div className="mx-auto mt-3 max-w-md text-center font-sans text-xs text-tx-3">
          {t('placeholder.palette_hint_prefix', { defaultValue: 'Press' })} <Kbd>⌘K</Kbd>{' '}
          {t('placeholder.palette_hint_suffix', { defaultValue: 'to navigate to another route.' })}
        </div>
      </PageBody>
    </>
  );
}
