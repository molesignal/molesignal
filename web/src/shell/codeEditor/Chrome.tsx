import { cn } from '@/shell/lib/cn';

import type { CodeLanguage } from './types';

const LANGUAGE_ACCENT: Record<CodeLanguage, string> = {
  sql: 'bg-blue',
  promql: 'bg-purple',
  json: 'bg-orange',
  yaml: 'bg-green',
  vrl: 'bg-yellow',
  javascript: 'bg-blue',
  'field-query': 'bg-indigo',
  template: 'bg-purple',
  text: 'bg-tx-3',
};

const LANGUAGE_LABEL: Record<CodeLanguage, string> = {
  sql: 'SQL',
  promql: 'PromQL',
  json: 'JSON',
  yaml: 'YAML',
  vrl: 'VRL',
  javascript: 'JavaScript',
  'field-query': 'Fields',
  template: 'Template',
  text: 'Plain text',
};

export function codeLanguageLabel(language: CodeLanguage): string {
  return LANGUAGE_LABEL[language];
}

export function CodeEditorHeader({
  language,
  label,
  readOnly = false,
  canRun = false,
  canSave = false,
}: {
  language: CodeLanguage;
  label?: string;
  readOnly?: boolean;
  canRun?: boolean;
  canSave?: boolean;
}) {
  const displayLabel = label ?? codeLanguageLabel(language);
  const languageLabel = codeLanguageLabel(language);
  const showLanguage = displayLabel.toLocaleLowerCase() !== languageLabel.toLocaleLowerCase();

  return (
    <div className="flex h-8 min-w-0 items-center border-b border-bd-0 bg-bg-1 px-3">
      <span className={cn('mr-2 h-1.5 w-1.5 shrink-0 rounded-full', LANGUAGE_ACCENT[language])} />
      <span className="type-micro truncate font-mono font-semibold tracking-[-0.01em] text-tx-1">
        {displayLabel}
      </span>
      {showLanguage ? (
        <span className="type-micro ml-2 shrink-0 rounded border border-bd-0 bg-bg-2 px-1.5 py-0.5 font-mono font-semibold uppercase tracking-[0.08em] text-tx-3">
          {languageLabel}
        </span>
      ) : null}
      <span className="type-micro ml-auto shrink-0 pl-3 font-mono text-tx-3">
        {readOnly ? 'READ ONLY' : canSave ? '⌘ S' : canRun ? '⌘ ↵' : null}
      </span>
    </div>
  );
}

export function CodeEditorStatus({
  language,
  lineCount,
  line = 1,
  column = 1,
}: {
  language: CodeLanguage;
  lineCount: number;
  line?: number;
  column?: number;
}) {
  return (
    <div className="type-micro flex h-6 min-w-0 items-center gap-3 overflow-hidden border-t border-bd-0 bg-bg-1 px-3 font-sans font-medium tracking-normal text-tx-2">
      <span className="inline-flex shrink-0 items-center gap-1.5 text-tx-1">
        <span className={cn('h-1.5 w-1.5 rounded-full', LANGUAGE_ACCENT[language])} />
        {codeLanguageLabel(language)}
      </span>
      <span className="shrink-0 tabular-nums">
        {lineCount} {lineCount === 1 ? 'line' : 'lines'}
      </span>
      <span className="hidden shrink-0 text-tx-3 sm:inline">Spaces: 2</span>
      <span className="ml-auto shrink-0 font-mono tabular-nums text-tx-2">
        Ln {line}, Col {column}
      </span>
    </div>
  );
}
