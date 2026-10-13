import { LoaderCircle, type LucideIcon, Play } from 'lucide-react';
import * as React from 'react';

import { ChromeButton } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/shell/ui/tooltip';

interface QueryIconActionProps
  extends Omit<React.ComponentPropsWithoutRef<typeof ChromeButton>, 'aria-label' | 'children' | 'title'> {
  icon: LucideIcon;
  /** What the action does. It is the button's name and the tip a pointer or the keyboard brings up. */
  label: string;
  iconClassName?: string | undefined;
  /** A dot in the button's corner: something it acts on has changed and waits for it. */
  marked?: boolean | undefined;
}

/**
 * How long after a menu or panel the button opened has closed a focus on the
 * button is that menu handing focus back, not someone arriving at the button.
 * The hand-back comes a task later, which a busy page can delay.
 */
const HANDED_BACK_MS = 250;

/**
 * An action of an explorer that is said by its sign alone: run, refresh,
 * share, columns, the histogram, download. Every one is the same box with the
 * same size of sign, so a row of them reads as one and leaves its room to the
 * query; the word is in the tip and in the name.
 */
export const QueryIconAction = React.forwardRef<HTMLButtonElement, QueryIconActionProps>(
  function QueryIconAction(
    { icon: Icon, label, iconClassName, marked = false, className, onFocus, ...button },
    ref,
  ) {
    // A trigger says through the button whether what it opens is open.
    const expanded = button['aria-expanded'] === true || button['aria-expanded'] === 'true';
    const wasExpandedRef = React.useRef(expanded);
    const closedAtRef = React.useRef(Number.NEGATIVE_INFINITY);

    React.useLayoutEffect(() => {
      if (wasExpandedRef.current && !expanded) closedAtRef.current = performance.now();
      wasExpandedRef.current = expanded;
    }, [expanded]);

    return (
      <Tooltip>
        <TooltipTrigger asChild>
          <ChromeButton
            ref={ref}
            data-query-icon-action
            {...button}
            aria-label={label}
            onFocus={(event) => {
              onFocus?.(event);
              // What the button opened hands focus back as it closes. That is not a question
              // about the button: the focus is held back from the tip, which opens on any other.
              if (performance.now() - closedAtRef.current < HANDED_BACK_MS) event.preventDefault();
            }}
            // The edge every variant is given, so the filled one is the box the gray ones are.
            className={cn('relative border border-transparent', className)}
          >
            <Icon aria-hidden="true" className={cn('h-3 w-3', iconClassName)} />
            {marked ? (
              <span
                aria-hidden="true"
                data-query-action-mark
                className="absolute right-[5px] top-[5px] h-1.5 w-1.5 rounded-full bg-orange-soft"
              />
            ) : null}
          </ChromeButton>
        </TooltipTrigger>
        <TooltipContent>{label}</TooltipContent>
      </Tooltip>
    );
  },
);

interface QueryRunActionProps
  extends Omit<QueryIconActionProps, 'icon' | 'iconClassName' | 'label' | 'marked' | 'variant'> {
  /** What running the query is called on this page. */
  label: string;
  /** What it is called while it runs. */
  runningLabel: string;
  running: boolean;
  /** The query has been changed since it last ran. */
  changed?: boolean | undefined;
}

/**
 * The run of an explorer's query: the one action of the bar in the product's
 * colour, a sign like the others. While the query runs the sign turns, and
 * the name says so.
 */
export const QueryRunAction = React.forwardRef<HTMLButtonElement, QueryRunActionProps>(
  function QueryRunAction({ label, runningLabel, running, changed, ...button }, ref) {
    return (
      <QueryIconAction
        ref={ref}
        data-query-run
        {...button}
        variant="primary"
        icon={running ? LoaderCircle : Play}
        iconClassName={running ? 'animate-spin' : undefined}
        label={running ? runningLabel : label}
        marked={changed}
      />
    );
  },
);
