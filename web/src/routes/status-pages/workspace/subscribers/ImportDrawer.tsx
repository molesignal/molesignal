import { useMutation } from '@tanstack/react-query';
import { Upload } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as statusPagesApi from '@/api/statusPages';
import type {
  StatusPageSubscriberImportMode,
  StatusPageSubscriberImportReport,
} from '@/api/statusPages';
import { toApiError } from '@/lib/http';
import { ChromeButton } from '@/shell/chrome';
import { FilePicker } from '@/shell/FilePicker';
import { FormDrawer, FormField, FormRadio, FormSection, FormTextarea } from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';

import { ImportReport } from './ImportReport';

/**
 * Email subscribers brought over from another status page as a CSV file. The
 * file can be checked first; the import itself says what it did.
 */
export function ImportDrawer({
  open,
  pageId,
  onOpenChange,
  onImported,
}: {
  open: boolean;
  pageId: string;
  onOpenChange: (open: boolean) => void;
  onImported: () => Promise<void>;
}) {
  const { t } = useTranslation('status-pages');
  const [csv, setCsv] = React.useState('');
  const [fileName, setFileName] = React.useState<string | undefined>();
  const [mode, setMode] = React.useState<StatusPageSubscriberImportMode>('active');
  const [report, setReport] = React.useState<StatusPageSubscriberImportReport | null>(null);

  React.useEffect(() => {
    if (!open) return;
    setCsv('');
    setFileName(undefined);
    setMode('active');
    setReport(null);
  }, [open]);

  const tooLarge = new Blob([csv]).size > statusPagesApi.MAX_SUBSCRIBER_IMPORT_BYTES;
  const importMutation = useMutation({
    mutationFn: (dryRun: boolean) => statusPagesApi.importSubscribers(pageId, csv, mode, dryRun),
    onSuccess: async (result) => {
      setReport(result);
      if (result.dry_run) return;
      toast.success(t('toast.subscribers_imported', { count: result.imported }));
      await onImported();
    },
    onError: (error: unknown) => toast.error(toApiError(error).message),
  });
  const imported = report !== null && !report.dry_run;
  const invalid = !csv.trim() || tooLarge || importMutation.isPending;

  return (
    <FormDrawer
      open={open}
      onOpenChange={onOpenChange}
      title={t('subscribers.import.title')}
      subtitle={t('subscribers.import.subtitle')}
      footer={
        imported ? (
          <ChromeButton variant="primary" onClick={() => onOpenChange(false)}>
            {t('actions.done')}
          </ChromeButton>
        ) : (
          <>
            <ChromeButton disabled={invalid} onClick={() => importMutation.mutate(true)}>
              {t('actions.check_file')}
            </ChromeButton>
            <ChromeButton variant="primary" disabled={invalid} onClick={() => importMutation.mutate(false)}>
              <Upload className="h-3.5 w-3.5" />
              {t('actions.import_subscribers')}
            </ChromeButton>
          </>
        )
      }
    >
      <FormSection title={t('subscribers.import.file')} description={t('subscribers.import.format_hint')}>
        <FilePicker
          buttonLabel={t('subscribers.import.choose_file')}
          fileName={fileName}
          accept=".csv,text/csv,text/plain"
          disabled={imported}
          onFile={async (file) => {
            setFileName(file.name);
            setCsv(await file.text());
            setReport(null);
          }}
        />
        <FormField label={t('subscribers.import.content')}>
          <FormTextarea
            value={csv}
            rows={8}
            disabled={imported}
            placeholder={'email,components\nops@example.com,API; Dashboard\ndev@example.com,'}
            onChange={(change) => {
              setCsv(change.currentTarget.value);
              setReport(null);
            }}
          />
        </FormField>
        {tooLarge && <p role="alert" className="text-xs text-red-soft">{t('subscribers.import.too_large')}</p>}
      </FormSection>
      <FormSection title={t('subscribers.import.mode')}>
        <FormRadio<StatusPageSubscriberImportMode>
          value={mode}
          onChange={(next) => {
            setMode(next);
            setReport(null);
          }}
          options={(['active', 'confirm'] as const).map((option) => ({
            value: option,
            label: t(`subscribers.import.modes.${option}`),
            hint: t(`subscribers.import.modes.${option}_hint`),
          }))}
        />
      </FormSection>
      {report && (
        <FormSection title={t(report.dry_run ? 'subscribers.import.checked' : 'subscribers.import.done')}>
          <ImportReport report={report} />
        </FormSection>
      )}
    </FormDrawer>
  );
}
