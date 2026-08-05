import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { PageHeader } from '@/admin';
import * as pipelinesApi from '@/api/pipelines';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { CodeEditor } from '@/shell/codeEditor';
import { FormField, FormSection } from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';

/**
 * Pipeline import. There is no dedicated `/scheduled_pipelines/import`
 * endpoint yet — until it lands, this page parses the supplied YAML/JSON
 * client-side and POSTs the equivalent payload to `/scheduled_pipelines`.
 */
export function PipelineImport() {
  const { t } = useTranslation('pipelines');
  const navigate = useNavigate();
  const createAccess = useActionAccess({ permission: 'pipelines.create' });
  const [text, setText] = React.useState('');
  const [busy, setBusy] = React.useState(false);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (!createAccess.allowed) return;
    setBusy(true);
    try {
      const trimmed = text.trim();
      if (!trimmed) {
        toast.error(t('flows.import.validation_empty'));
        return;
      }
      let payload: pipelinesApi.PipelineInput;
      if (trimmed.startsWith('{')) {
        payload = JSON.parse(trimmed) as pipelinesApi.PipelineInput;
      } else {
        // Minimal yaml: `key: value` line pairs. Reuse JSON for richer cases.
        const obj: Record<string, string> = {};
        trimmed.split('\n').forEach((line) => {
          const m = /^([\w-]+)\s*:\s*(.+)$/.exec(line);
          if (m) {
            const key = m[1];
            const value = m[2];
            if (key && value !== undefined) obj[key] = value;
          }
        });
        payload = {
          name: obj.name ?? 'imported',
          source_stream: obj.source_stream ?? '',
          target_stream: obj.target_stream ?? '',
          function_steps: [],
          cron: obj.cron ?? 'every:5m',
        };
      }
      const resp = await pipelinesApi.create(payload);
      toast.success(t('flows.import.toast_imported'));
      navigate(`/pipelines/${encodeURIComponent(resp.id)}/edit`);
    } catch (err) {
      toast.error(toApiError(err).message);
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <PageHeader
        title={t('flows.import.title')}
        actions={
          <>
            <ChromeButton onClick={() => navigate('/pipelines')}>{t('flows.import.cancel')}</ChromeButton>
            <ChromeButton
              type="submit"
              form="import-form"
              variant="primary"
              disabled={createAccess.disabled || busy}
              disabledReason={!busy ? createAccess.reason : undefined}
            >
              {busy ? t('flows.import.importing') : t('flows.import.import_label')}
            </ChromeButton>
          </>
        }
      />
      <div className="mx-auto max-w-3xl p-4">
        <form id="import-form" onSubmit={submit}>
          <FormSection
            title={t('flows.import.definition_title')}
            description={t('flows.import.definition_description')}
          >
            <FormField label={t('flows.import.definition_label')}>
              <CodeEditor
                value={text}
                onChange={setText}
                language={text.trim().startsWith('{') ? 'json' : 'yaml'}
                label={text.trim().startsWith('{') ? 'JSON' : 'YAML'}
                ariaLabel={t('flows.import.definition_aria')}
                placeholder={t('flows.import.definition_placeholder')}
                readOnly={createAccess.disabled}
                minHeight={420}
                maxHeight={640}
              />
            </FormField>
          </FormSection>
        </form>
      </div>
    </>
  );
}
