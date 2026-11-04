import { Check } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { writeClipboardText } from '@/lib/clipboard';
import { uiLabelClass, uiSectionTitleClass } from '@/shell/chrome';
import { CopyIconButton } from '@/shell/CopyIconButton';
import { cn } from '@/shell/lib/cn';
import { IconSwap } from '@/shell/polish/IconSwap';

/**
 * How far a section's words stand inside its edge: the line the opening statement's words stand on
 * (`STATEMENT_CELL`), so every line of the page starts at one x.
 */
export const SECTION_INSET = 'px-[20px]';

/** A named part of a source's page: one surface, its name at the top. */
export function SourceSection({
  title,
  description,
  action,
  children,
  className,
}: {
  title: React.ReactNode;
  description?: React.ReactNode;
  action?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
}) {
  // The section is named by its own heading, so it can be found and announced by it.
  const headingId = React.useId();

  return (
    <section
      aria-labelledby={headingId}
      className={cn(
        'min-w-0 overflow-hidden rounded-md bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]',
        className,
      )}
    >
      <div className={cn('flex min-h-12 items-center justify-between gap-4 py-2.5', SECTION_INSET)}>
        <div className="min-w-0">
          <h2 id={headingId} className={uiSectionTitleClass}>
            {title}
          </h2>
          {description && <p className="mt-0.5 text-pretty font-sans text-xs text-tx-2">{description}</p>}
        </div>
        {action}
      </div>
      {children}
    </section>
  );
}

/**
 * A value of the setup with its name over it and, when it needs one, a word under it: set as a
 * field is everywhere (a form's, a connector's callback address), the word a step lighter than
 * the name.
 */
export function Labeled({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string | undefined;
  children: React.ReactNode;
}) {
  return (
    <div className="flex min-w-0 flex-col gap-1.5">
      <span className={uiLabelClass}>{label}</span>
      {children}
      {hint && <ValueHint>{hint}</ValueHint>}
    </div>
  );
}

/** `done`: the sender is evidently set up. `todo`: still to do, in no order the page can see. */
export type SetupStepState = 'done' | 'todo';

/**
 * One step of setting up a sender, hung on the line that joins it to the next: a marker with its
 * number (a check once the source has received a request), the step's name as a topic, what there
 * is to know, and the values to copy. The guide's steps on the Datasource page, a size down,
 * since these sit inside a section.
 */
export function SetupStep({
  number,
  state,
  title,
  description,
  children,
}: {
  number: number;
  state: SetupStepState;
  title: string;
  description?: string | undefined;
  children?: React.ReactNode;
}) {
  return (
    <li
      data-setup-step={number}
      data-state={state}
      className="group/step relative grid grid-cols-[24px_minmax(0,1fr)] gap-x-3 pb-[24px] last:pb-0"
    >
      <span
        aria-hidden
        className="absolute bottom-[4px] left-[11.5px] top-[32px] w-px bg-bd-0 group-last/step:hidden"
      />
      <span
        aria-hidden
        className={cn(
          'grid h-[24px] w-[24px] place-items-center rounded-full font-mono text-xs font-bold transition-colors duration-fast',
          state === 'done' ? 'bg-[var(--green-tint)] text-[var(--green-ink)]' : 'bg-indigo-dim text-indigo-soft',
        )}
      >
        <IconSwap active={state === 'done'} inactiveIcon={number} activeIcon={<Check className="h-3 w-3 stroke-[3]" />} />
      </span>
      <div className="min-w-0">
        <h3 className="m-0 flex min-h-[24px] items-center font-sans text-md font-display-strong text-tx-0">{title}</h3>
        {description && <p className="m-0 mt-0.5 font-sans text-xs leading-relaxed text-tx-2">{description}</p>}
        {children && <div className="mt-2.5 flex min-w-0 flex-col gap-1.5">{children}</div>}
      </div>
    </li>
  );
}

/** A word under a value, as a field's is. */
export function ValueHint({ children }: { children: React.ReactNode }) {
  return <span className="font-sans text-xs leading-relaxed text-tx-3">{children}</span>;
}

/**
 * Configuration to paste into the sender, with one control to copy all of it.
 * A line longer than the block wraps, so a token is seen whole wherever the
 * block is; the copy is the text as written.
 */
export function SnippetBlock({ content, label }: { content: string; label: string }) {
  const { t } = useTranslation('common');
  const [copied, setCopied] = React.useState(false);
  const resetTimer = React.useRef<number | undefined>(undefined);

  React.useEffect(() => () => window.clearTimeout(resetTimer.current), []);

  const copy = async () => {
    try {
      await writeClipboardText(content);
      setCopied(true);
      window.clearTimeout(resetTimer.current);
      resetTimer.current = window.setTimeout(() => setCopied(false), 1500);
    } catch {
      // Clipboard access can be denied by the browser.
    }
  };

  return (
    <div className="relative min-w-0 rounded-md bg-[var(--page-canvas)]">
      <pre
        data-source-snippet
        className="m-0 whitespace-pre-wrap px-3 py-2.5 pr-12 font-mono text-xs leading-relaxed text-tx-1 [overflow-wrap:anywhere]"
      >
        {content}
      </pre>
      {/* The button's wrapper is what is placed: left in the flow, it would add a line under the text. */}
      <CopyIconButton
        onClick={() => void copy()}
        label={label}
        copied={copied}
        copiedLabel={t('actions.copied')}
        wrapperClassName="absolute right-1.5 top-1.5"
        className="h-8 w-8"
      />
    </div>
  );
}
