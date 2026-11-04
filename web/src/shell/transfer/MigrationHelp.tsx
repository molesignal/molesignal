import { ChevronRight } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { writeClipboardText } from '@/lib/clipboard';
import { CopyIconButton } from '@/shell/CopyIconButton';
import { ViewTabs } from '@/shell/tabs/ViewTabs';

import type { MigrationSource } from './scheduleSources';

/**
 * How to bring things over from another tool: a script to run wherever its API key may be, whose
 * file then goes through the import above. The server never sees the key or calls that tool.
 */
export function MigrationHelp({ sources, texts }: { sources: readonly MigrationSource[]; texts: string }) {
  const { t, i18n } = useTranslation('alerts');
  const { t: tc } = useTranslation('common');
  const [key, setKey] = React.useState(sources[0]?.key ?? '');
  const [copied, setCopied] = React.useState(false);
  const resetTimer = React.useRef<number | undefined>(undefined);
  const source = sources.find((item) => item.key === key) ?? sources[0];

  React.useEffect(() => () => window.clearTimeout(resetTimer.current), []);

  if (!source) return null;

  const copy = async () => {
    try {
      await writeClipboardText(source.script);
      setCopied(true);
      window.clearTimeout(resetTimer.current);
      resetTimer.current = window.setTimeout(() => setCopied(false), 1500);
    } catch {
      // Clipboard access can be denied by the browser.
    }
  };

  const language = i18n.language?.toLowerCase().startsWith('zh') ? 'zh-Hans' : 'en-US';

  return (
    <details data-import-migration className="group font-sans text-sm">
      <summary className="flex w-fit cursor-pointer list-none items-center gap-1.5 font-strong text-tx-1 hover:text-indigo-soft focus-visible:text-indigo-soft [&::-webkit-details-marker]:hidden">
        <ChevronRight className="h-3.5 w-3.5 transition-transform group-open:rotate-90" />
        {t(`${texts}.migration_title`)}
      </summary>
      <div className="mt-3 flex flex-col gap-3">
        <p className="text-xs text-tx-2">{t(`${texts}.migration_intro`)}</p>
        <ViewTabs
          label={t(`${texts}.migration_title`)}
          value={source.key}
          onValueChange={setKey}
          items={sources.map((item) => ({ key: item.key, label: item.label }))}
        />
        <p className="text-xs text-tx-2">
          {t(`${texts}.migration_key`, { name: `${source.key.toUpperCase()}_API_KEY` })}
        </p>
        <div className="relative">
          <pre
            data-import-script={source.key}
            className="max-h-72 overflow-auto rounded-md bg-bg-2 p-3 pr-10 font-mono text-xs text-tx-1"
          >
            {source.script}
          </pre>
          <CopyIconButton
            wrapperClassName="absolute right-1 top-1"
            onClick={() => void copy()}
            label={t(`${texts}.migration_copy`)}
            copied={copied}
            copiedLabel={tc('actions.copied')}
          />
        </div>
        <a
          href={`https://docs.molesignal.com/${language}/${source.docsPath}`}
          target="_blank"
          rel="noopener noreferrer"
          className="w-fit text-xs text-tx-1 underline hover:text-indigo-soft focus-visible:text-indigo-soft"
        >
          {t(`${texts}.migration_docs`)}
        </a>
      </div>
    </details>
  );
}
