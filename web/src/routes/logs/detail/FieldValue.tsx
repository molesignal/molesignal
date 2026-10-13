import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';
import { detectSignalTypeForLabel, SignalReference, type SignalReferenceTime } from '@/shell/SignalReference';

import { stringLabelsFromRecord } from '../relatedSignals';
import { leafFieldName, resolveLogFieldJump } from './fieldJump';
import { fieldValueText } from './model';

export type LogFieldJumpHandler = (to: string) => void;

interface LogFieldValueProps {
  field: string;
  value: unknown;
  record: Record<string, unknown>;
  /** When the log happened: a signal the value names is opened around that moment. */
  time?: SignalReferenceTime | undefined;
  onJump?: LogFieldJumpHandler | undefined;
  className?: string | undefined;
}

/**
 * A field's value. One that names a signal (a trace, a span, a service, a
 * host) is the same reference it is everywhere else in the product, with the
 * same ways on from it; one that names something with a page of its own leads
 * there; the rest is text.
 */
export function LogFieldValue({ field, value, record, time, onJump, className }: LogFieldValueProps) {
  const { t } = useTranslation('logs');
  const display = value === undefined ? 'undefined' : fieldValueText(value);
  const present = value !== null && value !== undefined && value !== '';
  const signalType = present
    ? detectSignalTypeForLabel(field) ?? detectSignalTypeForLabel(leafFieldName(field))
    : null;

  if (signalType) {
    return (
      <SignalReference
        type={signalType}
        value={String(value)}
        labelName={field}
        labels={stringLabelsFromRecord(record)}
        time={time}
        className={cn('max-w-full break-all text-left', className)}
      >
        {display}
      </SignalReference>
    );
  }

  const jump = onJump ? resolveLogFieldJump(record, field, value) : null;
  if (!jump || !onJump) {
    return <span className={cn('whitespace-pre-wrap break-all', className)}>{display}</span>;
  }

  return (
    <button
      type="button"
      onClick={(event) => {
        event.stopPropagation();
        onJump(jump.to);
      }}
      className={cn(
        'inline whitespace-pre-wrap break-all text-left underline decoration-current/40 underline-offset-2 hover:decoration-current focus-visible:bg-indigo-dim',
        className,
      )}
      title={t(jump.titleKey)}
    >
      {display}
    </button>
  );
}
