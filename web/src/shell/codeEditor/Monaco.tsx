import * as monaco from 'monaco-editor/editor/editor.api.js';
import EditorWorker from 'monaco-editor/editor/editor.worker.js?worker';
import 'monaco-editor/editor/contrib/suggest/browser/suggestController.js';
import JsonWorker from 'monaco-editor/language/json/json.worker.js?worker';
import 'monaco-editor/language/json/monaco.contribution.js';
import 'monaco-editor/languages/definitions/javascript/register.js';
import 'monaco-editor/languages/definitions/sql/register.js';
import 'monaco-editor/languages/definitions/yaml/register.js';
import * as React from 'react';

import { cn } from '@/shell/lib/cn';

import { CodeEditorHeader, CodeEditorStatus } from './Chrome';
import {
  filterFieldQueryCompletionItems,
  presentFieldQueryCompletion,
  resolveFieldQueryCompletionContext,
} from './fieldQueryCompletion';
import type {
  CodeCompletionItem,
  CodeCompletionKind,
  CodeEditorHandle,
  CodeEditorMarker,
  CodeEditorProps,
  CodeLanguage,
} from './types';
import {
  CODE_EDITOR_FONT_FAMILY,
  CODE_EDITOR_FONT_SIZE,
  CODE_EDITOR_FONT_WEIGHT,
  CODE_EDITOR_LINE_HEIGHT,
} from './typography';

type MonacoGlobal = typeof globalThis & {
  MonacoEnvironment?: {
    getWorker: (_workerId: string, label: string) => Worker;
  };
  MoleSignalCompletionProviders?: monaco.IDisposable[];
};

const MONACO_THEME = 'molesignal-shell';
const CODE_LETTER_SPACING = 0;
const CODE_FRAME_HORIZONTAL_PADDING = 4;
const CODE_LEFT_INSET = 14;
const CODE_LINE_DECORATIONS_WIDTH = 12;
const CODE_PLACEHOLDER_FALLBACK_LEFT_WITH_LINE_NUMBERS = 51;
const FIELD_QUERY_LANGUAGE = 'field-query';
const NOTIFY_TEMPLATE_LANGUAGE = 'notify-template';

const FIELD_QUERY_FIELDS = [
  'body',
  'container',
  'duration',
  'duration_ms',
  'duration_ns',
  'error',
  'host',
  'http.method',
  'http.route',
  'level',
  'message',
  'method',
  'name',
  'operation',
  'operation_name',
  'parent_span_id',
  'pod',
  'route',
  'service',
  'service.name',
  'service_name',
  'span_id',
  'status',
  'status_code',
  'trace_id',
];

const DEFAULT_FIELD_QUERY_COMPLETIONS: CodeCompletionItem[] = [
  ...FIELD_QUERY_FIELDS.map((label) => ({ label, kind: 'field' as const, detail: 'field' })),
  { label: '=', insertText: '= ', kind: 'operator', detail: 'operator' },
  { label: '!=', insertText: '!= ', kind: 'operator', detail: 'operator' },
  { label: 'contains', insertText: 'contains ', kind: 'operator', detail: 'operator' },
  { label: 'AND', insertText: 'AND ', kind: 'operator', detail: 'operator' },
  { label: 'OR', insertText: 'OR ', kind: 'operator', detail: 'operator' },
];

const DEFAULT_SQL_COMPLETIONS: CodeCompletionItem[] = [
  { label: 'SELECT', insertText: 'SELECT ', kind: 'keyword', detail: 'keyword' },
  { label: 'FROM', insertText: 'FROM ', kind: 'keyword', detail: 'keyword' },
  { label: 'WHERE', insertText: 'WHERE ', kind: 'keyword', detail: 'keyword' },
  { label: 'GROUP BY', insertText: 'GROUP BY ', kind: 'keyword', detail: 'clause' },
  { label: 'ORDER BY', insertText: 'ORDER BY ', kind: 'keyword', detail: 'clause' },
  { label: 'LIMIT', insertText: 'LIMIT ', kind: 'keyword', detail: 'clause' },
  { label: 'COUNT', insertText: 'COUNT()', kind: 'keyword', detail: 'aggregate' },
  { label: 'AVG', insertText: 'AVG()', kind: 'keyword', detail: 'aggregate' },
  { label: 'MAX', insertText: 'MAX()', kind: 'keyword', detail: 'aggregate' },
  { label: 'MIN', insertText: 'MIN()', kind: 'keyword', detail: 'aggregate' },
];

const DEFAULT_VRL_COMPLETIONS: CodeCompletionItem[] = [
  { label: 'parse_json!', insertText: 'parse_json!()', kind: 'keyword', detail: 'function' },
  { label: 'parse_timestamp!', insertText: 'parse_timestamp!()', kind: 'keyword', detail: 'function' },
  { label: 'to_int!', insertText: 'to_int!()', kind: 'keyword', detail: 'function' },
  { label: 'to_string!', insertText: 'to_string!()', kind: 'keyword', detail: 'function' },
  { label: 'exists', insertText: 'exists()', kind: 'keyword', detail: 'function' },
  { label: 'del', insertText: 'del()', kind: 'keyword', detail: 'function' },
];

const completionItemsByModel = new Map<string, CodeCompletionItem[]>();

let customLanguagesReady = false;
let workersReady = false;

