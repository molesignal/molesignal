import type { AuditChatTranscript as Transcript } from '@/api/audit';
import type { ApiError } from '@/lib/http';
import { MarkdownMessage } from '@/routes/agent/markdown';
import { cn } from '@/shell/lib/cn';

import { formatMicros } from '../../rum/_helpers';

interface Props {
  transcript: Transcript | undefined;
  loading: boolean;
  error: ApiError | null;
  t: (key: string) => string;
}

export function AuditChatTranscript({ transcript, loading, error, t }: Props) {
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
            <dd className="truncate text-tx-0">
              {transcript.chat.archive_object_key ?? '—'}
            </dd>
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
  message: Transcript['messages'][number];
  t: (key: string) => string;
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
            {t('audit.chat.tokens')}: {message.prompt_tokens ?? 0}/
            {message.completion_tokens ?? 0}
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

function roleLabel(role: string, t: (key: string) => string): string {
  if (role === 'user') return t('audit.chat.roles.user');
  if (role === 'assistant') return t('audit.chat.roles.assistant');
  if (role === 'tool') return t('audit.chat.roles.tool');
  if (role === 'system') return t('audit.chat.roles.system');
  return role;
}
