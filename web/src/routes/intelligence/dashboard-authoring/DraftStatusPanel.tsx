import {
  AlertTriangle,
  CheckCircle2,
  Clock3,
  ExternalLink,
  ShieldCheck,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { DashboardDraftPreview } from '@/api/intelligence/dashboardAuthoring';
import { formatMicrosActive } from '@/lib/time';
import { cn } from '@/shell/lib/cn';
import { Alert, AlertDescription, AlertTitle } from '@/shell/ui/alert';
import { Badge } from '@/shell/ui/badge';
import { Button } from '@/shell/ui/button';
import { Card, CardContent, CardHeader, CardTitle } from '@/shell/ui/card';

import {
  dashboardDraftAction,
  effectiveDraftExpiry,
  preflightRange,
  remainingDuration,
  uniqueDraftWarnings,
} from './model';

interface DraftStatusPanelProps {
  draft: DashboardDraftPreview;
  nowMicros: number;
  busy: boolean;
  onPropose: () => void;
  onExecute: () => void;
}

export function DraftStatusPanel({
  draft,
  nowMicros,
  busy,
  onPropose,
  onExecute,
}: DraftStatusPanelProps) {
  const { t } = useTranslation('intelligence');
  const action = dashboardDraftAction(draft, nowMicros);
  const warnings = uniqueDraftWarnings(draft);
  const range = preflightRange(draft);
  const effectiveExpiry = effectiveDraftExpiry(draft);
  const expired = draft.status === 'expired' || effectiveExpiry <= nowMicros;
  const approved = draft.operation?.approved_reviews ?? 0;
  const required = draft.operation?.required_approvals ?? 0;

  return (
    <aside className="space-y-3" aria-label={t('dashboard_authoring.review_title')}>
      <Card>
        <CardHeader className="flex-row items-start justify-between gap-3">
          <div>
            <CardTitle>{t('dashboard_authoring.review_title')}</CardTitle>
            <p className="mt-1 text-xs leading-5 text-tx-3">
              {t('dashboard_authoring.review_description')}
            </p>
          </div>
          <DraftBadge expired={expired} status={draft.status} />
        </CardHeader>
        <CardContent className="space-y-4">
          <dl className="divide-y divide-bd-0 border-y border-bd-0 text-xs">
            <MetaRow
              label={t('dashboard_authoring.expires')}
              value={
                expired
                  ? t('dashboard_authoring.expired')
                  : remainingDuration(effectiveExpiry, nowMicros)
              }
            />
            <MetaRow
              label={t('dashboard_authoring.created_at')}
              value={formatMicrosActive(draft.created_at)}
            />
            <MetaRow
              label={t('dashboard_authoring.model_hash')}
              value={draft.model_hash.slice(0, 12)}
              mono
            />
            {range ? (
              <MetaRow
                label={t('dashboard_authoring.preflight_range')}
                value={`${formatMicrosActive(range.from, false)} — ${formatMicrosActive(range.to, false)}`}
              />
            ) : null}
          </dl>

          {draft.operation ? (
            <div className="rounded-md border border-bd-0 bg-bg-2 p-3">
              <div className="flex items-center gap-2 text-xs font-strong text-tx-1">
                <ShieldCheck className="h-4 w-4 text-blue" />
                {t('dashboard_authoring.operation_title')}
              </div>
              <p className="mt-1.5 text-xs leading-5 text-tx-3">
                {required > 0
                  ? t('dashboard_authoring.review_progress', {
                      approved,
                      required,
                    })
                  : t('dashboard_authoring.confirmation_required')}
              </p>
            </div>
          ) : null}

          <DraftAction
            action={action}
            busy={busy}
            draft={draft}
            onExecute={onExecute}
            onPropose={onPropose}
          />
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>{t('dashboard_authoring.preflight_title')}</CardTitle>
        </CardHeader>
        <CardContent className="space-y-2">
          {draft.preflight.panels.map((panel) => (
            <div
              key={panel.path}
              className="flex items-start gap-2 rounded-md border border-bd-0 bg-bg-2 p-2.5"
            >
              {panel.status === 'passed' ? (
                <CheckCircle2 className="mt-0.5 h-4 w-4 shrink-0 text-green" />
              ) : (
                <Clock3 className="mt-0.5 h-4 w-4 shrink-0 text-yellow" />
              )}
              <div className="min-w-0">
                <div className="truncate text-xs font-strong text-tx-1">
                  {panel.title}
                </div>
                <div className="mt-0.5 text-type-micro text-tx-3">
                  {t(`dashboard_authoring.preflight_status.${panel.status}`)} ·{' '}
                  {t('dashboard_authoring.preflight_rows', {
                    count: panel.returned_rows,
                  })}
                </div>
              </div>
            </div>
          ))}
          {draft.preflight.panels.length === 0 ? (
            <p className="text-xs leading-5 text-tx-3">
              {t('dashboard_authoring.no_preflight_panels')}
            </p>
          ) : null}
        </CardContent>
      </Card>

      {warnings.length > 0 ? (
        <Alert variant="warning">
          <AlertTriangle />
          <AlertTitle>{t('dashboard_authoring.warnings_title')}</AlertTitle>
          <AlertDescription>
            <ul className="mt-2 space-y-2">
              {warnings.map((warning) => (
                <li key={`${warning.code}-${warning.path}`}>
                  <span className="font-strong">{warning.message}</span>
                  <span className="mt-0.5 block font-mono text-type-micro opacity-80">
                    {warning.path || '/'} · {warning.code}
                  </span>
                </li>
              ))}
            </ul>
          </AlertDescription>
        </Alert>
      ) : null}

      {draft.preflight.issues.length > 0 ? (
        <Alert variant="destructive">
          <AlertTriangle />
          <AlertTitle>{t('dashboard_authoring.issues_title')}</AlertTitle>
          <AlertDescription>
            <ul className="mt-2 space-y-2">
              {draft.preflight.issues.map((issue) => (
                <li key={`${issue.code}-${issue.path}`}>
                  <span className="font-strong">{issue.message}</span>
                  <span className="mt-0.5 block font-mono text-type-micro opacity-80">
                    {issue.path || '/'} · {issue.code}
                  </span>
                </li>
              ))}
            </ul>
            <Button asChild variant="outline" size="sm" className="mt-3">
              <Link to="/intelligence/chat">
                {t('dashboard_authoring.retry_in_chat')}
              </Link>
            </Button>
          </AlertDescription>
        </Alert>
      ) : null}
    </aside>
  );
}

function DraftAction({
  action,
  busy,
  draft,
  onExecute,
  onPropose,
}: Pick<DraftStatusPanelProps, 'busy' | 'draft' | 'onExecute' | 'onPropose'> & {
  action: ReturnType<typeof dashboardDraftAction>;
}) {
  const { t } = useTranslation('intelligence');
  if (action === 'open') {
    return (
      <Button asChild size="lg" className="w-full">
        <Link to={draft.dashboard_route ?? '/dashboards'}>
          {t('dashboard_authoring.open_dashboard')}
          <ExternalLink />
        </Link>
      </Button>
    );
  }
  if (action === 'propose') {
    return (
      <Button size="lg" className="w-full" disabled={busy} onClick={onPropose}>
        {busy
          ? t('dashboard_authoring.submitting')
          : t('dashboard_authoring.submit_proposal')}
      </Button>
    );
  }
  if (action === 'execute') {
    return (
      <Button size="lg" className="w-full" disabled={busy} onClick={onExecute}>
        {busy
          ? t('dashboard_authoring.creating')
          : t('dashboard_authoring.confirm_create')}
      </Button>
    );
  }
  if (action === 'wait_for_review') {
    return (
      <Button asChild variant="outline" size="lg" className="w-full">
        <Link to="/intelligence/approvals">
          {t('dashboard_authoring.view_approval')}
          <ExternalLink />
        </Link>
      </Button>
    );
  }
  return (
    <Button size="lg" className="w-full" disabled>
      {t('dashboard_authoring.creation_unavailable')}
    </Button>
  );
}

function DraftBadge({
  expired,
  status,
}: {
  expired: boolean;
  status: DashboardDraftPreview['status'];
}) {
  const { t } = useTranslation('intelligence');
  const value = expired ? 'expired' : status;
  return (
    <Badge
      variant="outline"
      className={cn(
        value === 'ready' && 'border-blue/35 bg-blue-dim text-blue-soft',
        value === 'consumed' && 'border-green/35 bg-green-dim text-green-soft',
        value === 'expired' && 'border-red/35 bg-red-dim text-red-soft',
      )}
    >
      {t(`dashboard_authoring.draft_status.${value}`)}
    </Badge>
  );
}

function MetaRow({
  label,
  value,
  mono = false,
}: {
  label: string;
  value: string;
  mono?: boolean;
}) {
  return (
    <div className="grid grid-cols-[104px_minmax(0,1fr)] gap-3 py-2.5">
      <dt className="text-tx-3">{label}</dt>
      <dd className={cn('break-words text-right text-tx-1', mono && 'font-mono')}>
        {value}
      </dd>
    </div>
  );
}
