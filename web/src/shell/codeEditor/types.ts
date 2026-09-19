export type CodeLanguage =
  | 'sql'
  | 'promql'
  | 'json'
  | 'yaml'
  | 'vrl'
  | 'javascript'
  | 'field-query'
  | 'template'
  | 'text';

export type CodeCompletionKind =
  | 'field'
  | 'operator'
  | 'value'
  | 'keyword'
  | 'function'
  | 'aggregation'
  | 'metric'
  | 'label';

export interface CodeCompletionItem {
  label: string;
  detail?: string;
  documentation?: string;
  insertText?: string;
  insertTextFormat?: 'plain' | 'snippet';
  kind?: CodeCompletionKind;
  sortText?: string;
  /** Fields-mode value suggestions are shown only while editing this field. */
  field?: string;
  /** Unquoted value used when completing inside an existing string literal. */
  value?: string;
}

export interface CodeEditorMarker {
  line: number;
  startColumn: number;
  endColumn: number;
  message: string;
  severity?: 'error' | 'warning' | 'info';
}

export interface CodeEditorHandle {
  focus: () => void;
  format: () => void;
  insertText: (text: string) => void;
}

export interface CodeEditorProps {
  value: string;
  onChange?: (value: string) => void;
  language?: CodeLanguage;
  label?: string;
  ariaLabel?: string;
  placeholder?: string;
  minHeight?: number;
  maxHeight?: number;
  fontSize?: number;
  fontWeight?: number;
  lineHeight?: number;
  lineNumbers?: boolean;
  highlightCurrentLine?: boolean;
  readOnly?: boolean;
  onModEnter?: () => void;
  onModSave?: () => void;
  onCursorChange?: (cursor: { line: number; col: number }) => void;
  completionItems?: CodeCompletionItem[];
  markers?: CodeEditorMarker[];
  resizable?: boolean;
  compact?: boolean;
  showHeader?: boolean;
  showStatus?: boolean;
  className?: string;
}
