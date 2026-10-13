import {
  Activity,
  CheckCircle2,
  Globe2,
  KeyRound,
  Plus,
  Radar,
  ServerCog,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { useActionAccess } from '@/product/actionAccess';
import type { ProductStateProps } from '@/product/states';
import { ListPage } from '@/product/templates';
import { ChromeButton } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { ModuleTabs } from '@/shell/tabs/ModuleTabs';

// What the checks say and what they are, then where they run, then what they are given.
const TABS = [
  ['overview', '/synthetics/overview', Radar, 'checks'],
  ['checks', '/synthetics/checks', Activity, 'checks'],
  ['results', '/synthetics/results', CheckCircle2, 'checks'],
  ['locations', '/synthetics/locations', Globe2, 'runtime'],
  ['agents', '/synthetics/agents', ServerCog, 'runtime'],
  ['variables', '/synthetics/variables', KeyRound, 'inputs'],
] as const;

export function SyntheticsNavigation() {
  const { t } = useTranslation('synthetics');

  return (
    <ModuleTabs
      label={t('navigation_label')}
      className="relative z-10"
      dataAttributes={{ 'data-testid': 'synthetics-subnav' }}
      items={TABS.map(([key, to, icon, group]) => ({
        key,
        to,
        icon,
        group,
        label: t(`tabs.${key}`),
      }))}
    />
  );
}

/**
 * The frame every tab of Synthetics stands in.
 *
 * The module has one name, so the title does not change from tab to tab: the
 * tabs say which one is open. The header carries the one thing the open tab
 * lets a person add, and nothing else.
 */
export function SyntheticsPage({
  action,
  lead,
  listHead,
  state,
  contentSurface,
  children,
}: {
  /** The tab's own action: the thing that can be added to what it lists. */
  action?: React.ReactNode;
  /** The tab's opening statement, above its content. */
  lead?: React.ReactNode;
  /** What the list is showing: its views and filters, as the head of the list itself. */
  listHead?: React.ReactNode;
  state?: ProductStateProps | null | undefined;
  contentSurface?: boolean;
  children?: React.ReactNode;
}) {
  const { t } = useTranslation('synthetics');

  return (
    <ListPage
      appearance="surface"
      title={t('title')}
      subtitle={t('subtitle')}
      subnav={<SyntheticsNavigation />}
      toolbar={action}
      lead={lead}
      listHead={listHead}
      state={state}
      {...(contentSurface !== undefined ? { contentSurface } : {})}
    >
      {children}
    </ListPage>
  );
}

/**
 * What a tab shows in place of its content while its data is on the way, or
 * when it could not be had. Nothing once the data is in.
 */
export function useLoadState({
  pending,
  error,
  onRetry,
}: {
  pending: boolean;
  error: unknown;
  onRetry: () => void;
}): ProductStateProps | null {
  const { t } = useTranslation('synthetics');
  if (pending) return { variant: 'loading' };
  if (!error) return null;

  return {
    variant: 'error',
    title: t('states.load_error'),
    error,
    action: <ChromeButton onClick={onRetry}>{t('retry')}</ChromeButton>,
  };
}

/** The action of the tabs that are about checks: a new one. */
export function CreateCheckAction() {
  const { t } = useTranslation('synthetics');
  const navigate = useNavigate();
  const access = useActionAccess({ permission: 'synthetics.manage' });

  return (
    <ChromeButton
      variant="primary"
      disabled={access.disabled}
      disabledReason={access.reason}
      onClick={() => navigate('/synthetics/checks/new')}
    >
      <Plus aria-hidden className="h-3.5 w-3.5" />
      {t('actions.create_check')}
    </ChromeButton>
  );
}

/**
 * A list on a surface of its own, with what it is showing at its head, for a
 * tab that has more on it than one list. A tab that is one list gives its head
 * to the page instead (`listHead`).
 */
export function SyntheticsList({
  head,
  children,
  className,
}: {
  head?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <section
      data-list-surface
      className={cn(
        'min-w-0 overflow-hidden rounded-md bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]',
        className,
      )}
    >
      {head && (
        <div
          data-list-head
          className="flex min-h-[48px] min-w-0 flex-wrap items-center gap-x-[12px] gap-y-[8px] border-b border-bd-0 px-[12px] py-[8px]"
        >
          {head}
        </div>
      )}
      {children}
    </section>
  );
}

/** A named part of a tab that is not a list: one surface, its name at the top. */
export function SyntheticsSection({
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
      data-synthetics-section
      aria-labelledby={headingId}
      className={cn(
        'min-w-0 overflow-hidden rounded-md bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]',
        className,
      )}
    >
      <div className="flex min-h-12 items-center justify-between gap-4 px-4 py-2.5">
        <div className="min-w-0">
          <h2 id={headingId} className="font-sans text-sm font-display-strong text-tx-0">
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
