import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { DataTable, PageHeader } from '@/admin';
import * as auditApi from '@/api/audit';
import * as usersApi from '@/api/users';
import { type ApiError, toApiError } from '@/lib/http';
import { ProductState } from '@/product/states';
import { MarkdownMessage } from '@/routes/intelligence/markdown';
import { ChromeButton } from '@/shell/chrome';
import { CopyIconButton } from '@/shell/CopyIconButton';
import { FormDrawer, FormField, FormInput, FormSelect } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';

import { SectionBody } from './_atoms';
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

const PAGE_SIZE = 50;
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
  const [nextCursor, setNextCursor] = React.useState<string | null>(null);
  const [loading, setLoading] = React.useState(false);
  const [error, setError] = React.useState<ApiError | null>(null);
  const [selected, setSelected] = React.useState<auditApi.AuditEvent | null>(null);
  const selectedChatId = React.useMemo(
    () => (selected ? auditChatId(selected) : null),
    [selected],
  );
  const chatTranscriptQ = useQuery({
    queryKey: ['audit', 'intelligence-chat-transcript', selectedChatId],
    queryFn: () => auditApi.getIntelligenceChatTranscript(selectedChatId as string),
    enabled: !!selectedChatId,
    retry: false,
  });
  const usersQ = useQuery({ queryKey: ['audit-actors'], queryFn: () => usersApi.list() });

  const run = React.useCallback(
    async (f: Filters, cursor: string | undefined, append: boolean) => {
      setLoading(true);
      try {
        const page = await auditApi.query({ ...toParams(f), cursor, limit: PAGE_SIZE });
        setItems((prev) => (append ? [...prev, ...page.items] : page.items));
        setNextCursor(page.next_cursor);
        setError(null);
      } catch (e) {
        setError(toApiError(e));
        if (!append) setItems([]);
      } finally {
        setLoading(false);
      }
    },
    [],
  );

  React.useEffect(() => {
    void run(applied, undefined, false);
  }, [applied, run]);

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
                  cell: (r) => `${r.actor_kind}:${r.actor_id}`,
                  width: 200,
                },
                { key: 'action', header: t('audit.columns.action'), cell: (r) => r.action },
                {
                  key: 'target',
                  header: t('audit.columns.target'),
                  cell: (r) =>
                    r.target_kind ? `${r.target_kind}${r.target_id ? `/${r.target_id}` : ''}` : '—',
                },
                {
                  key: 'status',
                  header: t('audit.columns.status'),
                  cell: (r) => String((r.payload?.status as string | undefined) ?? '—'),
                  width: 110,
                },
              ]}
            />
            {nextCursor && (
              <div className="mt-3 flex justify-center">
                <ChromeButton
                  type="button"
                  disabled={loading}
                  onClick={() => void run(applied, nextCursor, true)}
                >
                  {t('audit.load_more')}
                </ChromeButton>
              </div>
            )}
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
              <dd className="text-tx-0">{`${selected.actor_kind}:${selected.actor_id}`}</dd>
              <dt className="text-tx-3">{t('audit.columns.action')}</dt>
              <dd className="text-tx-0">{selected.action}</dd>
              <dt className="text-tx-3">{t('audit.columns.target')}</dt>
              <dd className="text-tx-0">
                {selected.target_kind
                  ? `${selected.target_kind}${selected.target_id ? `/${selected.target_id}` : ''}`
                  : '—'}
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

