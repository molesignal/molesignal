import { Braces, Download, FileJson } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { TraceResponse } from '@/api/web';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';

import { TraceHeadButton } from './HeadButton';
import { toOtlpTrace } from '../model/otlp';

/** The forms a trace is downloaded in: as the trace API answered, or as OTLP/JSON. */
export type TraceExportFormat = 'json' | 'otlp';

/** The name of a trace's file: its id, kept to what a file name holds everywhere. */
export function traceFileName(traceId: string, format: TraceExportFormat): string {
  const stem = traceId.replace(/[^A-Za-z0-9._-]+/g, '-').replace(/^-+|-+$/g, '') || 'trace';
  return format === 'otlp' ? `trace-${stem}.otlp.json` : `trace-${stem}.json`;
}

/** The text of a trace's file in a format. */
export function traceFileText(trace: TraceResponse, format: TraceExportFormat): string {
  return `${JSON.stringify(format === 'otlp' ? toOtlpTrace(trace) : trace, null, 2)}\n`;
}

function saveTraceFile(trace: TraceResponse, format: TraceExportFormat): void {
  const url = URL.createObjectURL(new Blob([traceFileText(trace, format)], { type: 'application/json' }));
  const link = document.createElement('a');
  link.href = url;
  link.download = traceFileName(trace.traceId, format);
  document.body.append(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
}

/**
 * The trace downloaded as a file: its spans as the trace API returns them,
 * or as OTLP/JSON, the form a collector takes in, to keep, share or send on.
 */
export function TraceExportMenu({ trace }: { trace: TraceResponse }) {
  const { t } = useTranslation('traces');

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <TraceHeadButton icon={Download} label={t('view.export.label')} data-testid="trace-export" />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-60">
        <DropdownMenuLabel>{t('view.export.label')}</DropdownMenuLabel>
        <DropdownMenuSeparator />
        <DropdownMenuItem onSelect={() => saveTraceFile(trace, 'json')} className="items-start py-1.5" data-testid="trace-export-json">
          <Braces aria-hidden="true" className="mt-[2px] h-3.5 w-3.5 shrink-0" />
          <span className="flex min-w-0 flex-col">
            <span>{t('view.export.json')}</span>
            <span className="type-micro text-tx-3">{t('view.export.json_hint')}</span>
          </span>
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={() => saveTraceFile(trace, 'otlp')} className="items-start py-1.5" data-testid="trace-export-otlp">
          <FileJson aria-hidden="true" className="mt-[2px] h-3.5 w-3.5 shrink-0" />
          <span className="flex min-w-0 flex-col">
            <span>{t('view.export.otlp')}</span>
            <span className="type-micro text-tx-3">{t('view.export.otlp_hint')}</span>
          </span>
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
