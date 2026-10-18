import type { TFunction } from 'i18next';
import { Bot, CircleAlert } from 'lucide-react';
import type * as React from 'react';
import { Link } from 'react-router-dom';

import type { MessageReference } from '@/api/agent/chat';
import type { MessageCommand } from '@/api/agent/commands';
import { Avatar, AvatarFallback, AvatarImage } from '@/shell/ui/avatar';

import { type AnswerLink, useAnswerLinks } from './answerLinks';
import { ConfidenceGauge } from './ConfidenceGauge';
import { InvestigationProcess } from './InvestigationProcess';
import { MessageTime } from './MessageTime';
import { type EvidenceRef, evidenceHref, parseStructuredAnswer } from '../../answer';
import { MarkdownMessage } from '../../markdown';
import { parseInvestigationEvidence, sanitizeAssistantContent } from '../../settings/presentation';
import { ReferenceList } from '../composer/references/ReferenceChips';
import { UserMessageText } from '../composer/UserMessageText';
import type { LiveTool } from '../run/runState';

export interface ChatUserIdentity {
  /** Who is signed in; the questions of others in a team chat show their own name. */
  id?: string;
  displayName: string;
  avatarUrl: string;
}


function ThinkingIndicator({ t }: { t: (k: string) => string }) {
  return (
    <div className="flex items-center gap-3 text-tx-2">
      <div className="flex items-center gap-1">
        {[0, 1, 2].map((i) => (
          <span
            key={i}
            className="h-1.5 w-1.5 animate-pulse rounded-full bg-indigo"
            style={{ animationDelay: `${i * 120}ms` }}
          />
        ))}
      </div>
      <span className="text-shimmer">{t('thinking')}</span>
    </div>
  );
}

export function MessageBubble({
  role,
  content,
  command,
  pending = false,
  t,
  user,
  senderName,
  streaming,
  createdAtMicros,
  evidence,
  liveTools = [],
  investigationStartedAtMicros,
  references = [],
  queued = false,
  actions,
  rating,
  messageId,
}: {
  role: string;
  content: string;
  /** The command a user message was. */
  command?: MessageCommand | null | undefined;
  /** What the question carried with @. */
  references?: readonly MessageReference[] | null;
  /** The question waits its turn behind an answer being written. */
  queued?: boolean;
  /** What can be done with the message: beside a question's time, on an answer's foot. */
  actions?: React.ReactNode;
  /** How the reader rates an answer: at the right end of its foot, after its confidence. */
  rating?: React.ReactNode;
  /** The answer's id, for the pinned bar to open it where it stands. */
  messageId?: string;
  /** A question sent and not answered yet. */
  pending?: boolean;
  t: TFunction<'agent'>;
  user?: ChatUserIdentity;
  /** Who asked, when it was not the one reading. */
  senderName?: string;
  streaming?: boolean;
  createdAtMicros?: number;
  evidence?: unknown;
  liveTools?: LiveTool[];
  investigationStartedAtMicros?: number;
}) {
  const isUser = role === 'user';
  if (role === 'tool') return null;
  const displayContent = isUser ? content : sanitizeAssistantContent(content);
  const structured = !isUser ? parseStructuredAnswer(displayContent) : null;
  const persistedEvidence = parseInvestigationEvidence(evidence);
  if (isUser) {
    return (
      <div className="group/question flex flex-row-reverse gap-3" data-message-role="user">
        <div className="flex h-7 w-7 shrink-0 items-center justify-center overflow-hidden rounded-full bg-gradient-to-br from-indigo via-blue to-green text-white">
          <UserMessageAvatar user={user} />
        </div>
        <div className="flex max-w-[min(680px,84%)] min-w-0 flex-col items-end gap-1">
          {senderName && (
            <span data-testid="message-sender" className="type-micro font-strong text-tx-2">
              {senderName}
            </span>
          )}
          <div className="min-w-0 rounded-lg border border-indigo/25 bg-indigo/10 px-3 py-2.5 font-sans text-base leading-6 text-tx-0 md:text-sm">
            <UserMessageText content={displayContent} command={command} pending={pending} />
          </div>
          <ReferenceList references={references ?? []} />
          <span className="flex items-center gap-1.5 type-micro text-tx-3">
            {actions}
            {queued && (
              <span data-agent-queued className="rounded bg-bg-2 px-1.5 font-strong text-tx-2">
                {t('run.queued')}
              </span>
            )}
            {createdAtMicros !== undefined && <MessageTime micros={createdAtMicros} />}
          </span>
        </div>
      </div>
    );
  }
  return (
    <div
      className="flex max-w-[960px] scroll-mt-14 gap-3"
      data-message-role="assistant"
      data-message-id={messageId}
    >
      <div className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md border border-indigo/20 bg-indigo/10 text-indigo">
        <Bot className="h-4 w-4" />
      </div>
      <div className="min-w-0 flex-1">
        <div className="mb-2 flex items-baseline gap-2">
          <span className="text-sm font-display-strong tracking-[-0.01em] text-tx-0">
            Mole Agent
          </span>
        </div>
        <div className="min-w-0 font-sans text-base leading-7 text-tx-1 md:text-sm md:leading-6">
          <InvestigationProcess
            t={t}
            evidence={persistedEvidence}
            liveTools={liveTools}
            running={Boolean(streaming)}
            {...(investigationStartedAtMicros !== undefined
              ? { startedAtMicros: investigationStartedAtMicros }
              : {})}
            {...(!streaming && createdAtMicros !== undefined
              ? { finishedAtMicros: createdAtMicros }
              : {})}
          />
          {streaming && !displayContent.trim() ? (
            <ThinkingIndicator t={t} />
          ) : isStreamErrorContent(displayContent) ? (
            <AgentFailure content={displayContent} t={t} />
          ) : structured ? (
            <StructuredView answer={structured} t={t} />
          ) : (
            <MarkdownMessage
              content={displayContent}
              streaming={Boolean(streaming)}
              variant="answer"
            />
          )}
        </div>
        {/* What can be done with the answer on the left, its marks in line with its text; how
            sure it is and how the reader rates it on the right. */}
        {actions && (
          <div
            data-agent-answer-actions
            className="-mx-[7px] mt-1.5 flex min-h-7 flex-wrap items-center gap-0.5"
          >
            {actions}
            <span className="min-w-2 flex-1" />
            {structured?.confidence !== undefined && (
              <ConfidenceGauge value={structured.confidence} t={t} />
            )}
            {rating}
          </div>
        )}
      </div>
    </div>
  );
}


