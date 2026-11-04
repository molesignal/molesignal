import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { VitalsArt } from '@/shell/firstRun/art';
import { FirstRun } from '@/shell/firstRun/FirstRun';

import { rumDocumentationUrl } from '../documentation';

const STEPS = ['1', '2', '3', '4'] as const;

/**
 * The explorer before the first application exists: there is nothing to look at until an SDK
 * has a client token to send with, so the page says how to get one.
 */
export function RumFirstRun({ embedded = false }: { embedded?: boolean }) {
  const { t, i18n } = useTranslation('rum');
  const navigate = useNavigate();
  const create = useActionAccess({ permission: 'rum.applications.manage' });

  return (
    // The explorer gives it the room under its tabs; a page that has its own body does not need it.
    <div className={embedded ? undefined : 'h-full min-h-0 overflow-y-auto pl-[var(--pane-gutter-start,20px)] pr-[var(--pane-gutter-end,20px)] pt-[12px]'}>
      <FirstRun
        data-testid="rum-activation"
        eyebrow={t('onboarding.eyebrow')}
        title={t('onboarding.title')}
        description={t('onboarding.description')}
        illustration={<VitalsArt />}
        tone="var(--green)"
        steps={STEPS.map((step) => ({ title: t(`onboarding.steps.${step}`) }))}
        actions={
          <>
            <ChromeButton
              variant="primary"
              disabled={create.disabled}
              disabledReason={create.reason}
              onClick={() => navigate('/rum/applications?create=1')}
            >
              {t('onboarding.create')}
            </ChromeButton>
            <a
              href={rumDocumentationUrl(i18n.resolvedLanguage ?? i18n.language, 'sdk')}
              target="_blank"
              rel="noopener noreferrer"
              className="ms-press inline-flex h-9 items-center rounded-md bg-bg-2 px-3 font-sans text-sm font-strong text-tx-1 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0"
            >
              {t('onboarding.view_docs')}
            </a>
          </>
        }
      />
    </div>
  );
}
