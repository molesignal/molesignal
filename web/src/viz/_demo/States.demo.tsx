import { AxiosError } from 'axios';

import type { ProductEmptyStateStrategy } from '@/product/ia';
import { EmptyState } from '@/shell/EmptyState';
import { ErrorState } from '@/shell/ErrorState';
import { LoadingState } from '@/shell/LoadingState';

/**
 * Showcase of the three head-of-line state components (M0.4):
 *   - 7 EmptyState strategies (icon + copy + CTAs)
 *   - ErrorState across 3 representative API failure shapes
 *   - LoadingState across the 3 skeleton variants
 *
 * Visit at `/_demo/states` in dev. This page is dead-code-eliminated
 * from production builds (same guard as the other demo routes).
 */
export function StatesDemo() {
  return (
    <div className="mx-auto flex max-w-[1100px] flex-col gap-10 p-8">
      <header className="flex flex-col gap-1">
        <h1 className="font-sans text-xl font-display-strong text-tx-0">
          States — Empty / Error / Loading
        </h1>
        <p className="font-sans text-xs text-tx-2">
          Phase 4 head-of-line citizens. Every product surface that has no data, can&apos;t load,
          or is still loading uses these three components.
        </p>
      </header>

      <Section title="EmptyState — 7 strategies">
        <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
          {EMPTY_STRATEGIES.map((s) => (
            <DemoFrame key={s.strategy}>
              <EmptyState
                strategy={s.strategy}
                title={s.title}
                description={s.description}
                primaryAction={s.primaryAction}
                secondaryAction={s.secondaryAction}
              />
            </DemoFrame>
          ))}
        </div>
      </Section>

      <Section title="ErrorState — 3 failure shapes">
        <div className="grid grid-cols-1 gap-3">
          <DemoFrame>
            <ErrorState
              title="Cannot load alerts"
              error={mockAxiosError(503, 'Service unavailable', 'BACKEND_TIMEOUT')}
              onRetry={() => alert('retry')}
              onReport={() => alert('report')}
            />
          </DemoFrame>
          <DemoFrame>
            <ErrorState
              title="Query rejected"
              error={mockAxiosError(400, 'syntax error near "SELEC"', 'PARSE_ERROR')}
              onRetry={() => alert('retry')}
            />
          </DemoFrame>
          <DemoFrame>
            <ErrorState
              title="Network unreachable"
              error={new Error('Failed to fetch')}
              onRetry={() => alert('retry')}
            />
          </DemoFrame>
        </div>
      </Section>

      <Section title="LoadingState — 3 variants (cancel appears at 3s)">
        <div className="grid grid-cols-1 gap-3 md:grid-cols-3">
          <DemoFrame>
            <LoadingState variant="query" onCancel={() => alert('cancel')} />
          </DemoFrame>
          <DemoFrame>
            <LoadingState variant="list" rows={5} onCancel={() => alert('cancel')} />
          </DemoFrame>
          <DemoFrame>
            <LoadingState variant="chart" onCancel={() => alert('cancel')} />
          </DemoFrame>
        </div>
      </Section>
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="flex flex-col gap-3">
      <h2 className="font-sans text-sm font-display-strong uppercase tracking-wider text-tx-2">
        {title}
      </h2>
      {children}
    </section>
  );
}

function DemoFrame({ children }: { children: React.ReactNode }) {
  return (
    <div className="rounded-md border border-bd-0 bg-bg-1 p-4">{children}</div>
  );
}

const EMPTY_STRATEGIES: ReadonlyArray<{
  strategy: ProductEmptyStateStrategy;
  title: string;
  description: string;
  primaryAction?: { label: string; onClick: () => void };
  secondaryAction?: { label: string; onClick: () => void };
}> = [
  {
    strategy: 'activation',
    title: 'Welcome to Molesignal',
    description: 'Connect your first data source to start observing.',
    primaryAction: { label: 'Get started', onClick: () => alert('activate') },
    secondaryAction: { label: 'Read the docs', onClick: () => alert('docs') },
  },
  {
    strategy: 'query-first',
    title: 'Run a query to see results',
    description: 'Logs, metrics, and traces are searched on demand. Hit Run, or pick a saved view.',
    primaryAction: { label: 'Run query', onClick: () => alert('run') },
    secondaryAction: { label: 'Open saved views', onClick: () => alert('views') },
  },
  {
    strategy: 'create-first',
    title: 'No dashboards yet',
    description: 'Create one from scratch, or import a MoleSignal Dashboard JSON.',
    primaryAction: { label: 'New dashboard', onClick: () => alert('new') },
    secondaryAction: { label: 'Import JSON', onClick: () => alert('import') },
  },
  {
    strategy: 'backend-pending',
    title: 'Waiting for backend',
    description: 'This page queries a stream that hasn’t received data yet, or an endpoint not deployed in this build.',
    secondaryAction: { label: 'Check datasource', onClick: () => alert('datasource') },
  },
  {
    strategy: 'license-gated',
    title: 'Mole Intelligence is a Pro feature',
    description: 'Investigate incidents with assisted analysis and controlled operations.',
    primaryAction: { label: 'Upgrade', onClick: () => alert('upgrade') },
    secondaryAction: { label: 'Compare editions', onClick: () => alert('compare') },
  },
  {
    strategy: 'permission-denied',
    title: 'You don’t have access to IAM',
    description: 'Ask an Owner or Admin to grant the iam:read role.',
    secondaryAction: { label: 'Contact admin', onClick: () => alert('admin') },
  },
  {
    strategy: 'none',
    title: 'Nothing here',
    description: 'A generic empty surface. Prefer one of the 6 specific strategies above.',
  },
];

function mockAxiosError(status: number, message: string, code?: string): AxiosError {
  const err = new AxiosError(`Request failed with status code ${status}`);
  err.response = {
    status,
    statusText: 'X',
    data: { message, code },
    headers: {},
    config: {} as never,
  };
  return err;
}
