import {
  BookOpen,
  Code2,
  EyeOff,
  SlidersHorizontal,
  Video,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useParams } from 'react-router-dom';

import { rumDocumentationUrl } from '../documentation';
import { RumListPage, RumSectionHeader } from '../RumLayout';

const SECTION_ICONS = {
  sdk: Code2,
  sampling: SlidersHorizontal,
  privacy: EyeOff,
  'session-replay': Video,
} as const;

type GuideSection = keyof typeof SECTION_ICONS;

export function RumSettingsGuide() {
  const { t, i18n } = useTranslation('rum');
  const { section = 'sdk' } = useParams();
  const key: GuideSection =
    section === 'sampling' ||
    section === 'privacy' ||
    section === 'session-replay'
      ? section
      : 'sdk';
  const Icon = SECTION_ICONS[key];
  const docsHref = rumDocumentationUrl(
    i18n.resolvedLanguage ?? i18n.language,
    key,
  );

  return (
    <RumListPage
      title={t(`settings.sections.${key}.title`)}
      subtitle={t(`settings.sections.${key}.subtitle`)}
      settings
    >
      <section className="mx-auto w-full max-w-4xl">
        <RumSectionHeader
          title={t(`settings.sections.${key}.heading`)}
          description={t(`settings.sections.${key}.description`)}
        />
        <div className="grid gap-6 py-8 md:grid-cols-[minmax(0,1fr)_280px]">
          <div>
            <span className="grid h-12 w-12 place-items-center rounded-lg bg-indigo-dim text-indigo-soft">
              <Icon aria-hidden className="h-6 w-6" />
            </span>
            <ol className="mt-6 grid gap-3">
              {[1, 2, 3].map((step) => (
                <li
                  key={step}
                  className="flex items-start gap-3 rounded-lg border border-bd-0 bg-bg-1 p-4"
                >
                  <span className="grid h-6 w-6 shrink-0 place-items-center rounded-full bg-bg-3 font-mono text-xs font-strong text-tx-1">
                    {step}
                  </span>
                  <span className="pt-0.5 text-sm text-tx-1">
                    {t(`settings.sections.${key}.steps.${step}`)}
                  </span>
                </li>
              ))}
            </ol>
          </div>
          <aside className="rounded-lg border border-bd-0 bg-bg-1 p-5">
            <BookOpen aria-hidden className="h-5 w-5 text-blue-soft" />
            <h2 className="mt-4 text-sm font-display-strong text-tx-0">
              {t('settings.docs_title')}
            </h2>
            <p className="mt-2 text-xs leading-relaxed text-tx-2">
              {t('settings.docs_description')}
            </p>
            <a
              href={docsHref}
              target="_blank"
              rel="noopener noreferrer"
              className="mt-4 inline-flex h-8 items-center rounded-md bg-indigo px-3 text-xs font-strong text-white hover:bg-indigo-soft focus-visible:bg-indigo-soft"
            >
              {t('settings.view_docs')}
            </a>
          </aside>
        </div>
      </section>
    </RumListPage>
  );
}
