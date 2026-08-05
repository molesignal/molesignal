import { ChevronDown, ChevronUp } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { IconButton } from '@/shell/chrome';
import { CopyIconButton } from '@/shell/CopyIconButton';
import { cn } from '@/shell/lib/cn';
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/shell/ui/tooltip';

import type {
  HighlightedLine,
  HighlightTokenKind,
} from './highlighter';

export interface MarkdownCodeBlockProps {
  language: string;
  content: string;
  className?: string;
}

const LANGUAGE_LABEL: Record<string, string> = {
  bash: 'Shell',
  dart: 'Dart',
  go: 'Go',
  ini: 'INI',
  json: 'JSON',
  kotlin: 'Kotlin',
  powershell: 'PowerShell',
  python: 'Python',
  rust: 'Rust',
  sql: 'SQL',
  swift: 'Swift',
  text: 'Plain text',
  toml: 'TOML',
  ts: 'TypeScript',
  yaml: 'YAML',
};

const TOKEN_CLASS: Record<HighlightTokenKind, string> = {
  plain: 'text-tx-1',
  comment: 'italic text-tx-3',
  keyword: 'font-semibold text-purple',
  string: 'text-green',
  number: 'text-orange',
  type: 'text-yellow',
  function: 'text-blue',
  property: 'text-blue',
  operator: 'text-indigo-soft',
  punctuation: 'text-tx-2',
  identifier: 'text-tx-0',
  invalid: 'text-red',
};

interface HighlighterModule {
  highlightCode: (
    content: string,
    language: string,
  ) => Promise<HighlightedLine[]>;
}

let highlighterPromise: Promise<HighlighterModule> | undefined;

function loadHighlighter(): Promise<HighlighterModule> {
  highlighterPromise ??= import('./highlighter');
  return highlighterPromise;
}

export function MarkdownCodeBlock({
  language,
  content,
  className,
}: MarkdownCodeBlockProps) {
  const { t } = useTranslation('onboarding');
  const [copied, setCopied] = React.useState(false);
  const [expanded, setExpanded] = React.useState(true);
  const [highlightedLines, setHighlightedLines] = React.useState<
    HighlightedLine[] | null
  >(null);
  const codeId = React.useId();
  const resetTimer = React.useRef<number | undefined>(undefined);
  const normalizedLanguage = language.trim().toLocaleLowerCase() || 'text';
  const languageLabel = LANGUAGE_LABEL[normalizedLanguage] ?? language;
  const toggleLabel = expanded
    ? t('datasource_page.collapse_code')
    : t('datasource_page.expand_code');

  React.useEffect(() => {
    let active = true;
    setHighlightedLines(null);
    void loadHighlighter()
      .then((module) => module.highlightCode(content, normalizedLanguage))
      .then((lines) => {
        if (active) setHighlightedLines(lines);
      })
      .catch(() => {
        if (active) setHighlightedLines(null);
      });
    return () => {
      active = false;
    };
  }, [content, normalizedLanguage]);

  React.useEffect(
    () => () => window.clearTimeout(resetTimer.current),
    [],
  );

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(content);
      setCopied(true);
      window.clearTimeout(resetTimer.current);
      resetTimer.current = window.setTimeout(() => setCopied(false), 1500);
    } catch {
      // Clipboard access can be denied by the browser.
    }
  };

  return (
    <figure
      className={cn(
        'min-w-0 overflow-hidden rounded-md border border-bd-0 bg-bg-1',
        className,
      )}
    >
      <figcaption className="flex h-9 min-w-0 items-center border-b border-bd-0 bg-bg-1 px-3">
        <span className="mr-2 h-1.5 w-1.5 shrink-0 rounded-full bg-indigo" />
        <span className="type-micro truncate font-mono font-semibold tracking-[-0.01em] text-tx-2">
          {languageLabel}
        </span>
        <div className="ml-auto flex shrink-0 items-center gap-0.5">
          <CopyIconButton
            onClick={copy}
            label={t('datasource_page.copy')}
            copied={copied}
            copiedLabel={t('datasource_page.copied')}
          />
          <Tooltip>
            <TooltipTrigger asChild>
              <IconButton
                aria-label={toggleLabel}
                aria-controls={codeId}
                aria-expanded={expanded}
                onClick={() => setExpanded((current) => !current)}
              >
                {expanded ? (
                  <ChevronUp aria-hidden="true" className="h-3.5 w-3.5" />
                ) : (
                  <ChevronDown aria-hidden="true" className="h-3.5 w-3.5" />
                )}
              </IconButton>
            </TooltipTrigger>
            <TooltipContent side="top">{toggleLabel}</TooltipContent>
          </Tooltip>
        </div>
      </figcaption>
      <pre
        id={codeId}
        hidden={!expanded}
        className="m-0 overflow-x-auto bg-bg-0 p-4 font-mono text-xs font-medium leading-[1.7] text-tx-1 [font-family:ui-monospace,SFMono-Regular,Menlo,Monaco,Consolas,monospace] [tab-size:2]"
      >
        <code
          aria-label={`${languageLabel} code`}
          className={`language-${normalizedLanguage}`}
          data-highlighted={highlightedLines ? 'true' : 'false'}
        >
          {highlightedLines
            ? renderHighlightedLines(highlightedLines)
            : content}
        </code>
      </pre>
    </figure>
  );
}

function renderHighlightedLines(lines: HighlightedLine[]): React.ReactNode {
  return lines.map((line, lineIndex) => (
    <React.Fragment key={lineIndex}>
      {line.map((token, tokenIndex) => (
        <span
          key={`${lineIndex}-${tokenIndex}`}
          className={TOKEN_CLASS[token.kind]}
          data-token-kind={token.kind}
        >
          {token.text}
        </span>
      ))}
      {lineIndex < lines.length - 1 ? '\n' : null}
    </React.Fragment>
  ));
}
