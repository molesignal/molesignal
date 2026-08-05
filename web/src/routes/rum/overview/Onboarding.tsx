import { ArrowRight, Check, Code2, Smartphone } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { rumDocumentationUrl } from '../documentation';

const PLATFORMS = [
  { key: 'web', icon: Code2, to: '/datasource/recommended/rum' },
  { key: 'flutter', icon: Smartphone, to: '/datasource/recommended/rum-flutter' },
  { key: 'android', icon: Smartphone, to: '/datasource/recommended/rum-android' },
  { key: 'ios', icon: Smartphone, to: '/datasource/recommended/rum-ios' },
] as const;

export function RumOnboarding() {
  const { t, i18n } = useTranslation('rum');
  const docsHref = rumDocumentationUrl(
    i18n.resolvedLanguage ?? i18n.language,
    'sdk',
  );
  return (
    <section
      data-testid="rum-activation"
      className="overflow-hidden rounded-xl border border-bd-0 bg-bg-1"
    >
      <div className="grid gap-8 px-6 py-8 xl:grid-cols-[minmax(0,1fr)_420px] xl:px-8">
        <div>
          <span className="inline-flex items-center rounded-full bg-indigo-dim px-2.5 py-1 text-xs font-strong text-indigo-soft">
            {t('onboarding.eyebrow')}
          </span>
          <h2 className="mt-4 text-2xl font-display-strong tracking-[-0.025em] text-tx-0">
            {t('onboarding.title')}
          </h2>
          <p className="mt-3 max-w-2xl text-sm leading-relaxed text-tx-2">
            {t('onboarding.description')}
          </p>
          <div className="mt-6 grid gap-3 sm:grid-cols-2">
            {PLATFORMS.map(({ key, icon: Icon, to }) => (
              <Link
                key={key}
                to={to}
                className="group flex min-h-20 items-center gap-3 rounded-lg border border-bd-0 bg-bg-2 px-4 text-left transition-colors hover:bg-bg-3 focus-visible:bg-bg-3"
              >
                <Icon aria-hidden className="h-5 w-5 text-indigo-soft" />
                <span className="text-sm font-strong text-tx-0">
                  {t(`onboarding.platforms.${key}`)}
                </span>
                <ArrowRight
                  aria-hidden
                  className="ml-auto h-4 w-4 text-tx-3 transition-transform group-hover:translate-x-0.5"
                />
              </Link>
            ))}
          </div>
          <div className="mt-6 flex flex-wrap items-center gap-3">
            <span className="text-xs text-tx-3">
              {t('onboarding.already_installed')}
            </span>
            <Link
              to="/datasource/recommended/rum?test=1"
              className="inline-flex h-8 items-center rounded-md bg-indigo px-3 text-xs font-strong text-white hover:bg-indigo-soft focus-visible:bg-indigo-soft"
            >
              {t('onboarding.send_test')}
            </Link>
            <a
              href={docsHref}
              target="_blank"
              rel="noopener noreferrer"
              className="inline-flex h-8 items-center rounded-md border border-bd-1 bg-bg-2 px-3 text-xs font-strong text-tx-1 hover:bg-bg-3 focus-visible:bg-bg-3"
            >
              {t('onboarding.view_docs')}
            </a>
          </div>
        </div>
        <div className="rounded-lg border border-bd-0 bg-bg-0 p-5">
          <div className="text-xs font-strong uppercase tracking-wide text-tx-3">
            {t('onboarding.progress_title')}
          </div>
          <ol className="mt-5 grid gap-4">
            {[1, 2, 3, 4].map((step) => (
              <li key={step} className="flex items-center gap-3">
                <span className="grid h-7 w-7 shrink-0 place-items-center rounded-full border border-bd-1 bg-bg-2 font-mono text-xs font-strong text-tx-1">
                  {step}
                </span>
                <span className="text-sm text-tx-1">
                  {t(`onboarding.steps.${step}`)}
                </span>
                {step === 1 && (
                  <Check aria-hidden className="ml-auto h-4 w-4 text-tx-3" />
                )}
              </li>
            ))}
          </ol>
        </div>
      </div>
    </section>
  );
}
