import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { DataTable, type DataTableColumn, PageHeader, TableExportButton } from '@/admin';
import * as auditApi from '@/api/audit';
import * as usersApi from '@/api/users';
import { writeClipboardText } from '@/lib/clipboard';
import { type ApiError, toApiError } from '@/lib/http';
import { ProductState } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';
import { CopyIconButton } from '@/shell/CopyIconButton';
import { CursorPagination } from '@/shell/CursorPagination';
import { FormDrawer } from '@/shell/FormDrawer';
import { JsonView } from '@/shell/JsonEditor';
import { StatusWord } from '@/shell/StatusWord';
import { ViewTabs } from '@/shell/tabs/ViewTabs';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/shell/ui/tooltip';

import { SectionBody } from './_atoms';
import { AuditChatTranscript } from './audit/AuditChatTranscript';
import {
  auditQueryOf,
  AuditFilters,
  type AuditFilterValues,
  EMPTY_AUDIT_FILTERS,
} from './audit/AuditFilters';
import { auditStatusTone, distinctValues, humanAuditAction, isAccessAction } from './audit/humanAction';
import {
  auditTargetName,
  rawAuditTarget,
  resourceNameKey,
  useAuditResourceNames,
} from './audit/useAuditResourceNames';
import { usePlatformAction } from './General/platformAction';
import { formatMicros } from '../rum/_helpers';

const DEFAULT_PAGE_SIZE = 20;
const PAGE_SIZE_OPTIONS = [20, 50, 100];
/** How long typing in a filter waits before the trail is asked again. */
const FILTER_SETTLE_MS = 300;
/** How many recent records the filters' lists are read from. */
const FACET_SAMPLE = 200;

/**
 * Retention is a platform setting. Whoever can enter the platform organization
 * (an owner, a platform administrator) is offered the way there; nobody else
 * sees the action.
 */
function ChangeRetention() {
  const { t } = useTranslation('settings-admin');
  const navigate = useNavigate();
  const retention = usePlatformAction('sys.settings.retention.read');

  if (retention.reach !== 'platform' && retention.reach !== 'here') return null;

  const button = (
    <ChromeButton
      disabled={retention.entering}
      onClick={() => void retention.run(() => navigate('/settings/audit_retention'))}
    >
      {t('audit.change_retention')}
    </ChromeButton>
  );

  if (retention.reach === 'here') return button;

  return (
    <Tooltip>
      <TooltipTrigger asChild>{button}</TooltipTrigger>
      <TooltipContent>{t('audit.change_retention_hint')}</TooltipContent>
    </Tooltip>
  );
}

