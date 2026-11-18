import * as React from 'react';

import { stackLines, type StackPartKind } from './parse';

/** The colours of a stack, those of code in the product's editor: types and places apart from words. */
const TONE: Record<StackPartKind, string> = {
  type: 'font-strong text-[var(--red-ink)]',
  message: 'text-tx-0',
  mark: 'text-tx-3',
  function: 'font-strong text-[var(--blue-ink)]',
  path: 'text-tx-3',
  file: 'text-tx-0',
  position: 'text-[var(--orange-ink)]',
  text: '',
};

/** A stack in its colours, line by line, as it came. */
export function StackText({ stack }: { stack: string }) {
  const lines = React.useMemo(() => stackLines(stack), [stack]);

  return (
    <>
      {lines.map((parts, index) => (
        <React.Fragment key={index}>
          {index > 0 && '\n'}
          {parts.map((piece, at) => (
            <span key={at} data-stack-part={piece.kind} className={TONE[piece.kind] || undefined}>
              {piece.text}
            </span>
          ))}
        </React.Fragment>
      ))}
    </>
  );
}