export function isStreamErrorContent(content: string): boolean {
  return /^\[error:[\s\S]*\]$/.test(content.trim());
}

function AgentFailure({
  content,
  t,
}: {
  content: string;
  t: (key: string) => string;
}) {
  const detail = content.trim().replace(/^\[error:\s*/, '').replace(/\]$/, '');
  return (
    <div>
      <p className="font-strong text-tx-0">{t('answer.failed_title')}</p>
      <p className="mt-1 text-tx-2">{t('answer.failed_description')}</p>
      <details className="mt-2 text-xs text-tx-3">
        <summary className="cursor-pointer">{t('answer.technical_error')}</summary>
        <pre className="mt-2 max-h-36 overflow-auto whitespace-pre-wrap rounded border border-bd-0 bg-bg-2 p-2 font-mono type-micro">
          {detail}
        </pre>
      </details>
    </div>
  );
}


function UserMessageAvatar({ user }: { user: ChatUserIdentity | undefined }) {
  const initial = (user?.displayName || 'M').trim()[0]?.toUpperCase() ?? 'M';
  return (
    <Avatar className="h-full w-full">
      {user?.avatarUrl && <AvatarImage src={user.avatarUrl} alt="" className="object-cover" />}
      <AvatarFallback className="bg-transparent font-sans text-xs font-bold text-white">
        {initial}
      </AvatarFallback>
    </Avatar>
  );
}

