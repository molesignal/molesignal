import { Upload } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { FlameArt } from '@/shell/firstRun/art';
import { FirstRun } from '@/shell/firstRun/FirstRun';

/** The profiling page before any profile has arrived: what it will show and the two ways to send one. */
export function ProfileActivation({
  onUpload,
}: {
  /** Opens the upload dialog; absent when the user may not write profiles. */
  onUpload?: (() => void) | undefined;
}) {
  const { t } = useTranslation('profiles');

  return (
    <FirstRun
      data-testid="profiles-activation"
      eyebrow={t('first_run.eyebrow')}
      title={t('list.empty_title')}
      description={t('list.empty_description')}
      illustration={<FlameArt />}
      tone="var(--orange)"
      actions={
        <>
          <Link
            to="/datasource/recommended/continuous-profiling"
            className="ms-press inline-flex h-9 items-center rounded-md bg-indigo px-3 font-sans text-sm font-bold text-white shadow-primary hover:brightness-90 focus-visible:brightness-90"
          >
            {t('upload.datasource_link')}
          </Link>
          {onUpload && (
            <button
              type="button"
              onClick={onUpload}
              className="ms-press inline-flex h-9 items-center gap-2 rounded-md bg-bg-2 px-3 font-sans text-sm font-strong text-tx-1 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0"
            >
              <Upload className="h-3.5 w-3.5" aria-hidden="true" />
              {t('upload.button')}
            </button>
          )}
        </>
      }
      steps={(['send', 'narrow', 'read'] as const).map((step) => ({
        title: t(`first_run.steps.${step}.title`),
        description: t(`first_run.steps.${step}.description`),
      }))}
    />
  );
}
