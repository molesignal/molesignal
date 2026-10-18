import { useMutation } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as skillsApi from '@/api/agent/skills';
import { toApiError } from '@/lib/http';
import { FormDrawer, FormField, FormSection, FormSelect } from '@/shell/FormDrawer';
import { Button } from '@/shell/ui/button';
import { toast } from '@/shell/ui/sonner';

import { scopeTarget, useScopes } from '../scope/useScopes';

/**
 * Uploads a skill package: a `.zip` or `.tar.gz` with a folder holding
 * `SKILL.md`. A skill of the same name in the scope is replaced only after
 * saying so.
 */
export function SkillUploadDrawer({
  open,
  onClose,
  onUploaded,
}: {
  open: boolean;
  onClose: () => void;
  onUploaded: () => Promise<void>;
}) {
  const { t } = useTranslation('agent');
  const scopes = useScopes();
  const [scope, setScope] = React.useState('personal');
  const [file, setFile] = React.useState<File | null>(null);
  const [exists, setExists] = React.useState<string | null>(null);

  React.useEffect(() => {
    if (open) {
      setFile(null);
      setExists(null);
    }
  }, [open]);

  const upload = useMutation({
    mutationFn: (replace: boolean) =>
      skillsApi.uploadSkill(file as File, scopeTarget(scope), replace),
    onSuccess: async () => {
      toast.success(t('settings.skills.upload.done'));
      await onUploaded();
      onClose();
    },
    onError: (error) => {
      const apiError = toApiError(error);
      if (apiError.code === 'skill_exists') {
        const data = (error as { response?: { data?: { name?: string } } }).response?.data;
        setExists(data?.name ?? '');
        return;
      }
      toast.error(apiError.message);
    },
  });

  return (
    <FormDrawer
      open={open}
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
      title={t('settings.skills.upload.title')}
      subtitle={t('settings.skills.upload.description')}
      width={560}
      footer={
        <>
          <Button variant="outline" onClick={onClose}>
            {t('common.cancel')}
          </Button>
          {exists !== null ? (
            <Button
              variant="destructive"
              disabled={upload.isPending}
              onClick={() => upload.mutate(true)}
            >
              {t('settings.skills.upload.replace')}
            </Button>
          ) : (
            <Button disabled={!file || upload.isPending} onClick={() => upload.mutate(false)}>
              {t('settings.skills.upload.submit')}
            </Button>
          )}
        </>
      }
    >
      <FormSection>
        <FormField label={t('scope.field')}>
          <FormSelect
            value={scope}
            onChange={(value) => {
              setScope(value);
              setExists(null);
            }}
            options={scopes.options}
            ariaLabel={t('scope.field')}
            className="w-full"
          />
        </FormField>
        <FormField
          label={t('settings.skills.upload.file')}
          hint={t('settings.skills.upload.file_hint')}
        >
          <input
            type="file"
            accept=".zip,.skill,.tar.gz,.tgz,application/zip,application/gzip"
            aria-label={t('settings.skills.upload.file')}
            onChange={(event) => {
              setFile(event.target.files?.[0] ?? null);
              setExists(null);
            }}
            className="block w-full rounded-md bg-[var(--control-surface)] px-3 py-2 text-xs text-tx-1 file:mr-3 file:rounded file:border-0 file:bg-bg-3 file:px-2 file:py-1 file:text-xs file:text-tx-1"
          />
        </FormField>
        {exists !== null && (
          <p
            role="alert"
            className="rounded-md bg-[var(--yellow-tint)] p-3 text-xs leading-5 text-[var(--yellow-ink)]"
          >
            {t('settings.skills.upload.exists', { name: exists })}
          </p>
        )}
      </FormSection>
    </FormDrawer>
  );
}