function StructuredView({
  answer,
  t,
}: {
  answer: ReturnType<typeof parseStructuredAnswer>;
  t: (k: string) => string;
}) {
  const follow = useAnswerLinks();
  if (!answer) return null;
  // The text one tone lighter than the names of the parts, which are a step larger, bold
  // and darkest, as is what matters most; what opens something is blue. Every line 25px
  // apart, the items of a list too, and 40px from one part to the next.
  return (
    <div className="flex flex-col gap-[15px] leading-[25px] text-tx-1">
      {answer.summary && (
        <Section title={t('answer.conclusion')}>
          <p>{answer.summary}</p>
        </Section>
      )}

      {answer.anomaly_points && answer.anomaly_points.length > 0 && (
        <Section title={t('answer.evidence')}>
          <ul className="space-y-0">
            {answer.anomaly_points.map((a, i) => (
              <li key={i} className="flex gap-2">
                <Bullet />
                <span className="min-w-0">
                  <span className="font-strong text-tx-0">{a.metric ?? a.stream ?? '—'}</span>
                  {a.observed && (
                    <span>
                      {' '}
                      · {a.observed}
                      {a.expected ? ` (exp ${a.expected})` : ''}
                    </span>
                  )}
                  {a.description && <span className="block">{a.description}</span>}
                </span>
              </li>
            ))}
          </ul>
        </Section>
      )}

      {answer.evidence && answer.evidence.length > 0 && (
        <Section title={t('answer.evidence')}>
          <ul className="space-y-0">
            {answer.evidence.map((ev, i) => (
              <EvidenceRow key={i} ev={ev} link={follow(evidenceHref(ev) ?? undefined)} />
            ))}
          </ul>
        </Section>
      )}

      {answer.likely_causes && answer.likely_causes.length > 0 && (
        <Section title={t('answer.likely_causes')}>
          <ol className="ml-4 list-decimal">
            {answer.likely_causes.map((c, i) => (
              <li key={i}>{c}</li>
            ))}
          </ol>
        </Section>
      )}

      {answer.limitations && answer.limitations.length > 0 && (
        <Section title={t('answer.limitations')}>
          <ul className="space-y-0">
            {answer.limitations.map((limitation, index) => (
              <li key={index} className="flex items-start gap-2">
                <CircleAlert className="mt-[5px] h-3.5 w-3.5 shrink-0 text-[var(--yellow)]" />
                <span>{limitation}</span>
              </li>
            ))}
          </ul>
        </Section>
      )}

      {answer.suggested_next_steps && answer.suggested_next_steps.length > 0 && (
        <Section title={t('answer.next_steps')}>
          <ol className="ml-4 list-decimal">
            {answer.suggested_next_steps.map((c, i) => (
              <li key={i}>{c}</li>
            ))}
          </ol>
        </Section>
      )}

      {answer.related_links && answer.related_links.length > 0 && (
        <Section title={t('answer.related_links')}>
          <ul className="flex flex-wrap gap-1.5">
            {answer.related_links.map((l, i) => {
              const link = follow(l.href ?? l.route);
              return (
                <li key={i}>
                  {link ? (
                    <AnswerAnchor
                      link={link}
                      className="inline-flex min-h-7 items-center rounded-md bg-bg-2 px-2.5 text-indigo hover:bg-bg-3"
                    >
                      {l.label}
                    </AnswerAnchor>
                  ) : (
                    <span className="inline-flex min-h-7 items-center rounded-md bg-bg-2 px-2.5">
                      {l.label}
                    </span>
                  )}
                </li>
              );
            })}
          </ul>
        </Section>
      )}

    </div>
  );
}

/** The mark before an item of a list without numbers. */
function Bullet() {
  return <span aria-hidden="true" className="mt-[10px] h-1 w-1 shrink-0 rounded-full bg-tx-3" />;
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div>
      <div className="font-sans text-md font-strong text-tx-0">{title}</div>
      {children}
    </div>
  );
}

/** A link of an answer: a page of the product in place, a web address in a new tab. */
function AnswerAnchor({
  link,
  className,
  children,
}: {
  link: AnswerLink;
  className: string;
  children: React.ReactNode;
}) {
  return link.external ? (
    <a href={link.to} target="_blank" rel="noreferrer" className={className}>
      {children}
    </a>
  ) : (
    <Link to={link.to} className={className}>
      {children}
    </Link>
  );
}

function EvidenceRow({ ev, link }: { ev: EvidenceRef; link: AnswerLink | null }) {
  const label =
    ev.label ?? ev.stream ?? ev.trace_id ?? ev.object_key ?? ev.kind ?? 'evidence';
  return (
    <li className="flex gap-2">
      <Bullet />
      <span className="min-w-0">
        {link ? (
          <AnswerAnchor link={link} className="text-indigo hover:underline">
            {label}
          </AnswerAnchor>
        ) : (
          <span>{label}</span>
        )}
        {ev.query && <span className="ml-1 font-mono">{ev.query}</span>}
      </span>
    </li>
  );
}
