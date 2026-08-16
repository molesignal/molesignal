import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { DataTable, PageHeader } from '@/admin';
import * as auditApi from '@/api/audit';
import * as usersApi from '@/api/users';
import { type ApiError, toApiError } from '@/lib/http';
import { ProductState } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';
import { CopyIconButton } from '@/shell/CopyIconButton';
import { CursorPagination } from '@/shell/CursorPagination';
import { FormDrawer, FormField, FormInput, FormSelect } from '@/shell/FormDrawer';

import { SectionBody } from './_atoms';
import { AuditChatTranscript } from './audit/AuditChatTranscript';
import {
  auditTargetName,
  rawAuditTarget,
  resourceNameKey,
  useAuditResourceNames,
} from './audit/useAuditResourceNames';
import { formatMicros } from '../rum/_helpers';

interface Filters {
  from: string;
  to: string;
  actor_kind: string;
  actor: string;
  action: string;
  target_kind: string;
  target_id: string;
}

const EMPTY: Filters = {
  from: '',
  to: '',
  actor_kind: '',
  actor: '',
  action: '',
  target_kind: '',
  target_id: '',
};

const DEFAULT_PAGE_SIZE = 20;
const PAGE_SIZE_OPTIONS = [20, 50, 100];
// Radix Select 不接受空字符串 value，用哨兵代表「全部用户」，提交时映射回空。
const ALL_ACTORS = '__all__';

function toParams(f: Filters): auditApi.AuditQueryParams {
  return {
    from: f.from || undefined,
    to: f.to || undefined,
    actor_kind: f.actor_kind || undefined,
    actor: f.actor || undefined,
    action: f.action || undefined,
    target_kind: f.target_kind || undefined,
    target_id: f.target_id || undefined,
  };
}

