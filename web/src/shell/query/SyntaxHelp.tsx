import { HelpCircle } from 'lucide-react';
import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';
import { Button } from '@/shell/ui/button';
import { Popover, PopoverContent, PopoverTrigger } from '@/shell/ui/popover';

type QuerySyntaxMode = 'fields' | 'sql' | 'promql';
type QuerySyntaxScope = 'logs' | 'traces' | 'metrics';
type FieldSqlScope = Exclude<QuerySyntaxScope, 'metrics'>;

interface QuerySyntaxHelpProps {
  mode: QuerySyntaxMode;
  scope: QuerySyntaxScope;
  className?: string | undefined;
  contentClassName?: string | undefined;
  ariaLabel?: string | undefined;
  triggerTitle?: string | undefined;
  title?: string | undefined;
  description?: string | undefined;
  examples?: SyntaxExample[] | undefined;
  footer?: ReactNode;
  compact?: boolean | undefined;
}

interface SyntaxExample {
  label: string;
  expression: string;
  description?: string | undefined;
}

interface SyntaxExampleLabels {
  equals: string;
  contains: string;
  substring: string;
  fullText: string;
  basic: string;
  filter: string;
  aggregate: string;
  sort: string;
}

function defaultExamples(
  mode: QuerySyntaxMode,
  scope: QuerySyntaxScope,
  labels: SyntaxExampleLabels,
): SyntaxExample[] {
  if (scope === 'metrics') return [];
  const fieldExamples: Record<FieldSqlScope, SyntaxExample[]> = {
    logs: [
      { label: labels.equals, expression: "service = 'checkout'" },
      { label: labels.contains, expression: "message contains 'timeout'" },
      { label: labels.substring, expression: "MATCH(message, 'timeout')" },
      { label: labels.fullText, expression: "MATCH_TEXT(message, 'timeout disk')" },
    ],
    traces: [
      { label: labels.equals, expression: "trace_id = '4bf92f3577b34da6a3ce929d0e0e4736'" },
      { label: labels.contains, expression: "service_name contains 'checkout'" },
      { label: labels.filter, expression: "status_code = 'ERROR'" },
      { label: labels.filter, expression: "duration_ms > 500" },
    ],
  };
  const sqlExamples: Record<FieldSqlScope, SyntaxExample[]> = {
    logs: [
      { label: labels.basic, expression: `SELECT * FROM "app_logs" ORDER BY _timestamp DESC LIMIT 200` },
      { label: labels.filter, expression: `WHERE "level" = 'error'` },
      { label: labels.substring, expression: `WHERE MATCH(message, 'timeout')` },
      { label: labels.fullText, expression: `WHERE MATCH_TEXT(message, 'timeout disk')` },
    ],
    traces: [
      { label: labels.basic, expression: `SELECT * FROM traces LIMIT 200` },
      { label: labels.filter, expression: `WHERE "service_name" = 'checkout'` },
      { label: labels.aggregate, expression: `GROUP BY trace_id` },
      { label: labels.sort, expression: `ORDER BY duration_ms DESC LIMIT 50` },
    ],
  };
  return mode === 'sql' ? sqlExamples[scope] : fieldExamples[scope];
}

