import { type QueryKey, useMutation, useQueryClient } from '@tanstack/react-query';
import { ChevronDown, Download, Upload } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { ImportAction, ImportFinding, ImportReport } from '@/api/alerts';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { FormDrawer, FormField, FormTextarea } from '@/shell/FormDrawer';
import { Checkbox } from '@/shell/ui/checkbox';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';
import { toast } from '@/shell/ui/sonner';

import { MigrationHelp } from './MigrationHelp';
import type { MigrationSource } from './scheduleSources';

/** The colour of an import's outcome word. */
const ACTION_INK: Record<ImportAction, string> = {
  create: 'text-[var(--green-ink)]',
  update: 'text-[var(--blue-ink)]',
  unchanged: 'text-tx-2',
  skip: 'text-tx-2',
  error: 'text-[var(--red-ink)]',
};

/** A file of `text` saved under `name`. */
function save(name: string, text: string) {
  const url = URL.createObjectURL(new Blob([text], { type: 'application/json;charset=utf-8' }));
  const link = document.createElement('a');
  link.href = url;
  link.download = name;
  document.body.append(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
}

/** What one kind of thing needs to go out as a file and come back from one. */
export interface TransferKind {
  /** Prefix of the test ids: `<prefix>-transfer`, `<prefix>-export`, `<prefix>-import`, `<prefix>-import-try`, `<prefix>-import-apply`. */
  testPrefix: string;
  /** Prefix of the kind's own texts in the `alerts` namespace: menu, export_all, export, import, import_title, import_subtitle, exported, imported, content_hint. */
  texts: string;
  /** The file name an export is saved under, without the date. */
  fileName: string;
  readPermission: `${string}.${string}`;
  managePermission: `${string}.${string}`;
  /** Exports the things with these ids, or all; says how many went out. */
  exportBundle: (ids: string[]) => Promise<{ bundle: unknown; count: number }>;
  importBundle: (input: { content: string; dryRun: boolean; createOnly: boolean }) => Promise<ImportReport>;
  /** What to read again after an import wrote something. */
  queryKey: QueryKey;
  placeholder: string;
  /** Other tools the import reads, with how to fetch from them. */
  migrationSources?: readonly MigrationSource[];
}

/**
 * Things as a file: exported as a bundle that names what they refer to, and imported from such a
 * bundle, tried first and then written.
 */
export function TransferMenu({ kind, ids = [] }: { kind: TransferKind; ids?: string[] }) {
  const { t } = useTranslation('alerts');
  const readAccess = useActionAccess({ permission: kind.readPermission });
  const manageAccess = useActionAccess({ permission: kind.managePermission });
  const [importing, setImporting] = React.useState(false);

  const exported = useMutation({
    mutationFn: () => kind.exportBundle(ids),
    onSuccess: ({ bundle, count }) => {
      save(`${kind.fileName}-${new Date().toISOString().slice(0, 10)}.json`, `${JSON.stringify(bundle, null, 2)}\n`);
      toast.success(t(`${kind.texts}.exported`, { count }));
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <ChromeButton data-testid={`${kind.testPrefix}-transfer`}>
            {t(`${kind.texts}.menu`)}
            <ChevronDown className="h-3.5 w-3.5" />
          </ChromeButton>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="w-64">
          <DropdownMenuItem
            disabled={readAccess.disabled || exported.isPending}
            onSelect={() => exported.mutate()}
            data-testid={`${kind.testPrefix}-export`}
          >
            <Download className="h-3.5 w-3.5" />
            {ids.length === 0 ? t(`${kind.texts}.export_all`) : t(`${kind.texts}.export`, { count: ids.length })}
          </DropdownMenuItem>
          <DropdownMenuItem
            disabled={manageAccess.disabled}
            disabledReason={manageAccess.reason}
            onSelect={() => setImporting(true)}
            data-testid={`${kind.testPrefix}-import`}
          >
            <Upload className="h-3.5 w-3.5" />
            {t(`${kind.texts}.import`)}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
      <ImportDrawer kind={kind} open={importing} onClose={() => setImporting(false)} />
    </>
  );
}

function ImportDrawer({ kind, open, onClose }: { kind: TransferKind; open: boolean; onClose: () => void }) {
  const { t } = useTranslation('alerts');
  const queryClient = useQueryClient();
  const access = useActionAccess({ permission: kind.managePermission });
  const [content, setContent] = React.useState('');
  const [createOnly, setCreateOnly] = React.useState(false);
  const [report, setReport] = React.useState<ImportReport | null>(null);

  React.useEffect(() => {
    if (!open) return;
    setContent('');
    setCreateOnly(false);
    setReport(null);
  }, [open]);

  // What was tried no longer stands once the file or the choice changes.
  React.useEffect(() => setReport(null), [content, createOnly]);

  const tried = useMutation({
    mutationFn: () => kind.importBundle({ content, dryRun: true, createOnly }),
    onSuccess: setReport,
    onError: (error) => toast.error(toApiError(error).message),
  });

  const written = useMutation({
    mutationFn: () => kind.importBundle({ content, dryRun: false, createOnly }),
    onSuccess: async (result) => {
      if (!result.applied) {
        setReport(result);
        return;
      }

      const writes = result.items.filter((item) => item.action === 'create' || item.action === 'update').length;
      toast.success(t(`${kind.texts}.imported`, { count: writes }));
      await queryClient.invalidateQueries({ queryKey: kind.queryKey });
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const failed = report?.items.some((item) => item.action === 'error') ?? false;
  const writes = report?.items.filter((item) => item.action === 'create' || item.action === 'update').length ?? 0;

  return (
    <FormDrawer
      open={open}
      onOpenChange={(next) => !next && onClose()}
      title={t(`${kind.texts}.import_title`)}
      subtitle={t(`${kind.texts}.import_subtitle`)}
      footer={
        <>
          <ChromeButton onClick={onClose}>{t('rule_transfer.cancel')}</ChromeButton>
          <ChromeButton
            disabled={!content.trim() || tried.isPending || access.disabled}
            onClick={() => tried.mutate()}
            data-testid={`${kind.testPrefix}-import-try`}
          >
            {t('rule_transfer.try')}
          </ChromeButton>
          <ChromeButton
            variant="primary"
            disabled={!report || failed || writes === 0 || written.isPending || access.disabled}
            disabledReason={access.reason}
            onClick={() => written.mutate()}
            data-testid={`${kind.testPrefix}-import-apply`}
          >
            {t('rule_transfer.apply', { count: writes })}
          </ChromeButton>
        </>
      }
    >
      <div className="flex flex-col gap-4" data-rule-import>
        <FormField label={t('rule_transfer.content')} hint={t(`${kind.texts}.content_hint`)}>
          <FormTextarea
            value={content}
            rows={12}
            spellCheck={false}
            className="font-mono text-xs"
            placeholder={kind.placeholder}
            onChange={(event) => setContent(event.target.value)}
          />
        </FormField>
        <div className="flex flex-wrap items-center gap-3">
          <label className="inline-flex cursor-pointer items-center gap-2 font-sans text-sm text-tx-1">
            <input
              type="file"
              accept=".json,.yaml,.yml,application/json,text/yaml"
              className="sr-only"
              data-rule-import-file
              onChange={(event) => {
                const file = event.target.files?.[0];
                if (file) void file.text().then(setContent);
                event.target.value = '';
              }}
            />
            <span className="rounded-md bg-[var(--control-surface)] px-2.5 py-1.5 hover:bg-bg-3">
              {t('rule_transfer.choose_file')}
            </span>
          </label>
          <label className="flex items-center gap-2 font-sans text-sm text-tx-0">
            <Checkbox checked={createOnly} onCheckedChange={(next) => setCreateOnly(next === true)} />
            <span>{t('rule_transfer.create_only')}</span>
          </label>
        </div>
        {kind.migrationSources && <MigrationHelp sources={kind.migrationSources} texts={kind.texts} />}
        {report && (
          <section aria-label={t('rule_transfer.result')} data-rule-import-report={report.format}>
            <p className="mb-2 font-sans text-xs text-tx-2">
              {t(`rule_transfer.formats.${report.format}`, { defaultValue: '' })}
              {failed ? ` · ${t(`${kind.texts}.has_errors`, { defaultValue: t('rule_transfer.has_errors') })}` : ''}
            </p>
            <ul className="divide-y divide-bd-0 rounded-md bg-bg-2">
              {report.items.map((item, index) => (
                <li key={`${item.name}-${index}`} className="flex min-w-0 items-start gap-3 px-3 py-2 font-sans text-sm" data-import-item={item.action}>
                  <span className={`w-16 shrink-0 font-strong ${ACTION_INK[item.action]}`}>
                    {t(`rule_transfer.actions.${item.action}`)}
                  </span>
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-tx-0">{item.name}</span>
                    {item.error && !item.blockers?.length && !item.unknownMembers?.length && (
                      <span className="block text-xs text-[var(--red-ink)]">{item.error}</span>
                    )}
                    <FindingLines findings={item.blockers} className="text-[var(--red-ink)]" />
                    {item.unknownMembers && item.unknownMembers.length > 0 && (
                      <span className="block text-xs text-[var(--red-ink)]" data-import-unknown-members>
                        {t('transfer.unknown_members')}
                        {item.unknownMembers.map((email) => (
                          <span key={email} className="block font-mono">
                            {email}
                          </span>
                        ))}
                      </span>
                    )}
                    {item.changes && item.changes.length > 0 && (
                      <span className="block text-xs text-tx-2">
                        {t('rule_transfer.changes', { fields: item.changes.join(', ') })}
                      </span>
                    )}
                    <FindingLines findings={item.findings} className="text-tx-2" />
                  </span>
                </li>
              ))}
            </ul>
            {report.notes && report.notes.length > 0 && (
              <ul className="mt-2 list-disc pl-5 font-sans text-xs text-tx-2">
                {report.notes.map((note) => (
                  <li key={note}>{note}</li>
                ))}
              </ul>
            )}
            {report.findings && report.findings.length > 0 && (
              <div className="mt-2 font-sans" data-import-findings>
                <FindingLines findings={report.findings} className="text-tx-2" />
              </div>
            )}
          </section>
        )}
      </div>
    </FormDrawer>
  );
}

/** What an import has to say about an item or the file, a line each, in the reader's language. */
function FindingLines({ findings, className }: { findings: ImportFinding[] | undefined; className: string }) {
  const { t } = useTranslation('alerts');

  return (
    <>
      {findings?.map((finding, index) => (
        <span key={`${finding.code}-${index}`} className={`block text-xs ${className}`} data-import-finding={finding.code}>
          {t(`transfer.findings.${finding.code}`, {
            ...finding.params,
            ...(finding.params?.count ? { count: Number(finding.params.count) } : {}),
          })}
        </span>
      ))}
    </>
  );
}
