import { ChevronDown, ChevronUp, Eraser } from 'lucide-react';

import { CodeEditor, type CodeEditorProps } from '@/shell/codeEditor';
import { codeLanguageLabel } from '@/shell/codeEditor/Chrome';
import { cn } from '@/shell/lib/cn';
import { Button } from '@/shell/ui/button';

interface QueryEditorFrameProps extends CodeEditorProps {
  queryRef?: string;
  frameClassName?: string;
  editorClassName?: string;
  collapsed?: boolean;
  onCollapsedChange?: ((collapsed: boolean) => void) | undefined;
  collapseLabel?: string | undefined;
  expandLabel?: string | undefined;
  onClear?: (() => void) | undefined;
  clearLabel?: string | undefined;
  summary?: string | undefined;
}

export function QueryEditorFrame({
  queryRef = 'A',
  frameClassName,
  editorClassName,
  collapsed = false,
  onCollapsedChange,
  collapseLabel = 'Collapse query editor',
  expandLabel = 'Expand query editor',
  onClear,
  clearLabel = 'Clear query',
  summary,
  fontSize = 13,
  lineHeight = 20,
  maxHeight = 360,
  minHeight = 160,
  showHeader = true,
  showStatus = true,
  language = 'text',
  label,
  readOnly = false,
  onModEnter,
  ...editorProps
}: QueryEditorFrameProps) {
  const displayLabel = label ?? codeLanguageLabel(language);
  const languageLabel = codeLanguageLabel(language);
  const showLanguage = displayLabel.toLocaleLowerCase() !== languageLabel.toLocaleLowerCase();
  const compactSummary = summary?.trim() || editorProps.placeholder || displayLabel;
  const showClear = !readOnly && Boolean(onClear);
  const clearDisabled = editorProps.value.trim().length === 0;

  if (collapsed && onCollapsedChange) {
    return (
      <div
        data-query-editor-state="collapsed"
        className={cn(
          'code-editor-shell overflow-hidden rounded-lg border border-bd-1 bg-bg-1 shadow-sm',
          frameClassName,
        )}
      >
        <button
          type="button"
          aria-expanded={false}
          aria-label={expandLabel}
          onClick={() => onCollapsedChange(false)}
          className="flex h-11 w-full min-w-0 items-center gap-2 px-3 text-left transition-colors hover:bg-bg-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-indigo"
        >
          <span className="type-micro grid h-5 min-w-5 shrink-0 place-items-center rounded border border-bd-1 bg-bg-2 px-1 font-mono font-bold text-tx-1">
            {queryRef}
          </span>
          <span className="shrink-0 font-sans text-xs font-semibold text-tx-0">{displayLabel}</span>
          <code className="min-w-0 flex-1 truncate font-mono text-xs text-tx-2">{compactSummary}</code>
          <span className="ml-2 hidden shrink-0 items-center gap-1 font-sans text-xs font-semibold text-indigo-soft sm:inline-flex">
            {expandLabel}
            <ChevronDown className="h-3.5 w-3.5" />
          </span>
        </button>
      </div>
    );
  }

  return (
    <div
      data-query-editor-state="expanded"
      className={cn(
        'code-editor-shell relative overflow-hidden rounded-lg border border-bd-1 bg-bg-1 shadow-sm',
        frameClassName,
      )}
    >
      {showHeader ? (
        <div className="flex h-9 min-w-0 items-center border-b border-bd-0 bg-bg-1">
          <div className="flex h-full min-w-0 items-center gap-2 border-b-2 border-indigo px-3">
            <span className="type-micro grid h-5 min-w-5 shrink-0 place-items-center rounded border border-bd-1 bg-bg-2 px-1 font-mono font-bold text-tx-1">
              {queryRef}
            </span>
            <span className="truncate font-sans text-xs font-semibold text-tx-0">{displayLabel}</span>
          </div>
          {showLanguage ? (
            <span className="type-micro ml-2 hidden shrink-0 rounded border border-bd-0 bg-bg-2 px-1.5 py-0.5 font-mono font-semibold uppercase tracking-[0.08em] text-tx-3 sm:inline">
              {languageLabel}
            </span>
          ) : null}
          {showClear || (!readOnly && onModEnter) || onCollapsedChange ? (
            <div className="ml-auto flex shrink-0 items-center gap-1 px-1">
              {showClear ? (
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  aria-label={clearLabel}
                  disabled={clearDisabled}
                  onClick={onClear}
                  className="h-7 gap-1 rounded px-2 font-sans text-xs font-semibold text-tx-2 hover:bg-bg-2 hover:text-tx-0 disabled:text-tx-4 disabled:opacity-100 [&_svg]:size-3.5"
                >
                  <Eraser aria-hidden="true" />
                  {clearLabel}
                </Button>
              ) : null}
              {!readOnly && onModEnter ? (
                <span className="type-micro hidden shrink-0 px-2 font-mono text-tx-3 sm:inline">
                  Cmd/Ctrl + Enter
                </span>
              ) : null}
              {onCollapsedChange ? (
                <button
                  type="button"
                  aria-expanded
                  aria-label={collapseLabel}
                  title={collapseLabel}
                  onClick={() => onCollapsedChange(true)}
                  className="inline-flex h-7 shrink-0 items-center gap-1 rounded px-2 font-sans text-xs font-semibold text-tx-2 hover:bg-bg-2 hover:text-tx-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo"
                >
                  <span className="hidden xl:inline">{collapseLabel}</span>
                  <ChevronUp className="h-3.5 w-3.5" />
                </button>
              ) : null}
            </div>
          ) : null}
        </div>
      ) : (
        <span className="sr-only">Query {queryRef}</span>
      )}
      <CodeEditor
        {...editorProps}
        language={language}
        readOnly={readOnly}
        fontSize={fontSize}
        lineHeight={lineHeight}
        maxHeight={maxHeight}
        minHeight={minHeight}
        showHeader={false}
        showStatus={showStatus}
        highlightCurrentLine={editorProps.highlightCurrentLine ?? true}
        className={cn(
          'code-editor-embedded rounded-none border-0 bg-transparent shadow-none',
          editorClassName,
        )}
        {...(label !== undefined ? { label } : {})}
        {...(onModEnter !== undefined ? { onModEnter } : {})}
      />
    </div>
  );
}