function auditChatId(event: auditApi.AuditEvent): string | null {
  if (event.target_kind === 'intelligence_chat' && event.target_id) return event.target_id;
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

function AuditChatTranscript({
  transcript,
  loading,
  error,
  t,
}: {
  transcript: auditApi.AuditChatTranscript | undefined;
  loading: boolean;
  error: ApiError | null;
  t: (k: string) => string;
}) {
  return (
    <section className="rounded-md border border-bd-0 bg-bg-1">
      <div className="flex items-center justify-between border-b border-bd-0 px-3 py-2">
        <div>
          <div className="font-sans text-xs font-display-strong text-tx-0">
            {t('audit.chat.title')}
          </div>
          {transcript && (
            <div className="mt-0.5 font-sans text-xs text-tx-3">
              {transcript.chat.provider} · {transcript.chat.model}
            </div>
          )}
        </div>
        {transcript?.chat.deleted_at_micros && (
          <span className="rounded border border-bd-0 bg-bg-2 px-2 py-0.5 font-sans text-xs text-tx-3">
            {t('audit.chat.deleted')}
          </span>
        )}
      </div>

      {loading ? (
        <div className="p-3 font-sans text-xs text-tx-2">{t('audit.chat.loading')}</div>
      ) : error ? (
        <div className="p-3 font-sans text-xs text-red-soft">
          {t('audit.chat.load_failed')}: {error.message}
        </div>
      ) : transcript ? (
        <div className="space-y-3 p-3">
          <dl className="grid grid-cols-[120px_minmax(0,1fr)_120px_minmax(0,1fr)] gap-x-3 gap-y-1 font-sans text-xs">
            <dt className="text-tx-3">{t('audit.chat.chat')}</dt>
            <dd className="truncate text-tx-0">{transcript.chat.id}</dd>
            <dt className="text-tx-3">{t('audit.chat.updated')}</dt>
            <dd className="text-tx-0">{formatMicros(transcript.chat.updated_at_micros)}</dd>
            <dt className="text-tx-3">{t('audit.chat.mode')}</dt>
            <dd className="text-tx-0">{transcript.chat.analysis_mode ?? '—'}</dd>
            <dt className="text-tx-3">{t('audit.chat.archive')}</dt>
            <dd className="truncate text-tx-0">{transcript.chat.archive_object_key ?? '—'}</dd>
          </dl>

          {transcript.messages.length === 0 ? (
            <div className="rounded-md border border-bd-0 bg-bg-2 p-3 font-sans text-xs text-tx-2">
              {t('audit.chat.empty')}
            </div>
          ) : (
            <div className="max-h-[44vh] space-y-2 overflow-auto pr-1">
              {transcript.messages.map((message) => (
                <TranscriptMessage key={message.id} message={message} t={t} />
              ))}
            </div>
          )}
        </div>
      ) : null}
    </section>
  );
}

function TranscriptMessage({
  message,
  t,
}: {
  message: auditApi.AuditChatTranscript['messages'][number];
  t: (k: string) => string;
}) {
  const isUser = message.role === 'user';
  const role = roleLabel(message.role, t);
  const evidence = message.evidence_json;
  const hasMetadata =
    evidence ||
    message.prompt_template_id ||
    message.prompt_builtin_key ||
    message.prompt_hash ||
    message.prompt_tokens ||
    message.completion_tokens ||
    message.cost_usd;

  return (
    <article
      className={cn(
        'rounded-md border p-3 font-sans text-xs',
        isUser ? 'border-bd-1 bg-bg-2' : 'border-bd-0 bg-bg-1',
      )}
    >
      <div className="mb-2 flex flex-wrap items-center gap-2 text-xs">
        <span className="font-display-strong text-tx-0">{role}</span>
        <span className="text-tx-3">{formatMicros(message.created_at_micros)}</span>
        {(message.prompt_tokens || message.completion_tokens) && (
          <span className="ml-auto text-tx-3">
            {t('audit.chat.tokens')}: {message.prompt_tokens ?? 0}/{message.completion_tokens ?? 0}
          </span>
        )}
      </div>
      <MarkdownMessage content={message.content || '—'} />
      {hasMetadata && (
        <details className="mt-2 rounded border border-bd-0 bg-bg-2 px-2 py-1">
          <summary className="cursor-pointer font-sans text-xs text-tx-3">
            {t('audit.chat.metadata')}
          </summary>
          <pre className="mt-2 max-h-40 overflow-auto font-mono text-xs text-tx-2">
            {JSON.stringify(
              {
                prompt_template_id: message.prompt_template_id,
                prompt_builtin_key: message.prompt_builtin_key,
                prompt_version: message.prompt_version,
                prompt_hash: message.prompt_hash,
                evidence_json: evidence,
                prompt_tokens: message.prompt_tokens,
                completion_tokens: message.completion_tokens,
                cost_usd: message.cost_usd,
              },
              null,
              2,
            )}
          </pre>
        </details>
      )}
    </article>
  );
}

function roleLabel(role: string, t: (k: string) => string): string {
  if (role === 'user') return t('audit.chat.roles.user');
  if (role === 'assistant') return t('audit.chat.roles.assistant');
  if (role === 'tool') return t('audit.chat.roles.tool');
  if (role === 'system') return t('audit.chat.roles.system');
  return role;
}