export function Audit() {
  const { t } = useTranslation('settings-admin');
  const { t: tc } = useTranslation('common');
  const [filters, setFilters] = React.useState<AuditFilterValues>(EMPTY_AUDIT_FILTERS);
  const [applied, setApplied] = React.useState<AuditFilterValues>(EMPTY_AUDIT_FILTERS);
  const [view, setView] = React.useState<'all' | 'access'>('all');
  const [items, setItems] = React.useState<auditApi.AuditEvent[]>([]);
  const [pageSize, setPageSize] = React.useState(DEFAULT_PAGE_SIZE);
  const [cursor, setCursor] = React.useState<string | undefined>();
  const [previousCursors, setPreviousCursors] = React.useState<
    Array<string | undefined>
  >([]);
  const [nextCursor, setNextCursor] = React.useState<string | null>(null);
  const [loading, setLoading] = React.useState(false);
  const [error, setError] = React.useState<ApiError | null>(null);
  const [retentionDays, setRetentionDays] = React.useState<number | null>(null);
  const [selected, setSelected] = React.useState<auditApi.AuditEvent | null>(null);
  const selectedChatId = React.useMemo(
    () => (selected ? auditChatId(selected) : null),
    [selected],
  );
  const chatTranscriptQ = useQuery({
    queryKey: ['audit', 'agent-chat-transcript', selectedChatId],
    queryFn: () => auditApi.getAgentChatTranscript(selectedChatId as string),
    enabled: !!selectedChatId,
    retry: false,
  });
  const usersQ = useQuery({ queryKey: ['audit-actors'], queryFn: () => usersApi.list() });
  const usersById = React.useMemo(
    () => new Map((usersQ.data ?? []).map((user) => [user.id, user])),
    [usersQ.data],
  );
  const resourceNames = useAuditResourceNames(items);

  const facets = useQuery({
    queryKey: ['audit', 'facets'],
    queryFn: () => auditApi.query({ limit: FACET_SAMPLE }),
    staleTime: 60_000,
  });

  // Filters apply as they are set, so an answer can arrive after a newer question: only the last one counts.
  const lastRun = React.useRef(0);
  const run = React.useCallback(
    async (f: AuditFilterValues, pageCursor: string | undefined, limit: number) => {
      lastRun.current += 1;
      const thisRun = lastRun.current;

      setLoading(true);
      try {
        const page = await auditApi.query({ ...auditQueryOf(f), cursor: pageCursor, limit });
        if (thisRun !== lastRun.current) return;

        setItems(page.items);
        setNextCursor(page.nextCursor);
        setRetentionDays(page.retentionDays);
        setError(null);
      } catch (e) {
        if (thisRun !== lastRun.current) return;

        setError(toApiError(e));
        setItems([]);
        setNextCursor(null);
      } finally {
        if (thisRun === lastRun.current) setLoading(false);
      }
    },
    [],
  );

  React.useEffect(() => {
    void run(applied, cursor, pageSize);
  }, [applied, cursor, pageSize, run]);

  // A filter takes effect as it is set; what is typed waits for the typing to stop.
  React.useEffect(() => {
    const settled = window.setTimeout(() => {
      setApplied(filters);
      setCursor(undefined);
      setPreviousCursors([]);
    }, FILTER_SETTLE_MS);

    return () => window.clearTimeout(settled);
  }, [filters]);

  const sample = React.useMemo(() => [...(facets.data?.items ?? []), ...items], [facets.data, items]);
  const shownItems = view === 'access' ? items.filter((event) => isAccessAction(event.action)) : items;
  const statusOf = (event: auditApi.AuditEvent) => String((event.payload?.status as string | undefined) ?? '');
  const columns: DataTableColumn<auditApi.AuditEvent>[] = [
    {
      key: 'ts',
      header: t('audit.columns.ts'),
      value: (r) => formatMicros(r.ts),
      cell: (r) => formatMicros(r.ts),
      width: 190,
    },
    {
      key: 'actor',
      header: t('audit.columns.actor'),
      value: (r) => auditActorName(r, usersById, resourceNames),
      cell: (r) => auditActorName(r, usersById, resourceNames),
      width: 200,
    },
    {
      key: 'action',
      header: t('audit.columns.action'),
      value: (r) => r.action,
      cell: (r) => <span title={r.action}>{humanAuditAction(r.action, t)}</span>,
    },
    {
      key: 'target',
      header: t('audit.columns.target'),
      value: (r) => auditTargetName(r, resourceNames),
      cell: (r) => auditTargetName(r, resourceNames),
    },
    {
      key: 'status',
      header: t('audit.columns.status'),
      value: statusOf,
      cell: (r) => {
        const status = statusOf(r);
        if (!status) return '—';
        const tone = auditStatusTone(status);
        return (
          <StatusWord tone={tone} strong={tone === 'red'}>
            {status}
          </StatusWord>
        );
      },
      width: 110,
    },
  ];

  // 403 → permission-denied; otherwise empty-on-no-rows after a load.
  const permissionDenied = error?.status === 403;
  // How long records are kept is said once the first page has told.
  const subtitle =
    retentionDays === null || retentionDays === undefined
      ? t('audit.subtitle')
      : retentionDays === 0
        ? t('audit.subtitle_retention_forever')
        : t('audit.subtitle_retention_days', { count: retentionDays });

  return (
    <>
      <PageHeader
        title={t('audit.title')}
        subtitle={subtitle}
        actions={<ChangeRetention />}
      />
      <SectionBody>
        <AuditFilters
          value={filters}
          onChange={setFilters}
          users={usersQ.data ?? []}
          actions={distinctValues(sample.map((event) => event.action))}
          actorKinds={distinctValues(sample.map((event) => event.actorKind))}
          targetKinds={distinctValues(sample.map((event) => event.targetKind))}
        />
        <div className="mb-2 flex items-center gap-3">
          <ViewTabs<'all' | 'access'>
            label={t('audit.views_label')}
            value={view}
            onValueChange={setView}
            items={[
              { key: 'all', label: t('audit.views.all') },
              { key: 'access', label: t('audit.views.access') },
            ]}
          />
          {view === 'access' && <span className="font-sans text-xs text-tx-3">{t('audit.views_page_hint')}</span>}
          <TableExportButton name="molesignal-audit" columns={columns} rows={shownItems} className="ml-auto" />
        </div>

        {permissionDenied ? (
          <ProductState variant="permission-denied" />
        ) : error ? (
          <ProductState
            variant="error"
            title={t('audit.load_error')}
            error={error}
            onRetry={() => void run(applied, cursor, pageSize)}
          />
        ) : shownItems.length === 0 && !loading ? (
          <ProductState
            variant="empty"
            title={t('audit.empty_title')}
            description={t('audit.empty_description') as string}
          />
        ) : (
          <>
            <DataTable
              rows={shownItems}
              rowKey={(r) => r.id}
              loading={loading && items.length === 0}
              onRowClick={(r) => setSelected(r)}
              columns={columns}
            />
            <CursorPagination
              pageSize={pageSize}
              pageSizeOptions={PAGE_SIZE_OPTIONS}
              hasPrevious={previousCursors.length > 0}
              hasNext={Boolean(nextCursor)}
              pending={loading}
              ariaLabel={t('audit.pagination.aria')}
              pageSizeAriaLabel={t('audit.pagination.page_size')}
              previousLabel={t('audit.pagination.previous')}
              nextLabel={t('audit.pagination.next')}
              onPrevious={() => {
                const previous = previousCursors[previousCursors.length - 1];
                setPreviousCursors((value) => value.slice(0, -1));
                setCursor(previous);
                setSelected(null);
              }}
              onNext={() => {
                if (!nextCursor) return;
                setPreviousCursors((value) => [...value, cursor]);
                setCursor(nextCursor);
                setSelected(null);
              }}
              onPageSizeChange={(value) => {
                setPageSize(value);
                setCursor(undefined);
                setPreviousCursors([]);
                setSelected(null);
              }}
            />
          </>
        )}
      </SectionBody>

      <FormDrawer
        open={selected !== null}
        onOpenChange={(v) => !v && setSelected(null)}
        title={t('audit.detail_title')}
        width={selectedChatId ? 960 : 720}
        footer={
          <div className="flex justify-end gap-2">
            <CopyIconButton
              type="button"
              label={t('audit.copy_id')}
              onClick={() => {
                if (selected) void writeClipboardText(selected.id);
              }}
            />
            <ChromeButton type="button" variant="primary" onClick={() => setSelected(null)}>
              {tc('actions.close')}
            </ChromeButton>
          </div>
        }
      >
        {selected && (
          <div className="space-y-3">
            <dl className="grid grid-cols-[120px_minmax(0,1fr)] gap-1 font-sans text-xs">
              <dt className="text-tx-3">{t('audit.columns.ts')}</dt>
              <dd className="text-tx-0">{formatMicros(selected.ts)}</dd>
              <dt className="text-tx-3">{t('audit.columns.actor')}</dt>
              <dd className="min-w-0 text-tx-0">
                <div>{auditActorName(selected, usersById, resourceNames)}</div>
                {auditActorName(selected, usersById, resourceNames) !== rawAuditActor(selected) && (
                  <div className="truncate type-micro font-mono text-tx-3">
                    {rawAuditActor(selected)}
                  </div>
                )}
              </dd>
              <dt className="text-tx-3">{t('audit.columns.action')}</dt>
              <dd className="min-w-0 text-tx-0">
                <div>{humanAuditAction(selected.action, t)}</div>
                <div className="truncate type-micro font-mono text-tx-3">{selected.action}</div>
              </dd>
              <dt className="text-tx-3">{t('audit.columns.target')}</dt>
              <dd className="min-w-0 text-tx-0">
                <div>{auditTargetName(selected, resourceNames)}</div>
                {auditTargetName(selected, resourceNames) !== rawAuditTarget(selected) && (
                  <div className="truncate type-micro font-mono text-tx-3">
                    {rawAuditTarget(selected)}
                  </div>
                )}
              </dd>
            </dl>
            {selectedChatId && (
              <AuditChatTranscript
                transcript={chatTranscriptQ.data}
                loading={chatTranscriptQ.isLoading}
                error={chatTranscriptQ.error ? toApiError(chatTranscriptQ.error) : null}
                t={t}
              />
            )}
            <JsonView value={selected.payload} ariaLabel={t('audit.detail_title')} maxHeight={480} />
          </div>
        )}
      </FormDrawer>
    </>
  );
}

function auditActorName(
  event: auditApi.AuditEvent,
  users: ReadonlyMap<string, usersApi.UserView>,
  resourceNames: ReadonlyMap<string, string>,
): string {
  if (event.actorKind === 'user') {
    const user = users.get(event.actorId);
    return user?.displayName.trim() || user?.email || rawAuditActor(event);
  }
  return (
    resourceNames.get(resourceNameKey(event.actorKind, event.actorId)) ??
    rawAuditActor(event)
  );
}

function rawAuditActor(event: auditApi.AuditEvent): string {
  // Nobody signed in: a sign-in limit reached by an unknown visitor has no actor id.
  return event.actorId ? `${event.actorKind}:${event.actorId}` : event.actorKind;
}

function auditChatId(event: auditApi.AuditEvent): string | null {
  if (event.targetKind === 'agent_chat' && event.targetId) return event.targetId;
  const payload = recordOf(event.payload);
  const direct = stringValue(payload.chatId);
  if (direct) return direct;
  const chat = recordOf(payload.chat);
  return stringValue(chat.id);
}

function recordOf(value: unknown): Record<string, unknown> {
  return value && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}

function stringValue(value: unknown): string | null {
  return typeof value === 'string' && value.trim() ? value : null;
}