export function Audit() {
  const { t } = useTranslation('settings-admin');
  const { t: tc } = useTranslation('common');
  const [draft, setDraft] = React.useState<Filters>(EMPTY);
  const [applied, setApplied] = React.useState<Filters>(EMPTY);
  const [items, setItems] = React.useState<auditApi.AuditEvent[]>([]);
  const [pageSize, setPageSize] = React.useState(DEFAULT_PAGE_SIZE);
  const [cursor, setCursor] = React.useState<string | undefined>();
  const [previousCursors, setPreviousCursors] = React.useState<
    Array<string | undefined>
  >([]);
  const [nextCursor, setNextCursor] = React.useState<string | null>(null);
  const [loading, setLoading] = React.useState(false);
  const [error, setError] = React.useState<ApiError | null>(null);
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

  const run = React.useCallback(
    async (f: Filters, pageCursor: string | undefined, limit: number) => {
      setLoading(true);
      try {
        const page = await auditApi.query({ ...toParams(f), cursor: pageCursor, limit });
        setItems(page.items);
        setNextCursor(page.next_cursor);
        setError(null);
      } catch (e) {
        setError(toApiError(e));
        setItems([]);
        setNextCursor(null);
      } finally {
        setLoading(false);
      }
    },
    [],
  );

  React.useEffect(() => {
    void run(applied, cursor, pageSize);
  }, [applied, cursor, pageSize, run]);

  const update = (k: keyof Filters) => (e: React.ChangeEvent<HTMLInputElement>) =>
    setDraft((d) => ({ ...d, [k]: e.target.value }));

  // 403 → permission-denied; otherwise empty-on-no-rows after a load.
  const permissionDenied = error?.status === 403;

  return (
    <>
      <PageHeader title={t('audit.title')} subtitle={t('audit.subtitle') as string} />
      <SectionBody>
        <form
          className="mb-3 grid grid-cols-2 gap-2 md:grid-cols-4"
          onSubmit={(e) => {
            e.preventDefault();
            setCursor(undefined);
            setPreviousCursors([]);
            setApplied({ ...draft });
          }}
        >
          <FormField label={t('audit.filters.from')}>
            <FormInput
              value={draft.from}
              onChange={update('from')}
              placeholder={t('audit.filters.placeholder_relative') as string}
            />
          </FormField>
          <FormField label={t('audit.filters.to')}>
            <FormInput
              value={draft.to}
              onChange={update('to')}
              placeholder={t('audit.filters.placeholder_relative') as string}
            />
          </FormField>
          <FormField label={t('audit.filters.action')}>
            <FormInput value={draft.action} onChange={update('action')} />
          </FormField>
          <FormField label={t('audit.filters.actor')}>
            <FormSelect
              value={draft.actor || ALL_ACTORS}
              onChange={(v) => setDraft((d) => ({ ...d, actor: v === ALL_ACTORS ? '' : v }))}
              options={[
                { value: ALL_ACTORS, label: t('audit.filters.actor_all') },
                ...(usersQ.data ?? []).map((u) => ({
                  value: u.id,
                  label: u.display_name ? `${u.display_name} (${u.email})` : u.email,
                })),
              ]}
            />
          </FormField>
          <FormField label={t('audit.filters.actor_kind')}>
            <FormInput value={draft.actor_kind} onChange={update('actor_kind')} />
          </FormField>
          <FormField label={t('audit.filters.target_kind')}>
            <FormInput value={draft.target_kind} onChange={update('target_kind')} />
          </FormField>
          <FormField label={t('audit.filters.target_id')}>
            <FormInput value={draft.target_id} onChange={update('target_id')} />
          </FormField>
          <div className="flex items-end gap-2">
            <ChromeButton type="submit" variant="primary">
              {t('audit.filters.apply')}
            </ChromeButton>
            <ChromeButton
              type="button"
              onClick={() => {
                setDraft(EMPTY);
                setApplied(EMPTY);
                setCursor(undefined);
                setPreviousCursors([]);
              }}
            >
              {t('audit.filters.reset')}
            </ChromeButton>
          </div>
        </form>

        {permissionDenied ? (
          <ProductState variant="permission-denied" />
        ) : items.length === 0 && !loading ? (
          <ProductState
            variant="empty"
            title={t('audit.empty_title')}
            description={t('audit.empty_description') as string}
          />
        ) : (
          <>
            <DataTable
              rows={items}
              rowKey={(r) => r.id}
              onRowClick={(r) => setSelected(r)}
              columns={[
                {
                  key: 'ts',
                  header: t('audit.columns.ts'),
                  cell: (r) => formatMicros(r.ts_micros),
                  width: 190,
                },
                {
                  key: 'actor',
                  header: t('audit.columns.actor'),
                  cell: (r) => auditActorName(r, usersById, resourceNames),
                  width: 200,
                },
                { key: 'action', header: t('audit.columns.action'), cell: (r) => r.action },
                {
                  key: 'target',
                  header: t('audit.columns.target'),
                  cell: (r) => auditTargetName(r, resourceNames),
                },
                {
                  key: 'status',
                  header: t('audit.columns.status'),
                  cell: (r) => String((r.payload?.status as string | undefined) ?? '—'),
                  width: 110,
                },
              ]}
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
                if (selected) void navigator.clipboard?.writeText(selected.id);
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
              <dd className="text-tx-0">{formatMicros(selected.ts_micros)}</dd>
              <dt className="text-tx-3">{t('audit.columns.actor')}</dt>
              <dd className="min-w-0 text-tx-0">
                <div>{auditActorName(selected, usersById, resourceNames)}</div>
                {auditActorName(selected, usersById, resourceNames) !== rawAuditActor(selected) && (
                  <div className="truncate font-mono text-[11px] text-tx-3">
                    {rawAuditActor(selected)}
                  </div>
                )}
              </dd>
              <dt className="text-tx-3">{t('audit.columns.action')}</dt>
              <dd className="text-tx-0">{selected.action}</dd>
              <dt className="text-tx-3">{t('audit.columns.target')}</dt>
              <dd className="min-w-0 text-tx-0">
                <div>{auditTargetName(selected, resourceNames)}</div>
                {auditTargetName(selected, resourceNames) !== rawAuditTarget(selected) && (
                  <div className="truncate font-mono text-[11px] text-tx-3">
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
            <pre className="max-h-[50vh] overflow-auto rounded-md border border-bd-0 bg-bg-2 p-3 font-mono text-xs text-tx-1">
              {JSON.stringify(selected.payload, null, 2)}
            </pre>
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
  if (event.actor_kind === 'user') {
    const user = users.get(event.actor_id);
    return user?.display_name.trim() || user?.email || rawAuditActor(event);
  }
  return (
    resourceNames.get(resourceNameKey(event.actor_kind, event.actor_id)) ??
    rawAuditActor(event)
  );
}

function rawAuditActor(event: auditApi.AuditEvent): string {
  return `${event.actor_kind}:${event.actor_id}`;
}

function auditChatId(event: auditApi.AuditEvent): string | null {
  if (event.target_kind === 'agent_chat' && event.target_id) return event.target_id;
  const payload = recordOf(event.payload);
  const direct = stringValue(payload.chat_id);
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
