import * as React from 'react';

import { cn } from '@/shell/lib/cn';

import { CodeEditorHeader, CodeEditorStatus } from './Chrome';
import type { CodeEditorHandle, CodeEditorProps, CodeLanguage } from './types';
import {
  CODE_EDITOR_FONT_FAMILY,
  CODE_EDITOR_FONT_SIZE,
  CODE_EDITOR_FONT_WEIGHT,
  CODE_EDITOR_LINE_HEIGHT,
} from './typography';

export type {
  CodeEditorHandle,
  CodeEditorMarker,
  CodeEditorProps,
  CodeLanguage,
} from './types';

const MonacoCodeEditor = React.lazy(() =>
  import('./Monaco').then((module) => ({ default: module.MonacoCodeEditor })),
);

const FALLBACK_FRAME_HORIZONTAL_PADDING = 4;
const FALLBACK_LEFT_INSET = 14;
const FALLBACK_LEFT_INSET_WITH_LINE_NUMBERS = 38;

export const CodeEditor = React.forwardRef<CodeEditorHandle, CodeEditorProps>(
  function CodeEditor(props, ref) {
    return (
      <React.Suspense fallback={<CodeEditorFallback {...props} />}>
        <MonacoCodeEditor ref={ref} {...props} />
      </React.Suspense>
    );
  },
);

function CodeEditorFallback({
  value,
  language = 'text',
  label,
  placeholder,
  minHeight = 96,
  fontSize,
  fontWeight = CODE_EDITOR_FONT_WEIGHT,
  lineHeight,
  lineNumbers = true,
  readOnly = false,
  onModEnter,
  onModSave,
  compact = false,
  showHeader = true,
  showStatus = true,
  className,
}: CodeEditorProps) {
  const effectivePlaceholder = placeholder ?? (!readOnly ? defaultPlaceholder(language) : undefined);
  const metrics = editorMetrics(compact, fontSize, lineHeight);

  return (
    <div
      className={cn(
        'code-editor-shell overflow-hidden rounded-md border border-bd-1 bg-bg-1 shadow-[inset_0_1px_0_rgba(255,255,255,0.02)]',
        readOnly && 'bg-bg-2',
        className,
      )}
    >
      {showHeader && (
        <CodeEditorHeader
          language={language}
          readOnly={readOnly}
          canRun={Boolean(onModEnter)}
          canSave={Boolean(onModSave)}
          {...(label !== undefined ? { label } : {})}
        />
      )}
      <div
        className="flex items-start bg-bg-0 font-normal tracking-normal text-tx-3"
        style={{
          fontFamily: CODE_EDITOR_FONT_FAMILY,
          height: minHeight,
          fontSize: metrics.fontSize,
          fontWeight,
          lineHeight: `${metrics.lineHeight}px`,
          paddingBottom: metrics.paddingBottom,
          paddingLeft: (lineNumbers ? FALLBACK_LEFT_INSET_WITH_LINE_NUMBERS : FALLBACK_LEFT_INSET) + FALLBACK_FRAME_HORIZONTAL_PADDING,
          paddingRight: FALLBACK_LEFT_INSET + FALLBACK_FRAME_HORIZONTAL_PADDING,
          paddingTop: metrics.paddingTop,
        }}
      >
        {effectivePlaceholder}
      </div>
      {showStatus && (
        <CodeEditorStatus
          language={language}
          lineCount={Math.max(1, value.split('\n').length)}
        />
      )}
    </div>
  );
}

function defaultPlaceholder(language: CodeLanguage): string | undefined {
  if (language === 'sql') return 'SELECT * FROM "stream"\nORDER BY _timestamp DESC\nLIMIT 200';
  if (language === 'promql') return 'rate(http_requests_total[5m])';
  if (language === 'json') return '{\n  "key": "value"\n}';
  if (language === 'yaml') return 'key: value';
  if (language === 'vrl') return '.level = upcase(.level)';
  if (language === 'javascript') return 'export function transform(event) {\n  return event;\n}';
  if (language === 'field-query') return 'trace_id = "..." AND service_name contains "checkout"';
  if (language === 'template') return '{{severity}} {{rule.name}}';
  return undefined;
}

function editorMetrics(compact: boolean, fontSize?: number, lineHeight?: number) {
  return {
    fontSize: fontSize ?? CODE_EDITOR_FONT_SIZE,
    lineHeight: lineHeight ?? CODE_EDITOR_LINE_HEIGHT,
    paddingBottom: compact ? 6 : 8,
    paddingTop: compact ? 6 : 8,
  };
}
