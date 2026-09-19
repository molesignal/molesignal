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
 * Shared placeholder for routes whose backend integration is pending. Each
 * page renders the final PageHeader and PageBody chrome so the Topbar and
 * Sidebar remain exercised end-to-end. The shared `backend-pending`
 * EmptyState strategy keeps placeholder copy and layout consistent.
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
                  defaultValue: 'This view is coming soon.',
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