export function QuerySyntaxHelp({
  mode,
  scope,
  className,
  contentClassName,
  ariaLabel,
  triggerTitle,
  title,
  description,
  examples: suppliedExamples,
  footer,
  compact = false,
}: QuerySyntaxHelpProps) {
  const { t } = useTranslation('common');
  const modeLabel = t(`query_syntax_help.modes.${mode}`);
  const labels: SyntaxExampleLabels = {
    equals: t('query_syntax_help.labels.equals'),
    contains: t('query_syntax_help.labels.contains'),
    substring: t('query_syntax_help.labels.substring'),
    fullText: t('query_syntax_help.labels.full_text'),
    basic: t('query_syntax_help.labels.basic'),
    filter: t('query_syntax_help.labels.filter'),
    aggregate: t('query_syntax_help.labels.aggregate'),
    sort: t('query_syntax_help.labels.sort'),
  };
  const examples = suppliedExamples ?? defaultExamples(mode, scope, labels);
  const resolvedAriaLabel = ariaLabel ?? t('query_syntax_help.aria');
  const resolvedTriggerTitle = triggerTitle ?? t('query_syntax_help.trigger');
  const resolvedTitle = title ?? t('query_syntax_help.title');
  const resolvedDescription = description
    ?? t('query_syntax_help.description', { mode: modeLabel });

  return (
    <Popover>
      <PopoverTrigger asChild>
        <Button
          type="button"
          variant="outline"
          size="icon"
          className={cn(
            'h-[26px] w-[26px] rounded-md border-bd-1 bg-bg-2 text-tx-2 hover:bg-bg-3 hover:text-tx-0',
            className,
          )}
          aria-label={resolvedAriaLabel}
          title={resolvedTriggerTitle}
        >
          <HelpCircle className="h-3.5 w-3.5" />
        </Button>
      </PopoverTrigger>
      <PopoverContent
        side="bottom"
        align="start"
        className={cn(
          compact
            ? 'w-[360px] max-w-[calc(100vw-2rem)] overflow-hidden rounded-md border-bd-1 bg-bg-0 p-0 text-tx-1 shadow-drawer'
            : 'w-[560px] overflow-hidden rounded-md border-bd-1 bg-bg-0 p-0 text-tx-1 shadow-drawer',
          contentClassName,
        )}
      >
        {compact ? (
          <>
            <div className="flex items-center justify-between gap-3 bg-bg-1 px-3 py-2.5">
              <div className="font-sans text-sm font-bold text-tx-0">{resolvedTitle}</div>
              <div className="shrink-0 font-sans text-xs text-tx-3">
                {t('query_syntax_help.mode_shortcut', { mode: modeLabel })}
              </div>
            </div>
            <div className="space-y-1 px-3 py-2.5 font-sans text-xs">
              {examples.map((example) => (
                <div
                  key={`${example.label}-${example.expression}`}
                  className="grid grid-cols-[74px_minmax(0,1fr)] items-center gap-2"
                >
                  <span className="font-semibold text-tx-2">{example.label}</span>
                  <code className="min-w-0 truncate rounded bg-bg-2 px-1.5 py-1 font-mono text-xs text-tx-0">
                    {example.expression}
                  </code>
                </div>
              ))}
            </div>
          </>
        ) : (
          <>
            <div className="border-b border-bd-0 bg-bg-1 px-4 py-3">
              <div className="font-sans text-sm font-bold text-tx-0">{resolvedTitle}</div>
              <div className="mt-1 font-sans text-xs text-tx-3">
                {resolvedDescription}
              </div>
            </div>
            <div className="space-y-2.5 px-4 py-3 font-sans text-xs">
              {examples.map((example) => (
                <div key={`${example.label}-${example.expression}`} className="grid grid-cols-[78px_minmax(0,1fr)] gap-3">
                  <span className="pt-0.5 font-semibold text-tx-2">{example.label}</span>
                  <div className="min-w-0">
                    <code className="inline-block max-w-full overflow-x-auto rounded border border-bd-0 bg-bg-2 px-1.5 py-0.5 font-mono text-xs text-tx-0">
                      {example.expression}
                    </code>
                    {example.description ? (
                      <div className="mt-1 text-xs text-tx-3">{example.description}</div>
                    ) : null}
                  </div>
                </div>
              ))}
            </div>
            {footer != null ? (
              <div className="border-t border-bd-0 bg-bg-1 px-4 py-2 font-sans text-xs text-tx-3">
                {footer}
              </div>
            ) : null}
          </>
        )}
      </PopoverContent>
    </Popover>
  );
}
