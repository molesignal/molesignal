import * as React from 'react';

import { cn } from '@/shell/lib/cn';
import { SignalReference } from '@/shell/SignalReference';

/**
 * One part of an incident. In the drawer the parts are a column inside the
 * drawer's own surface, each under a small heading. On the incident's page
 * they are surfaces themselves, so the page reads as a set of cards and not as
 * text poured onto the canvas.
 */
export function IncidentSection({
  title,
  icon,
  surface = false,
  children,
}: {
  title: string;
  icon?: React.ReactNode;
  /** Draw the part as a surface of its own: the incident's page does, the drawer does not. */
  surface?: boolean;
  children: React.ReactNode;
}) {
  if (surface) {
    return (
      <section
        data-incident-section
        className="min-w-0 rounded-md bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]"
      >
        <h2 className="flex min-h-12 items-center gap-2 px-4 font-sans text-sm font-display-strong text-tx-0">
          {icon && <span className="text-tx-2">{icon}</span>}
          {title}
        </h2>
        <div className="flex flex-col gap-3 px-4 pb-4">{children}</div>
      </section>
    );
  }

  return (
    <section data-incident-section className="flex flex-col gap-2">
      <h3 className="flex items-center gap-1.5 font-sans text-xs font-strong text-tx-2">
        {icon}
        {title}
      </h3>
      {children}
    </section>
  );
}

/** Services, traces or hosts an incident touches, each a way into that signal. */
export function SignalRow({
  label,
  items,
  type,
  time,
}: {
  label: string;
  items: string[];
  type: 'trace_id' | 'host' | 'service';
  time: { from: string; to: string };
}) {
  return (
    <div className="flex flex-col gap-1">
      <div className="font-sans text-xs text-tx-2">{label}</div>
      <ul className="flex flex-wrap gap-1.5">
        {items.map((value) => (
          <li key={value}>
            <SignalReference type={type} value={value} time={time}>
              {/* Long ids are shortened here; the hover card has the whole value and a copy button. */}
              {value.length > 24 ? `${value.slice(0, 12)}…${value.slice(-6)}` : value}
            </SignalReference>
          </li>
        ))}
      </ul>
    </div>
  );
}

const WEB_ADDRESS = /^https?:\/\/\S+$/i;

/** Labels or annotations as they were written. A value that is a web address is a link to it. */
export function KvBlock({ label, entries }: { label: string; entries: Record<string, string> }) {
  return (
    <div className="flex flex-col gap-1.5">
      <div className="font-sans text-xs text-tx-2">{label}</div>
      <dl className="grid grid-cols-[max-content_minmax(0,1fr)] gap-x-4 gap-y-1.5 rounded-md bg-bg-2 px-3 py-2.5 font-sans text-xs">
        {Object.entries(entries).map(([key, value]) => (
          <React.Fragment key={key}>
            <dt className="font-mono text-tx-2">{key}</dt>
            <dd className={cn('min-w-0 break-words text-tx-0')}>
              {WEB_ADDRESS.test(value) ? (
                <a
                  href={value}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="break-all text-indigo-soft underline-offset-4 outline-none hover:underline focus-visible:underline"
                >
                  {value}
                </a>
              ) : (
                value
              )}
            </dd>
          </React.Fragment>
        ))}
      </dl>
    </div>
  );
}