export const MonacoCodeEditor = React.forwardRef<CodeEditorHandle, CodeEditorProps>(
function MonacoCodeEditor({
  value,
  onChange,
  language = 'text',
  label,
  ariaLabel,
  placeholder,
  minHeight = 96,
  maxHeight = 320,
  fontSize,
  fontWeight = CODE_EDITOR_FONT_WEIGHT,
  lineHeight,
  lineNumbers = true,
  highlightCurrentLine = true,
  readOnly = false,
  onModEnter,
  onModSave,
  onCursorChange,
  completionItems,
  markers,
  resizable = false,
  compact = false,
  showHeader = true,
  showStatus = true,
  className,
}: CodeEditorProps, forwardedRef) {
  const hostRef = React.useRef<HTMLDivElement | null>(null);
  const editorRef = React.useRef<monaco.editor.IStandaloneCodeEditor | null>(null);
  const modelRef = React.useRef<monaco.editor.ITextModel | null>(null);
  const onChangeRef = React.useRef(onChange);
  const onModEnterRef = React.useRef(onModEnter);
  const onModSaveRef = React.useRef(onModSave);
  const onCursorChangeRef = React.useRef(onCursorChange);
  const completionItemsRef = React.useRef(completionItems);
  const valueRef = React.useRef(value);
  const manualHeightRef = React.useRef<number | null>(null);
  const resizeCleanupRef = React.useRef<(() => void) | null>(null);
  const initialOptionsRef = React.useRef({
    ariaLabel,
    compact,
    fontSize,
    fontWeight,
    highlightCurrentLine,
    label,
    language,
    lineHeight,
    lineNumbers,
    minHeight,
    readOnly,
  });
  const sizingRef = React.useRef({ compact, fontSize, lineHeight, maxHeight, minHeight });
  const [cursor, setCursor] = React.useState({ line: 1, col: 1 });
  const [isEmpty, setIsEmpty] = React.useState(() => value.length === 0);
  const [contentLineCount, setContentLineCount] = React.useState(() => lineCount(value));
  const [editorHeight, setEditorHeight] = React.useState(() =>
    estimateHeight(value, minHeight, maxHeight, compact, fontSize, lineHeight),
  );
  const [placeholderLeft, setPlaceholderLeft] = React.useState(
    () => CODE_FRAME_HORIZONTAL_PADDING
      + (lineNumbers ? CODE_PLACEHOLDER_FALLBACK_LEFT_WITH_LINE_NUMBERS : CODE_LEFT_INSET),
  );
  const effectivePlaceholder = placeholder ?? (!readOnly ? defaultPlaceholder(language) : undefined);
  const metrics = editorMetrics(compact, fontSize, lineHeight);

  sizingRef.current = { compact, fontSize, lineHeight, maxHeight, minHeight };

  React.useEffect(() => {
    onChangeRef.current = onChange;
  }, [onChange]);

  React.useEffect(() => {
    onModEnterRef.current = onModEnter;
  }, [onModEnter]);

  React.useEffect(() => {
    onModSaveRef.current = onModSave;
  }, [onModSave]);

  React.useEffect(() => {
    onCursorChangeRef.current = onCursorChange;
  }, [onCursorChange]);

  React.useEffect(() => {
    completionItemsRef.current = completionItems;
    const model = modelRef.current;
    if (model) {
      completionItemsByModel.set(model.uri.toString(), completionItems ?? []);
    }
  }, [completionItems]);

  React.useEffect(() => {
    valueRef.current = value;
    setIsEmpty(value.length === 0);
  }, [value]);

  const updateHeight = React.useCallback(() => {
    const editor = editorRef.current;
    const sizing = sizingRef.current;
    const autoHeight = editor
      ? clamp(Math.ceil(editor.getContentHeight()), sizing.minHeight, sizing.maxHeight)
      : estimateHeight(
          valueRef.current,
          sizing.minHeight,
          sizing.maxHeight,
          sizing.compact,
          sizing.fontSize,
          sizing.lineHeight,
        );
    const next = manualHeightRef.current === null
      ? autoHeight
      : clamp(manualHeightRef.current, sizing.minHeight, sizing.maxHeight);
    setEditorHeight(next);
    if (editor) {
      requestAnimationFrame(() => editor.layout());
    }
  }, []);

  React.useEffect(() => () => {
    resizeCleanupRef.current?.();
  }, []);

  React.useEffect(() => {
    configureMonaco();
    defineMoleSignalTheme();

    const root = document.documentElement;
    const observer = new MutationObserver(() => {
      defineMoleSignalTheme();
      monaco.editor.setTheme(MONACO_THEME);
    });
    observer.observe(root, {
      attributes: true,
      attributeFilter: ['data-theme', 'data-palette'],
    });
    return () => observer.disconnect();
  }, []);

  React.useEffect(() => {
    if (!hostRef.current) return undefined;

    configureMonaco();
    defineMoleSignalTheme();

    const initialOptions = initialOptionsRef.current;
    const initialMetrics = editorMetrics(initialOptions.compact, initialOptions.fontSize, initialOptions.lineHeight);
    const model = monaco.editor.createModel(valueRef.current, toMonacoLanguage(initialOptions.language));
    model.updateOptions({ tabSize: 2, insertSpaces: true });
    completionItemsByModel.set(model.uri.toString(), completionItemsRef.current ?? []);

    const editor = monaco.editor.create(hostRef.current, {
      model,
      theme: MONACO_THEME,
      ariaLabel: initialOptions.ariaLabel ?? initialOptions.label ?? `${initialOptions.language.toUpperCase()} editor`,
      autoClosingBrackets: 'always',
      autoClosingQuotes: 'always',
      autoIndent: 'full',
      automaticLayout: true,
      bracketPairColorization: {
        enabled: true,
        independentColorPoolPerBracketType: true,
      },
      matchBrackets: 'always',
      codeLens: false,
      contextmenu: true,
      cursorBlinking: 'smooth',
      cursorSmoothCaretAnimation: 'on',
      cursorStyle: 'line',
      cursorWidth: 2,
      detectIndentation: false,
      domReadOnly: initialOptions.readOnly,
      fixedOverflowWidgets: true,
      folding: true,
      foldingHighlight: true,
      foldingStrategy: 'auto',
      formatOnPaste: true,
      fontFamily: CODE_EDITOR_FONT_FAMILY,
      fontLigatures: true,
      fontSize: initialMetrics.fontSize,
      fontWeight: String(initialOptions.fontWeight),
      letterSpacing: CODE_LETTER_SPACING,
      guides: {
        bracketPairs: 'active',
        bracketPairsHorizontal: 'active',
        highlightActiveBracketPair: true,
        highlightActiveIndentation: true,
        indentation: true,
      },
      glyphMargin: false,
      hideCursorInOverviewRuler: true,
      lineDecorationsWidth: initialOptions.lineNumbers ? CODE_LINE_DECORATIONS_WIDTH : CODE_LEFT_INSET,
      lineHeight: initialMetrics.lineHeight,
      lineNumbers: initialOptions.lineNumbers ? 'on' : 'off',
      lineNumbersMinChars: initialOptions.lineNumbers ? 3 : 0,
      minimap: { enabled: false },
      occurrencesHighlight: 'singleFile',
      overviewRulerBorder: false,
      overviewRulerLanes: 0,
      padding: { top: initialMetrics.paddingTop, bottom: initialMetrics.paddingBottom },
      parameterHints: { cycle: true, enabled: !initialOptions.readOnly },
      quickSuggestions: quickSuggestionsFor(initialOptions.language, initialOptions.readOnly),
      quickSuggestionsDelay: 80,
      readOnly: initialOptions.readOnly,
      renderLineHighlight: initialOptions.highlightCurrentLine ? (initialOptions.lineNumbers ? 'all' : 'line') : 'none',
      renderWhitespace: 'selection',
      renderValidationDecorations: 'on',
      roundedSelection: true,
      scrollBeyondLastLine: false,
      scrollbar: {
        alwaysConsumeMouseWheel: false,
        horizontalScrollbarSize: 10,
        verticalScrollbarSize: 10,
      },
      selectOnLineNumbers: initialOptions.lineNumbers,
      showFoldingControls: 'mouseover',
      smoothScrolling: true,
      stickyScroll: {
        enabled: !initialOptions.compact && initialOptions.minHeight >= 280,
        maxLineCount: 3,
      },
      suggest: {
        insertMode: 'replace',
        preview: true,
        showIcons: true,
      },
      suggestOnTriggerCharacters: suggestionsEnabled(initialOptions.language, initialOptions.readOnly),
      tabSize: 2,
      tabCompletion: 'on',
      wordWrap: 'on',
      wrappingIndent: 'indent',
    });

    setPlaceholderLeft(editor.getLayoutInfo().contentLeft + CODE_FRAME_HORIZONTAL_PADDING);
    const layoutDisposable = editor.onDidLayoutChange((layoutInfo) => {
      setPlaceholderLeft(layoutInfo.contentLeft + CODE_FRAME_HORIZONTAL_PADDING);
    });

    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.Enter, () => {
      onModEnterRef.current?.();
    });
    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => {
      onModSaveRef.current?.();
    });

    let suggestionTimer: number | undefined;
    const changeDisposable = editor.onDidChangeModelContent((event) => {
      const next = model.getValue();
      valueRef.current = next;
      setIsEmpty(next.length === 0);
      setContentLineCount(model.getLineCount());
      onChangeRef.current?.(next);
      if (
        !event.isFlush
        && editor.hasTextFocus()
        && next.trim().length > 0
        && shouldAutoTriggerSuggestions(model, editor)
      ) {
        window.clearTimeout(suggestionTimer);
        suggestionTimer = window.setTimeout(() => {
          if (model.isDisposed() || editor.getModel() !== model || !editor.hasTextFocus()) return;
          editor.trigger(model.getLanguageId(), 'editor.action.triggerSuggest', {});
        }, 80);
      }
      updateHeight();
    });

    const cursorDisposable = editor.onDidChangeCursorPosition((event) => {
      const nextCursor = {
        line: event.position.lineNumber,
        col: event.position.column,
      };
      setCursor(nextCursor);
      onCursorChangeRef.current?.(nextCursor);
    });

    const sizeDisposable = editor.onDidContentSizeChange(updateHeight);

    editorRef.current = editor;
    modelRef.current = model;
    setContentLineCount(model.getLineCount());
    updateHeight();

    return () => {
      changeDisposable.dispose();
      cursorDisposable.dispose();
      layoutDisposable.dispose();
      sizeDisposable.dispose();
      window.clearTimeout(suggestionTimer);
      completionItemsByModel.delete(model.uri.toString());
      editor.dispose();
      model.dispose();
      editorRef.current = null;
      modelRef.current = null;
    };
  }, [updateHeight]);

  React.useEffect(() => {
    const model = modelRef.current;
    if (!model || model.getValue() === value) return;
    model.setValue(value);
    setContentLineCount(model.getLineCount());
    updateHeight();
  }, [updateHeight, value]);

  React.useEffect(() => {
    const model = modelRef.current;
    if (!model) return;
    monaco.editor.setModelLanguage(model, toMonacoLanguage(language));
  }, [language]);

  React.useEffect(() => {
    const model = modelRef.current;
    if (!model) return;
    monaco.editor.setModelMarkers(
      model,
      'molesignal-code-editor',
      (markers ?? []).map(toMonacoMarker),
    );
  }, [markers]);

  React.useImperativeHandle(forwardedRef, () => ({
    focus() {
      editorRef.current?.focus();
    },
    format() {
      void editorRef.current?.getAction('editor.action.formatDocument')?.run();
    },
    insertText(text: string) {
      const editor = editorRef.current;
      const model = modelRef.current;
      const selection = editor?.getSelection();
      if (!editor || !model || !selection) return;
      const startOffset = model.getOffsetAt(selection.getStartPosition());
      editor.executeEdits('molesignal-insert-text', [{
        range: selection,
        text,
        forceMoveMarkers: true,
      }]);
      editor.setPosition(model.getPositionAt(startOffset + text.length));
      editor.focus();
    },
  }), []);

  React.useEffect(() => {
    const editor = editorRef.current;
    if (!editor) return;
    const nextMetrics = editorMetrics(compact, fontSize, lineHeight);
    editor.updateOptions({
      ariaLabel: ariaLabel ?? label ?? `${language.toUpperCase()} editor`,
      domReadOnly: readOnly,
      fontFamily: CODE_EDITOR_FONT_FAMILY,
      fontSize: nextMetrics.fontSize,
      fontWeight: String(fontWeight),
      fontLigatures: true,
      letterSpacing: CODE_LETTER_SPACING,
      cursorWidth: 2,
      bracketPairColorization: {
        enabled: true,
        independentColorPoolPerBracketType: true,
      },
      guides: {
        bracketPairs: 'active',
        bracketPairsHorizontal: 'active',
        highlightActiveBracketPair: true,
        highlightActiveIndentation: true,
        indentation: true,
      },
      lineDecorationsWidth: lineNumbers ? CODE_LINE_DECORATIONS_WIDTH : CODE_LEFT_INSET,
      lineHeight: nextMetrics.lineHeight,
      lineNumbers: lineNumbers ? 'on' : 'off',
      lineNumbersMinChars: lineNumbers ? 3 : 0,
      padding: { top: nextMetrics.paddingTop, bottom: nextMetrics.paddingBottom },
      readOnly,
      parameterHints: { cycle: true, enabled: !readOnly },
      quickSuggestions: quickSuggestionsFor(language, readOnly),
      renderLineHighlight: highlightCurrentLine ? (lineNumbers ? 'all' : 'line') : 'none',
      selectOnLineNumbers: lineNumbers,
      stickyScroll: {
        enabled: !compact && minHeight >= 280,
        maxLineCount: 3,
      },
      suggestOnTriggerCharacters: suggestionsEnabled(language, readOnly),
    });
    updateHeight();
  }, [ariaLabel, compact, fontSize, fontWeight, highlightCurrentLine, label, language, lineHeight, lineNumbers, minHeight, placeholder, readOnly, updateHeight]);

  React.useEffect(() => {
    updateHeight();
  }, [compact, maxHeight, minHeight, updateHeight]);

  const handleResizeStart = React.useCallback((event: React.MouseEvent<HTMLDivElement>) => {
    if (!resizable || readOnly) return;
    event.preventDefault();
    resizeCleanupRef.current?.();

    const startY = event.clientY;
    const startHeight = editorHeight;
    const handleMove = (moveEvent: MouseEvent) => {
      const sizing = sizingRef.current;
      const next = clamp(startHeight + moveEvent.clientY - startY, sizing.minHeight, sizing.maxHeight);
      manualHeightRef.current = next;
      setEditorHeight(next);
      requestAnimationFrame(() => editorRef.current?.layout());
    };
    const handleUp = () => {
      window.removeEventListener('mousemove', handleMove);
      window.removeEventListener('mouseup', handleUp);
      resizeCleanupRef.current = null;
    };
    window.addEventListener('mousemove', handleMove);
    window.addEventListener('mouseup', handleUp);
    resizeCleanupRef.current = handleUp;
  }, [editorHeight, readOnly, resizable]);

  const resetManualHeight = React.useCallback(() => {
    manualHeightRef.current = null;
    updateHeight();
  }, [updateHeight]);

  return (
    <div
      className={cn(
        'code-editor-shell overflow-hidden rounded-md border border-bd-1 bg-bg-1 shadow-[inset_0_1px_0_rgba(255,255,255,0.02)]',
        readOnly && 'bg-bg-2',
        className,
      )}
      style={{ '--code-editor-font-weight': fontWeight } as React.CSSProperties}
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
      <div className="relative overflow-hidden bg-bg-0" style={{ height: editorHeight }}>
        <div
          ref={hostRef}
          className="h-full w-full"
          style={{
            paddingLeft: CODE_FRAME_HORIZONTAL_PADDING,
            paddingRight: CODE_FRAME_HORIZONTAL_PADDING,
          }}
        />
        {effectivePlaceholder && isEmpty ? (
          <div
            aria-hidden="true"
            className="pointer-events-none absolute inset-y-0 right-3 z-10 overflow-hidden whitespace-pre-wrap font-normal italic tracking-normal text-tx-3"
            style={{
              fontFamily: CODE_EDITOR_FONT_FAMILY,
              fontSize: metrics.fontSize,
              fontWeight,
              left: placeholderLeft,
              lineHeight: `${metrics.lineHeight}px`,
              paddingTop: metrics.paddingTop,
            }}
          >
            {effectivePlaceholder}
          </div>
        ) : null}
      </div>
      {resizable && !readOnly ? (
        <div
          role="separator"
          aria-orientation="horizontal"
          aria-label="Resize query editor"
          className="group flex h-2 cursor-row-resize items-center justify-center border-t border-bd-0 bg-bg-1 hover:bg-bg-2"
          onDoubleClick={resetManualHeight}
          onMouseDown={handleResizeStart}
        >
          <span className="h-px w-8 rounded-full bg-bd-2 opacity-70 group-hover:bg-blue" />
        </div>
      ) : null}
      {showStatus && (
        <CodeEditorStatus
          language={language}
          lineCount={contentLineCount}
          line={cursor.line}
          column={cursor.col}
        />
      )}
    </div>
  );
});

