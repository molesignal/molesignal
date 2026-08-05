import { useMutation, useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import * as dashboardsApi from '@/api/dashboards';
import * as foldersApi from '@/api/folders';
import {
  dashboardDefinitionToModel,
  parseDashboardDefinitionJson,
} from '@/dashboard-engine/model';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton, uiLabelStrongClass } from '@/shell/chrome';
import { CodeEditor } from '@/shell/codeEditor';
import { FilePicker } from '@/shell/FilePicker';
import { PageBody, PageHeader } from '@/shell/PageHeader';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/shell/ui/select';
import { toast } from '@/shell/ui/sonner';

const DEFAULT_FOLDER_VALUE = '__default_folder__';

function selectedFolderId(value: string): string | undefined {
  return value === DEFAULT_FOLDER_VALUE ? undefined : value;
}

export function DashboardImport() {
  const { t } = useTranslation('dashboards');
  const { t: tc } = useTranslation('common');
  const nav = useNavigate();
  const createAccess = useActionAccess({ permission: 'dashboards.create' });
  const [text, setText] = React.useState('');
  const [parseError, setParseError] = React.useState<string | null>(null);
  const [fileName, setFileName] = React.useState('');
  const [folderSelection, setFolderSelection] = React.useState(DEFAULT_FOLDER_VALUE);

  const foldersQuery = useQuery({
    queryKey: ['folders', 'list'],
    queryFn: () => foldersApi.list(),
  });

  const validate = (raw: string): boolean => {
    if (!raw.trim()) {
      setParseError(t('import.validate_empty'));
      return false;
    }
    try {
      parseDashboardDefinitionJson(raw);
      setParseError(null);
      return true;
    } catch (e) {
      setParseError(t('import.validate_invalid', { message: (e as Error).message }));
      return false;
    }
  };

  const upload = useMutation({
    mutationFn: ({
      json,
      folderId,
    }: {
      json: string;
      folderId: string | undefined;
    }) => {
      const definition = parseDashboardDefinitionJson(json);
      return dashboardsApi.create(
        dashboardDefinitionToModel({
          ...definition,
          folderId,
        }),
        folderId,
      );
    },
    onSuccess: (d) => {
      toast.success(t('import.toast_imported'));
      nav(`/dashboards/${d.id}`);
    },
    onError: (err) => toast.error(toApiError(err).message),
  });

  const onSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!createAccess.allowed) return;
    if (!validate(text)) return;
    upload.mutate({ json: text, folderId: selectedFolderId(folderSelection) });
  };

  const onFile = async (file: File) => {
    setFileName(file.name);
    const raw = await file.text();
    setText(raw);
    validate(raw);
  };

  return (
    <>
      <PageHeader
        title={t('import.title')}
        subtitle={t('import.subtitle')}
      />
      <PageBody>
        <form onSubmit={onSubmit} className="flex flex-col gap-3">
          <FilePicker
            id="dashboard-import-file"
            label={t('import.upload_label')}
            buttonLabel={tc('actions.choose_file')}
            fileName={fileName}
            accept="application/json,.json"
            disabled={createAccess.disabled}
            disabledReason={createAccess.reason}
            onFile={onFile}
          />
          <label className="flex max-w-sm flex-col gap-1.5 font-sans text-xs text-tx-2">
            <span className={uiLabelStrongClass}>{t('editor.folder_label')}</span>
            <Select
              value={folderSelection}
              onValueChange={setFolderSelection}
              disabled={
                upload.isPending
                || foldersQuery.isLoading
                || createAccess.disabled
              }
            >
              <SelectTrigger
                aria-label={t('editor.folder_label')}
                className="h-8 rounded-md border-bd-1 bg-bg-2 px-2 font-sans text-sm font-strong text-tx-0"
              >
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={DEFAULT_FOLDER_VALUE} className="font-sans text-xs">
                  {t('list.default_folder')}
                </SelectItem>
                {(foldersQuery.data ?? []).map((folder) => (
                  <SelectItem key={folder.id} value={folder.id} className="font-sans text-xs">
                    {folder.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </label>
          <CodeEditor
            value={text}
            onChange={setText}
            language="json"
            label={t('import.editor_label')}
            ariaLabel={t('import.editor_aria')}
            placeholder={t('import.placeholder')}
            minHeight={420}
            maxHeight={640}
            readOnly={createAccess.disabled}
          />
          {parseError && <div className="font-sans text-xs text-red-soft">{parseError}</div>}
          <div className="flex items-center gap-2">
            <ChromeButton
              type="submit"
              variant="primary"
              disabled={upload.isPending || createAccess.disabled}
              disabledReason={!upload.isPending ? createAccess.reason : undefined}
            >
              {upload.isPending ? t('import.submitting') : t('import.submit')}
            </ChromeButton>
          </div>
        </form>
      </PageBody>
    </>
  );
}
