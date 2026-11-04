import type * as React from 'react';

/** One setting on the security page: what it is on the left, what to do about it on the right. */
export function SecurityRow({
  label,
  description,
  action,
  testId,
}: {
  label: string;
  description: React.ReactNode;
  action: React.ReactNode;
  testId?: string;
}) {
  return (
    <div
      className="grid min-h-20 gap-3 py-4 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-center"
      data-testid={testId}
    >
      <div>
        <div className="font-sans text-sm font-strong text-tx-0">{label}</div>
        <div className="mt-1 max-w-xl font-sans text-xs leading-relaxed text-tx-2">
          {description}
        </div>
      </div>
      <div>{action}</div>
    </div>
  );
}
