import * as React from 'react';

import { CodeEditor, type CodeEditorHandle, type CodeEditorProps } from '@/shell/codeEditor';

export type JsonEditorProps = Omit<CodeEditorProps, 'language' | 'completionItems'>;

/** A line and the space above and below it: a value is never shown shorter. */
const ONE_LINE = 36;

/**
 * JSON written or read in the code editor: a key, a string, a number, `true`, `false`, `null` and
 * each level of brackets in a colour of its own, and a guide down every indent; no line numbers.
 * A text that is JSON is laid out with two-space indents as it is read in, pasted, or left.
 */
export const JsonEditor = React.forwardRef<CodeEditorHandle, JsonEditorProps>(function JsonEditor(
  { showHeader = false, showStatus = false, lineNumbers = false, ...props },
  ref,
) {
  return (
    <CodeEditor
      ref={ref}
      language="json"
      showHeader={showHeader}
      showStatus={showStatus}
      lineNumbers={lineNumbers}
      {...props}
    />
  );
});

export type JsonViewProps = Omit<JsonEditorProps, 'value' | 'onChange' | 'readOnly' | 'placeholder'> & {
  value: unknown;
};

/** A value shown as JSON, to read, select and copy from: as tall as it is, up to `maxHeight`. */
export function JsonView({ value, minHeight = ONE_LINE, ...props }: JsonViewProps) {
  const text = React.useMemo(() => JSON.stringify(value, null, 2) ?? '', [value]);

  return <JsonEditor value={text} readOnly minHeight={minHeight} {...props} />;
}
