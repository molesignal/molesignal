import { ArrowRight, Code2, Smartphone } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { VitalsArt } from '@/shell/firstRun/art';
import { FirstRun } from '@/shell/firstRun/FirstRun';

import { rumDocumentationUrl } from '../documentation';

const PLATFORMS = [
  { key: 'web', icon: Code2, to: '/datasource/recommended/rum' },
  { key: 'flutter', icon: Smartphone, to: '/datasource/recommended/rum-flutter' },
  { key: 'android', icon: Smartphone, to: '/datasource/recommended/rum-android' },
  { key: 'ios', icon: Smartphone, to: '/datasource/recommended/rum-ios' },
] as const;

const STEPS = ['1', '2', '3', '4'] as const;

/** The RUM overview before any session has arrived: pick a platform, install, send a test event. */
export function RumOnboarding() {
  const { t, i18n } = useTranslation('rum');
  const docsHref = rumDocumentationUrl(
    i18n.resolvedLanguage ?? i18n.language,
    'sdk',
  );

  return (
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
          <span className="mr-1 font-sans text-xs text-tx-3">
            {t('onboarding.already_installed')}
          </span>
          <Link
            to="/datasource/recommended/rum?test=1"
            className="ms-press inline-flex h-9 items-center rounded-md bg-indigo px-3 font-sans text-sm font-bold text-white shadow-primary hover:brightness-90 focus-visible:brightness-90"
          >
            {t('onboarding.send_test')}
          </Link>
          <a
            href={docsHref}
            target="_blank"
            rel="noopener noreferrer"
            className="ms-press inline-flex h-9 items-center rounded-md bg-bg-2 px-3 font-sans text-sm font-strong text-tx-1 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0"
          >
            {t('onboarding.view_docs')}
          </a>
        </>
      }
    >
      {/* Where the application runs decides which SDK to install. */}
      <div className="mt-[20px] grid gap-[8px] sm:grid-cols-2">
        {PLATFORMS.map(({ key, icon: Icon, to }) => (
          <Link
            key={key}
            to={to}
            className="group flex min-h-[48px] items-center gap-3 rounded-md bg-[var(--control-surface)] px-4 text-left transition-colors hover:bg-bg-3 focus-visible:bg-bg-3"
          >
            <Icon aria-hidden className="h-4 w-4 shrink-0 text-indigo-soft" />
            <span className="font-sans text-sm font-strong text-tx-0">
              {t(`onboarding.platforms.${key}`)}
            </span>
            <ArrowRight
              aria-hidden
              className="ml-auto h-4 w-4 text-tx-3 transition-transform group-hover:translate-x-0.5"
            />
          </Link>
        ))}
      </div>
    </FirstRun>
  );
}
