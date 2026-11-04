import { ChevronDown, type LucideIcon } from 'lucide-react';
import * as React from 'react';

/**
 * A part of the template editor's right column that folds up and down: closed until its head is
 * pressed. What it holds is kept while it is closed, since that lives with the editor.
 */
export function FoldSection({
  icon: Icon,
  title,
  description,
  children,
}: {
  icon: LucideIcon;
  title: string;
  /** Said under the head once the section is open. */
  description?: string;
  children: React.ReactNode;
}) {
  const [open, setOpen] = React.useState(false);
  const id = React.useId();

  return (
    <section data-fold-section={open ? 'open' : 'closed'} className="rounded-md border border-bd-0 bg-bg-2">
      <h3>
        <button
          type="button"
          aria-expanded={open}
          aria-controls={id}
          onClick={() => setOpen((current) => !current)}
          className={`flex min-h-11 w-full items-center gap-2 px-3 text-left transition-colors hover:bg-bg-1 focus-visible:bg-bg-3 ${
            open ? 'rounded-t-md' : 'rounded-md'
          }`}
        >
          <Icon aria-hidden className="h-4 w-4 shrink-0 text-tx-2" />
          <span className="min-w-0 flex-1 text-xs font-semibold text-tx-0">{title}</span>
          <ChevronDown
            aria-hidden
            className={`h-3 w-3 shrink-0 text-tx-3 transition-transform ${open ? 'rotate-180' : ''}`}
          />
        </button>
      </h3>
      {open && (
        <div id={id} className="border-t border-bd-0">
          {description && <p className="px-3 pt-2.5 text-xs text-tx-3">{description}</p>}
          {children}
        </div>
      )}
    </section>
  );
}