function configureMonaco() {
  if (!workersReady) {
    (globalThis as MonacoGlobal).MonacoEnvironment = {
      getWorker(_workerId, label) {
        if (label === 'json') return new JsonWorker();
        return new EditorWorker();
      },
    };
    workersReady = true;
  }

  if (customLanguagesReady) return;
  const runtime = globalThis as MonacoGlobal;
  runtime.MoleSignalCompletionProviders?.forEach((provider) => provider.dispose());
  runtime.MoleSignalCompletionProviders = [];
  registerSql();
  registerPromql();
  registerVrl();
  registerFieldQuery();
  registerNotifyTemplate();
  customLanguagesReady = true;
}

function registerSql() {
  monaco.languages.setLanguageConfiguration('sql', {
    comments: {
      lineComment: '--',
      blockComment: ['/*', '*/'],
    },
    brackets: [
      ['(', ')'],
    ],
    autoClosingPairs: [
      { open: '"', close: '"' },
      { open: "'", close: "'" },
      { open: '(', close: ')' },
    ],
    surroundingPairs: [
      { open: '"', close: '"' },
      { open: "'", close: "'" },
      { open: '(', close: ')' },
    ],
  });
  monaco.languages.setMonarchTokensProvider('sql', {
    ignoreCase: true,
    keywords: [
      'and',
      'as',
      'asc',
      'between',
      'by',
      'case',
      'desc',
      'distinct',
      'else',
      'end',
      'from',
      'group',
      'having',
      'in',
      'is',
      'join',
      'left',
      'limit',
      'match',
      'match_text',
      'not',
      'null',
      'on',
      'or',
      'order',
      'right',
      'select',
      'then',
      'when',
      'where',
    ],
    tokenizer: {
      root: [
        [/--.*$/, 'comment'],
        [/\/\*/, 'comment', '@comment_block'],
        [/[a-zA-Z_][\w$]*\s*(?=\()/, 'function'],
        [/[a-zA-Z_][\w$]*/, { cases: { '@keywords': 'keyword', '@default': 'identifier' } }],
        [/"([^"\\]|\\.)*$/, 'string.invalid'],
        [/"/, 'string', '@string_double'],
        [/'([^'\\]|\\.)*$/, 'string.invalid'],
        [/'/, 'string', '@string_single'],
        [/\b-?\d+(?:\.\d+)?\b/, 'number'],
        [/[()[\],.;]/, 'delimiter'],
        [/[+\-*/%=!<>|&]+/, 'operator'],
      ],
      comment_block: [
        [/[^*/]+/, 'comment'],
        [/\*\//, 'comment', '@pop'],
        [/./, 'comment'],
      ],
      string_double: [
        [/[^\\"]+/, 'string'],
        [/\\./, 'string.escape'],
        [/"/, 'string', '@pop'],
      ],
      string_single: [
        [/[^\\']+/, 'string'],
        [/\\./, 'string.escape'],
        [/'/, 'string', '@pop'],
      ],
    },
  });
  registerCompletionProvider('sql', DEFAULT_SQL_COMPLETIONS, [' ', '.']);
}

function registerPromql() {
  if (!monaco.languages.getLanguages().some((language) => language.id === 'promql')) {
    monaco.languages.register({ id: 'promql' });
  }
  monaco.languages.setLanguageConfiguration('promql', {
    brackets: [
      ['{', '}'],
      ['[', ']'],
      ['(', ')'],
    ],
    autoClosingPairs: [
      { open: '{', close: '}' },
      { open: '[', close: ']' },
      { open: '(', close: ')' },
      { open: '"', close: '"' },
    ],
    surroundingPairs: [
      { open: '{', close: '}' },
      { open: '[', close: ']' },
      { open: '(', close: ')' },
      { open: '"', close: '"' },
    ],
  });
  monaco.languages.setMonarchTokensProvider('promql', {
    // Keep in sync with the engine's supported set
    // (crates/infra/src/query/promql). Function calls are also highlighted by
    // the `identifier(` rule below; bare operators (and/or/unless/by/...) rely
    // on this list.
    keywords: [
      'abs',
      'absent',
      'absent_over_time',
      'acos',
      'acosh',
      'and',
      'asin',
      'asinh',
      'atan',
      'atanh',
      'avg',
      'avg_over_time',
      'bool',
      'bottomk',
      'by',
      'ceil',
      'changes',
      'clamp',
      'clamp_max',
      'clamp_min',
      'cos',
      'cosh',
      'count',
      'count_over_time',
      'count_values',
      'day_of_month',
      'day_of_week',
      'day_of_year',
      'days_in_month',
      'deg',
      'delta',
      'deriv',
      'double_exponential_smoothing',
      'exp',
      'floor',
      'group',
      'group_left',
      'group_right',
      'histogram_quantile',
      'holt_winters',
      'hour',
      'idelta',
      'ignoring',
      'increase',
      'irate',
      'label_join',
      'label_replace',
      'last_over_time',
      'limit_ratio',
      'limitk',
      'ln',
      'log10',
      'log2',
      'mad_over_time',
      'max',
      'max_over_time',
      'min',
      'min_over_time',
      'minute',
      'month',
      'offset',
      'on',
      'or',
      'pi',
      'predict_linear',
      'present_over_time',
      'quantile',
      'quantile_over_time',
      'rad',
      'rate',
      'resets',
      'round',
      'scalar',
      'sgn',
      'sin',
      'sinh',
      'sort',
      'sort_by_label',
      'sort_by_label_desc',
      'sort_desc',
      'sqrt',
      'stddev',
      'stddev_over_time',
      'stdvar',
      'stdvar_over_time',
      'sum',
      'sum_over_time',
      'tan',
      'tanh',
      'time',
      'timestamp',
      'topk',
      'unless',
      'vector',
      'without',
      'year',
    ],
    tokenizer: {
      root: [
        [/[a-zA-Z_:][\w:]*\s*(?=\()/, 'function'],
        [/[a-zA-Z_:][\w:]*/, { cases: { '@keywords': 'keyword', '@default': 'identifier' } }],
        [/"([^"\\]|\\.)*$/, 'string.invalid'],
        [/"/, 'string', '@string_double'],
        [/'([^'\\]|\\.)*$/, 'string.invalid'],
        [/'/, 'string', '@string_single'],
        [/\d+(?:\.\d+)?(?:ms|s|m|h|d|w|y)?/, 'number'],
        [/[{}()[\],]/, 'delimiter'],
        [/[+\-*/%^=!~<>]+/, 'operator'],
      ],
      string_double: [
        [/[^\\"]+/, 'string'],
        [/\\./, 'string.escape'],
        [/"/, 'string', '@pop'],
      ],
      string_single: [
        [/[^\\']+/, 'string'],
        [/\\./, 'string.escape'],
        [/'/, 'string', '@pop'],
      ],
    },
  });
  registerCompletionProvider('promql', [], ['(', '{', '[']);
}

function registerVrl() {
  if (!monaco.languages.getLanguages().some((language) => language.id === 'vrl')) {
    monaco.languages.register({ id: 'vrl' });
  }
  monaco.languages.setLanguageConfiguration('vrl', {
    comments: {
      lineComment: '#',
    },
    brackets: [
      ['{', '}'],
      ['[', ']'],
      ['(', ')'],
    ],
    autoClosingPairs: [
      { open: '{', close: '}' },
      { open: '[', close: ']' },
      { open: '(', close: ')' },
      { open: '"', close: '"' },
      { open: "'", close: "'" },
    ],
  });
  monaco.languages.setMonarchTokensProvider('vrl', {
    keywords: [
      'del',
      'downcase',
      'else',
      'exists',
      'false',
      'for',
      'if',
      'in',
      'merge',
      'now',
      'null',
      'parse_json',
      'parse_json!',
      'parse_timestamp',
      'to_int',
      'to_string',
      'true',
      'upcase',
    ],
    tokenizer: {
      root: [
        [/#.*$/, 'comment'],
        [/\/\/.*$/, 'comment'],
        [/[a-zA-Z_][\w!]*\s*(?=\()/, 'function'],
        [/[a-zA-Z_][\w!]*/, { cases: { '@keywords': 'keyword', '@default': 'identifier' } }],
        [/"([^"\\]|\\.)*$/, 'string.invalid'],
        [/"/, 'string', '@string_double'],
        [/'([^'\\]|\\.)*$/, 'string.invalid'],
        [/'/, 'string', '@string_single'],
        [/\b-?\d+(?:\.\d+)?\b/, 'number'],
        [/[{}()[\],.]/, 'delimiter'],
        [/[+\-*/%=!<>|&]+/, 'operator'],
      ],
      string_double: [
        [/[^\\"]+/, 'string'],
        [/\\./, 'string.escape'],
        [/"/, 'string', '@pop'],
      ],
      string_single: [
        [/[^\\']+/, 'string'],
        [/\\./, 'string.escape'],
        [/'/, 'string', '@pop'],
      ],
    },
  });
  registerCompletionProvider('vrl', DEFAULT_VRL_COMPLETIONS, ['.', '!', '(']);
}

function registerFieldQuery() {
  if (!monaco.languages.getLanguages().some((language) => language.id === FIELD_QUERY_LANGUAGE)) {
    monaco.languages.register({ id: FIELD_QUERY_LANGUAGE });
  }
  monaco.languages.setLanguageConfiguration(FIELD_QUERY_LANGUAGE, {
    brackets: [
      ['(', ')'],
      ['[', ']'],
    ],
    autoClosingPairs: [
      { open: '"', close: '"' },
      { open: "'", close: "'" },
      { open: '(', close: ')' },
      { open: '[', close: ']' },
    ],
    surroundingPairs: [
      { open: '"', close: '"' },
      { open: "'", close: "'" },
      { open: '(', close: ')' },
      { open: '[', close: ']' },
    ],
  });
  monaco.languages.setMonarchTokensProvider(FIELD_QUERY_LANGUAGE, {
    fields: FIELD_QUERY_FIELDS,
    operators: ['AND', 'OR', 'contains', 'like', 'eq', 'ne'],
    tokenizer: {
      root: [
        [/[a-zA-Z_][\w.]*\s*(?=\()/, 'function'],
        [/[a-zA-Z_][\w.]*/, { cases: { '@operators': 'operator.query', '@fields': 'field.query', '@default': 'identifier' } }],
        [/"([^"\\]|\\.)*$/, 'string.invalid'],
        [/"/, 'string.query', '@string_double'],
        [/'([^'\\]|\\.)*$/, 'string.invalid'],
        [/'/, 'string.query', '@string_single'],
        [/\b-?\d+(?:\.\d+)?(?:ms|s|m|h|d|w)?\b/, 'number'],
        [/[()[\],]/, 'delimiter'],
        [/!=|>=|<=|=|>|</, 'operator.query'],
      ],
      string_double: [
        [/[^\\"]+/, 'string.query'],
        [/\\./, 'string.escape'],
        [/"/, 'string.query', '@pop'],
      ],
      string_single: [
        [/[^\\']+/, 'string.query'],
        [/\\./, 'string.escape'],
        [/'/, 'string.query', '@pop'],
      ],
    },
  });
  registerCompletionProvider(
    FIELD_QUERY_LANGUAGE,
    DEFAULT_FIELD_QUERY_COMPLETIONS,
    [' ', '"', "'", '.', '=', '_'],
  );
}

function registerNotifyTemplate() {
  if (!monaco.languages.getLanguages().some((language) => language.id === NOTIFY_TEMPLATE_LANGUAGE)) {
    monaco.languages.register({ id: NOTIFY_TEMPLATE_LANGUAGE });
  }
  monaco.languages.setLanguageConfiguration(NOTIFY_TEMPLATE_LANGUAGE, {
    brackets: [
      ['{{', '}}'],
      ['(', ')'],
      ['[', ']'],
    ],
    autoClosingPairs: [
      { open: '{{', close: '}}' },
      { open: '(', close: ')' },
      { open: '[', close: ']' },
      { open: '"', close: '"' },
      { open: "'", close: "'" },
    ],
    surroundingPairs: [
      { open: '{{', close: '}}' },
      { open: '(', close: ')' },
      { open: '[', close: ']' },
      { open: '"', close: '"' },
      { open: "'", close: "'" },
    ],
  });
  monaco.languages.setMonarchTokensProvider(NOTIFY_TEMPLATE_LANGUAGE, {
    tokenizer: {
      root: [
        [/{{\s*[A-Za-z_][\w.-]*\s*}}/, 'variable.template'],
        [/{{|}}/, 'delimiter.bracket'],
        [/<\/?[A-Za-z][^>]*>/, 'tag'],
        [/^\s*#{1,6}\s+.*$/, 'keyword'],
        [/\*\*[^*]+\*\*/, 'type'],
        [/`[^`]+`/, 'string'],
        [/\[[^\]]+\]\([^)]+\)/, 'string'],
      ],
    },
  });
  registerCompletionProvider(NOTIFY_TEMPLATE_LANGUAGE, [], ['{', '.']);
}

function defineMoleSignalTheme() {
  const light = document.documentElement.getAttribute('data-theme') === 'light';
  const bg0 = cssColor('--bg-0', light ? '#f5f6f8' : '#08090c');
  const bg1 = cssColor('--bg-1', light ? '#fbfbfc' : '#0c0e13');
  const bg2 = cssColor('--bg-2', light ? '#eef0f5' : '#1a1f30');
  const bg3 = cssColor('--bg-3', light ? '#e3e6ee' : '#232839');
  const bd0 = cssColor('--bd-0', light ? '#e7e9ee' : '#1a1e28');
  const bd1 = cssColor('--bd-1', light ? '#d8dce4' : '#232936');
  const bd2 = cssColor('--bd-2', light ? '#b5bcce' : '#3a4263');
  const tx0 = cssColor('--tx-0', light ? '#15181f' : '#e7ebf3');
  const tx1 = cssColor('--tx-1', light ? '#404763' : '#b9bdcb');
  const tx2 = cssColor('--tx-2', light ? '#636a7d' : '#7a8295');
  const tx3 = cssColor('--tx-3', light ? '#9097a6' : '#545b6c');
  const keyword = cssColor('--purple', light ? '#7c3aed' : '#c792ea');
  const callable = cssColor('--blue', light ? '#2563eb' : '#82aaff');
  const string = cssColor('--green', light ? '#047857' : '#a8d88d');
  const number = cssColor('--orange', light ? '#c2410c' : '#f0a06b');
  const type = cssColor('--yellow', light ? '#9c6600' : '#e0b45a');
  const operator = cssColor('--indigo-soft', light ? '#4f46b8' : '#93a4ff');
  const invalid = cssColor('--red', light ? '#d1252a' : '#f78784');
  const editorBg = light ? bg1 : bg0;

  monaco.editor.defineTheme(MONACO_THEME, {
    base: light ? 'vs' : 'vs-dark',
    inherit: true,
    rules: [
      { token: '', foreground: tokenHex(tx0) },
      { token: 'comment', foreground: tokenHex(tx2), fontStyle: 'italic' },
      { token: 'delimiter', foreground: tokenHex(tx1) },
      { token: 'delimiter.bracket', foreground: tokenHex(tx1) },
      { token: 'delimiter.curly', foreground: tokenHex(tx1) },
      { token: 'delimiter.parenthesis', foreground: tokenHex(tx1) },
      { token: 'delimiter.square', foreground: tokenHex(tx1) },
      { token: 'delimiter.sql', foreground: tokenHex(tx1) },
      { token: 'field.query', foreground: tokenHex(callable) },
      { token: 'function', foreground: tokenHex(callable), fontStyle: 'bold' },
      { token: 'function.sql', foreground: tokenHex(callable), fontStyle: 'bold' },
      { token: 'identifier', foreground: tokenHex(tx0) },
      { token: 'identifier.sql', foreground: tokenHex(tx0) },
      { token: 'keyword', foreground: tokenHex(keyword), fontStyle: 'bold' },
      { token: 'keyword.sql', foreground: tokenHex(keyword), fontStyle: 'bold' },
      { token: 'keyword.json', foreground: tokenHex(keyword), fontStyle: 'bold' },
      { token: 'number', foreground: tokenHex(number) },
      { token: 'number.json', foreground: tokenHex(number) },
      { token: 'number.sql', foreground: tokenHex(number) },
      { token: 'operator', foreground: tokenHex(operator) },
      { token: 'operator.query', foreground: tokenHex(operator), fontStyle: 'bold' },
      { token: 'operator.sql', foreground: tokenHex(operator) },
      { token: 'predefined', foreground: tokenHex(keyword), fontStyle: 'bold' },
      { token: 'string', foreground: tokenHex(string) },
      { token: 'string.key.json', foreground: tokenHex(callable) },
      { token: 'string.value.json', foreground: tokenHex(string) },
      { token: 'string.sql', foreground: tokenHex(string) },
      { token: 'string.query', foreground: tokenHex(string) },
      { token: 'string.invalid', foreground: tokenHex(invalid) },
      { token: 'string.invalid.sql', foreground: tokenHex(invalid) },
      { token: 'regexp', foreground: tokenHex(number) },
      { token: 'tag', foreground: tokenHex(callable) },
      { token: 'type', foreground: tokenHex(type) },
      { token: 'type.identifier', foreground: tokenHex(type) },
      { token: 'variable', foreground: tokenHex(tx0) },
      { token: 'variable.template', foreground: tokenHex(keyword), fontStyle: 'bold' },
    ],
    colors: {
      'editor.background': editorBg,
      'editor.foreground': tx0,
      'editor.findMatchBackground': light ? '#ffedd5' : '#5a2b18',
      'editor.findMatchHighlightBackground': light ? '#fef3c7' : '#3b2a10',
      'editor.foldBackground': light ? '#e9edf7' : '#141b2b',
      'editor.hoverHighlightBackground': light ? '#e8edf8' : '#151d2e',
      'editor.inactiveSelectionBackground': light ? '#dce4f5' : '#1a2943',
      'editor.lineHighlightBackground': light ? '#edf0f6' : '#111725',
      'editor.lineHighlightBorder': bd0,
      'editor.placeholder.foreground': tx3,
      'editor.selectionBackground': light ? '#cbd8f3' : '#263d6b',
      'editor.selectionHighlightBackground': light ? '#dde5f4' : '#1b2c49',
      'editor.wordHighlightBackground': light ? '#dfe7f5' : '#172944',
      'editor.wordHighlightStrongBackground': light ? '#eadcf5' : '#302042',
      'editor.wordHighlightTextBackground': light ? '#e4e8f1' : '#202636',
      'editorBracketMatch.background': light ? '#dfe6f7' : '#1b2945',
      'editorBracketMatch.border': operator,
      'editorBracketHighlight.foreground1': callable,
      'editorBracketHighlight.foreground2': keyword,
      'editorBracketHighlight.foreground3': type,
      'editorBracketHighlight.foreground4': callable,
      'editorBracketHighlight.foreground5': keyword,
      'editorBracketHighlight.foreground6': type,
      'editorBracketHighlight.unexpectedBracket.foreground': invalid,
      'editorCursor.foreground': tx0,
      'editorError.foreground': invalid,
      'editorGutter.background': editorBg,
      'editorGutter.foldingControlForeground': tx2,
      'editorInfo.foreground': callable,
      'editorIndentGuide.activeBackground1': bd2,
      'editorIndentGuide.background1': bd0,
      'editorLineNumber.activeForeground': tx0,
      'editorLineNumber.foreground': tx3,
      'editorStickyScroll.background': editorBg,
      'editorStickyScroll.border': bd0,
      'editorStickyScrollHover.background': bg2,
      'editorSuggestWidget.background': bg1,
      'editorSuggestWidget.border': bd1,
      'editorSuggestWidget.foreground': tx1,
      'editorSuggestWidget.focusHighlightForeground': number,
      'editorSuggestWidget.highlightForeground': number,
      'editorSuggestWidget.selectedBackground': bg3,
      'editorSuggestWidget.selectedForeground': tx0,
      'editorWarning.foreground': type,
      'editorWhitespace.foreground': bd1,
      'editorWidget.background': bg1,
      'editorWidget.border': bd0,
      'editorWidget.foreground': tx1,
      'input.background': bg0,
      'input.border': bd1,
      'input.foreground': tx0,
      'input.placeholderForeground': tx3,
    },
  });
  monaco.editor.setTheme(MONACO_THEME);
}

function registerCompletionProvider(
  language: string,
  defaults: CodeCompletionItem[],
  triggerCharacters: string[],
) {
  const provider = monaco.languages.registerCompletionItemProvider(language, {
    triggerCharacters,
    provideCompletionItems(model, position) {
      const word = model.getWordUntilPosition(position);
      const range = completionRange(model, position, language, word);
      const configured = completionItemsByModel.get(model.uri.toString()) ?? [];
      const merged = mergeCompletionItems(configured, defaults);
      const fieldQueryContext = language === FIELD_QUERY_LANGUAGE
        ? resolveFieldQueryCompletionContext(model.getValueInRange({
            startLineNumber: 1,
            startColumn: 1,
            endLineNumber: position.lineNumber,
            endColumn: position.column,
          }))
        : null;
      const items = fieldQueryContext
        ? filterFieldQueryCompletionItems(merged, fieldQueryContext)
        : merged;
      return {
        suggestions: items.map((item) => {
          const presentation = fieldQueryContext
            ? presentFieldQueryCompletion(item, fieldQueryContext)
            : { label: item.label, insertText: item.insertText ?? item.label, advanceSnippet: false };
          return {
            label: presentation.label,
            kind: completionKind(item.kind),
            insertText: presentation.insertText,
            range,
            ...(presentation.advanceSnippet
              ? {
                  command: {
                    id: 'jumpToNextSnippetPlaceholder',
                    title: 'Go to next function argument',
                  },
                }
              : {}),
            ...(item.detail ? { detail: item.detail } : {}),
            ...(item.documentation ? { documentation: item.documentation } : {}),
            ...(item.insertTextFormat === 'snippet'
              ? {
                  insertTextRules:
                    monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet,
                }
              : {}),
            ...(item.sortText ? { sortText: item.sortText } : {}),
          };
        }),
      };
    },
  });
  const runtime = globalThis as MonacoGlobal;
  runtime.MoleSignalCompletionProviders ??= [];
  runtime.MoleSignalCompletionProviders.push(provider);
}

function mergeCompletionItems(...groups: CodeCompletionItem[][]): CodeCompletionItem[] {
  const seen = new Set<string>();
  const merged: CodeCompletionItem[] = [];
  for (const group of groups) {
    for (const item of group) {
      const key = `${item.kind ?? 'keyword'}:${item.field ?? ''}:${item.label}`;
      if (seen.has(key)) continue;
      seen.add(key);
      merged.push(item);
    }
  }
  return merged;
}

function completionKind(kind: CodeCompletionKind | undefined): monaco.languages.CompletionItemKind {
  if (kind === 'field') return monaco.languages.CompletionItemKind.Field;
  if (kind === 'function') return monaco.languages.CompletionItemKind.Function;
  if (kind === 'aggregation') return monaco.languages.CompletionItemKind.Method;
  if (kind === 'metric') return monaco.languages.CompletionItemKind.Variable;
  if (kind === 'label') return monaco.languages.CompletionItemKind.Field;
  if (kind === 'operator') return monaco.languages.CompletionItemKind.Operator;
  if (kind === 'value') return monaco.languages.CompletionItemKind.Value;
  return monaco.languages.CompletionItemKind.Keyword;
}

function cssColor(name: string, fallback: string): string {
  const raw = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return normalizeColor(raw) ?? fallback;
}

function normalizeColor(raw: string): string | null {
  const value = raw.trim();
  const hex = value.match(/^#([0-9a-f]{3}|[0-9a-f]{4}|[0-9a-f]{6}|[0-9a-f]{8})$/i)?.[1];
  if (hex) {
    if (hex.length === 3 || hex.length === 4) {
      const [r, g, b] = hex;
      if (!r || !g || !b) return null;
      return `#${r}${r}${g}${g}${b}${b}`.toLowerCase();
    }
    return `#${hex.slice(0, 6)}`.toLowerCase();
  }

  const rgb = value.match(/^rgba?\((.+)\)$/i);
  if (!rgb?.[1]) return null;
  const [r, g, b] = rgb[1].match(/[\d.]+/g)?.map((part) => Number(part)) ?? [];
  if (r === undefined || g === undefined || b === undefined) return null;
  return `#${toHex(r)}${toHex(g)}${toHex(b)}`;
}

function tokenHex(color: string): string {
  return color.replace('#', '').slice(0, 6);
}

function toHex(value: number): string {
  return Math.max(0, Math.min(255, Math.round(value))).toString(16).padStart(2, '0');
}

function toMonacoLanguage(language: CodeLanguage): string {
  if (language === 'text') return 'plaintext';
  if (language === 'template') return NOTIFY_TEMPLATE_LANGUAGE;
  return language;
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

function suggestionsEnabled(language: CodeLanguage, readOnly: boolean): boolean {
  return !readOnly && language !== 'text';
}

function shouldAutoTriggerSuggestions(
  model: monaco.editor.ITextModel,
  editor: monaco.editor.IStandaloneCodeEditor,
): boolean {
  const language = model.getLanguageId();
  if (
    ![FIELD_QUERY_LANGUAGE, NOTIFY_TEMPLATE_LANGUAGE, 'promql', 'sql', 'vrl']
      .includes(language)
  ) return false;
  const position = editor.getPosition();
  if (!position || position.column <= 1) return false;
  const previous = model.getLineContent(position.lineNumber).charAt(position.column - 2);
  if (language === FIELD_QUERY_LANGUAGE) return /[\w."'=_\s]/.test(previous);
  if (language === NOTIFY_TEMPLATE_LANGUAGE) return /[\w.{]/.test(previous);
  if (language === 'promql') return /[A-Za-z0-9_:]/.test(previous);
  return /[A-Za-z0-9_.!]/.test(previous);
}

function completionRange(
  model: monaco.editor.ITextModel,
  position: monaco.Position,
  language: string,
  word: monaco.editor.IWordAtPosition,
): monaco.IRange {
  if (language === NOTIFY_TEMPLATE_LANGUAGE) {
    const beforeCursor = model
      .getLineContent(position.lineNumber)
      .slice(0, position.column - 1);
    const opening = beforeCursor.lastIndexOf('{{');
    if (opening >= 0 && beforeCursor.lastIndexOf('}}') < opening) {
      return {
        startLineNumber: position.lineNumber,
        endLineNumber: position.lineNumber,
        startColumn: opening + 1,
        endColumn: position.column,
      };
    }
  }
  return {
    startLineNumber: position.lineNumber,
    endLineNumber: position.lineNumber,
    startColumn: word.startColumn,
    endColumn: word.endColumn,
  };
}

function toMonacoMarker(marker: CodeEditorMarker): monaco.editor.IMarkerData {
  const severity = marker.severity === 'info'
    ? monaco.MarkerSeverity.Info
    : marker.severity === 'warning'
      ? monaco.MarkerSeverity.Warning
      : monaco.MarkerSeverity.Error;
  return {
    severity,
    message: marker.message,
    startLineNumber: marker.line,
    endLineNumber: marker.line,
    startColumn: Math.max(1, marker.startColumn),
    endColumn: Math.max(marker.startColumn + 1, marker.endColumn),
  };
}

function quickSuggestionsFor(
  language: CodeLanguage,
  readOnly: boolean,
): false | { other: true; comments: false; strings: false } {
  if (!suggestionsEnabled(language, readOnly)) return false;
  return { other: true, comments: false, strings: false };
}

function lineCount(value: string): number {
  return Math.max(1, value.split('\n').length);
}

function estimateHeight(
  value: string,
  minHeight: number,
  maxHeight: number,
  compact: boolean,
  fontSize?: number,
  lineHeight?: number,
): number {
  const metrics = editorMetrics(compact, fontSize, lineHeight);
  const padding = metrics.paddingTop + metrics.paddingBottom;
  return clamp(lineCount(value) * metrics.lineHeight + padding, minHeight, maxHeight);
}

function editorMetrics(compact: boolean, fontSize?: number, lineHeight?: number) {
  return {
    fontSize: fontSize ?? CODE_EDITOR_FONT_SIZE,
    lineHeight: lineHeight ?? CODE_EDITOR_LINE_HEIGHT,
    paddingBottom: compact ? 6 : 8,
    paddingTop: compact ? 6 : 8,
  };
}

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}
