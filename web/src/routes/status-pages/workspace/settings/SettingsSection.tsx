import type { ReactNode } from 'react';

export function SettingsCard({
  title,
  description,
  children,
}: {
  title: string;
  description: string;
  children: ReactNode;
}) {
  return (
    <section data-status-page-settings-section className="min-w-0 rounded-md bg-[var(--functional-surface)] p-4 [box-shadow:var(--shadow-functional-surface)]">
      <header className="py-4">
        <h2 className="text-base font-display-strong text-tx-0">{title}</h2>
        <p className="mt-1 text-xs leading-5 text-tx-3">{description}</p>
      </header>
      <div className="space-y-5 py-5">{children}</div>
    </section>
  );
}

export function SettingsFooter({ children }: { children: ReactNode }) {
  return <div className="flex justify-end pt-4">{children}</div>;
}
