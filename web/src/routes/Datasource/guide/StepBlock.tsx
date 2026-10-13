import { TriangleAlert } from 'lucide-react';

import { MarkdownCodeBlock } from '../codeBlock/MarkdownCodeBlock';
import type { SourceStepText } from '../sourceText';

/** One thing to do inside a step: what it is, why, and the command or file to copy. */
export function StepBlock({
  text,
  code,
}: {
  text: SourceStepText;
  code?: { language: string; content: string } | undefined;
}) {
  return (
    <div data-guide-step className="min-w-0">
      <h4 className="m-0 font-sans text-md font-strong text-tx-0">{text.title}</h4>
      {text.description && (
        <p className="m-0 mt-1 max-w-[680px] font-sans text-sm leading-relaxed text-tx-2">
          {text.description}
        </p>
      )}
      {code && (
        <MarkdownCodeBlock className="mt-[8px]" language={code.language} content={code.content} />
      )}
      {text.note && (
        <p className="m-0 mt-[8px] flex items-start gap-2 rounded-md bg-[var(--yellow-tint)] px-3 py-2 font-sans text-xs leading-relaxed text-[var(--yellow-ink)]">
          <TriangleAlert aria-hidden className="mt-0.5 h-3.5 w-3.5 shrink-0" />
          <span>{text.note}</span>
        </p>
      )}
    </div>
  );
}
