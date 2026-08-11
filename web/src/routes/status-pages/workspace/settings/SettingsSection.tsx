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
    <section className="rounded-lg border border-bd-0 bg-bg-1">
      <header className="border-b border-bd-0 px-5 py-4">
        <h2 className="text-base font-display-strong text-tx-0">{title}</h2>
        <p className="mt-1 text-xs leading-5 text-tx-3">{description}</p>
      </header>
      <div className="space-y-5 p-5">{children}</div>
    </section>
  );
}

export function SettingsFooter({ children }: { children: ReactNode }) {
  return <div className="flex justify-end border-t border-bd-0 pt-4">{children}</div>;
}
