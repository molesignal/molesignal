import { CircleHelp, ShieldQuestion } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { PendingConfirmation, PendingQuestion, RespondInput } from '@/api/agent/runs';
import { toApiError } from '@/lib/http';
import { ChromeButton } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { toast } from '@/shell/ui/sonner';

import { growWithText } from '../composer/autoGrow';

/** Send an answer or a decision; a failure is said and the card stays. */
function useRespond(onRespond: (input: RespondInput) => Promise<void>) {
  const [busy, setBusy] = React.useState(false);
  const send = async (input: RespondInput) => {
    setBusy(true);
    try {
      await onRespond(input);
    } catch (error) {
      toast.error(toApiError(error).message);
    } finally {
      setBusy(false);
    }
  };
  return { busy, send };
}

/**
 * What Mole Agent asks the person: the answer waits for it. A question with a few
 * likely answers offers them as choices, with room for another answer.
 */
export function QuestionCard({
  pending,
  onRespond,
}: {
  pending: PendingQuestion;
  onRespond: (input: RespondInput) => Promise<void>;
}) {
  const { t } = useTranslation('agent');
  const { busy, send } = useRespond(onRespond);
  const [picked, setPicked] = React.useState<string[]>([]);
  const [text, setText] = React.useState('');
  const answerRef = React.useRef<HTMLTextAreaElement>(null);
  const multiple = pending.kind === 'multiple';

  // The answer grows with what is written, the way the composer does.
  React.useLayoutEffect(() => growWithText(answerRef.current), [text]);
  const hasOptions = pending.options.length > 0 && pending.kind !== 'text';
  const ready = picked.length > 0 || text.trim().length > 0;

  const toggle = (option: string) =>
    setPicked((current) =>
      multiple
        ? current.includes(option)
          ? current.filter((item) => item !== option)
          : [...current, option]
        : [option],
    );

  return (
    <div
      data-agent-question={pending.call_id}
      className="ml-10 rounded-lg bg-[var(--blue-tint)] px-3 py-3"
    >
      <div className="flex items-start gap-2">
        <CircleHelp aria-hidden="true" className="mt-0.5 h-4 w-4 shrink-0 text-[var(--blue-ink)]" />
        <div className="min-w-0 flex-1">
          <p className="text-xs font-strong text-[var(--blue-ink)]">{t('run.question.title')}</p>
          <p className="mt-1 text-sm text-tx-0">{pending.question}</p>
          {hasOptions && (
            <div role={multiple ? 'group' : 'radiogroup'} className="mt-2 flex flex-wrap gap-1.5">
              {pending.options.map((option) => {
                const chosen = picked.includes(option);
                return (
                  <button
                    key={option}
                    type="button"
                    role={multiple ? 'checkbox' : 'radio'}
                    aria-checked={chosen}
                    disabled={busy}
                    onClick={() => toggle(option)}
                    className={cn(
                      'inline-flex h-8 items-center rounded-md px-2.5 text-xs font-strong',
                      chosen ? 'bg-indigo text-white' : 'bg-bg-1 text-tx-1 hover:bg-bg-2',
                    )}
                  >
                    {option}
                  </button>
                );
              })}
            </div>
          )}
          <form
            className="mt-2 flex items-end gap-2"
            onSubmit={(event) => {
              event.preventDefault();
              if (!ready || busy) return;
              void send({ call_id: pending.call_id, choices: picked, text: text.trim() });
            }}
          >
            <textarea
              ref={answerRef}
              aria-label={hasOptions ? t('run.question.other') : t('run.question.placeholder')}
              placeholder={hasOptions ? t('run.question.other') : t('run.question.placeholder')}
              value={text}
              rows={1}
              disabled={busy}
              onChange={(event) => setText(event.target.value)}
              className="block min-h-8 min-w-0 flex-1 resize-none overflow-y-hidden rounded-md bg-bg-1 px-2.5 py-1.5 text-xs leading-5 text-tx-0 placeholder:text-tx-3"
            />
            <ChromeButton type="submit" size="sm" variant="primary" disabled={!ready || busy}>
              {t('run.question.send')}
            </ChromeButton>
          </form>
        </div>
      </div>
    </div>
  );
}

/** A few words on what a tool is about to change, from its arguments. */
function describeArguments(args: Record<string, unknown>): string[] {
  const lines: string[] = [];
  for (const key of ['summary', 'path', 'team', 'name']) {
    const value = args[key];
    if (typeof value === 'string' && value.trim()) lines.push(value.trim());
  }
  if (lines.length > 0) return lines;
  return Object.entries(args)
    .filter(([, value]) => ['string', 'number', 'boolean'].includes(typeof value))
    .slice(0, 4)
    .map(([key, value]) => `${key}: ${String(value)}`);
}

/**
 * A tool in confirmation mode waits for the person who asked: the tool runs as
 * them, so only they decide. Everyone else in a team chat sees whose turn it is.
 */
export function ConfirmationCard({
  pending,
  canDecide,
  initiatorName,
  toolLabel,
  onRespond,
}: {
  pending: PendingConfirmation;
  canDecide: boolean;
  initiatorName: string;
  toolLabel: string;
  onRespond: (input: RespondInput) => Promise<void>;
}) {
  const { t } = useTranslation('agent');
  const { busy, send } = useRespond(onRespond);
  const preview =
    typeof pending.arguments.preview === 'string' ? pending.arguments.preview : null;
  const deleting = pending.arguments.delete === true;

  return (
    <div
      data-agent-confirmation={pending.call_id}
      className="ml-10 rounded-lg bg-[var(--yellow-tint)] px-3 py-3"
    >
      <div className="flex items-start gap-2">
        <ShieldQuestion aria-hidden="true" className="mt-0.5 h-4 w-4 shrink-0 text-[var(--yellow-ink)]" />
        <div className="min-w-0 flex-1">
          <p className="text-xs font-strong text-[var(--yellow-ink)]">
            {t(deleting ? 'run.confirmation.delete_title' : 'run.confirmation.title', {
              tool: toolLabel,
            })}
          </p>
          <ul className="mt-1 space-y-0.5 text-sm text-tx-0">
            {describeArguments(pending.arguments).map((line) => (
              <li key={line} className="break-words">
                {line}
              </li>
            ))}
          </ul>
          {preview && (
            <pre className="mt-2 max-h-40 overflow-auto whitespace-pre-wrap break-words rounded-md bg-bg-1 px-2.5 py-2 font-mono type-micro text-tx-2">
              {preview}
            </pre>
          )}
          {canDecide ? (
            <div className="mt-2 flex flex-wrap gap-2">
              <ChromeButton
                size="sm"
                variant="primary"
                disabled={busy}
                onClick={() => void send({ call_id: pending.call_id, approved: true })}
              >
                {t('run.confirmation.approve')}
              </ChromeButton>
              <ChromeButton
                size="sm"
                disabled={busy}
                onClick={() => void send({ call_id: pending.call_id, approved: false })}
              >
                {t('run.confirmation.decline')}
              </ChromeButton>
            </div>
          ) : (
            <p className="mt-2 text-xs text-tx-2">
              {t('run.confirmation.waiting', { name: initiatorName })}
            </p>
          )}
        </div>
      </div>
    </div>
  );
}
