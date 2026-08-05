import { useMutation } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import * as debugArtifactsApi from '@/api/debugArtifacts';
import { toApiError } from '@/lib/http';
import { DetailPage } from '@/product/templates';
import { ChromeButton } from '@/shell/chrome';
import { FilePicker } from '@/shell/FilePicker';
import { FormField, FormInput, FormSection, FormSelect } from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';

import { useRumBasePath } from '../RumLayout';

const DEFAULT_PLATFORM: Record<debugArtifactsApi.DebugArtifactKind, string> = {
  javascript_sourcemap: 'web',
  flutter_symbols: 'android',
  android_mapping: 'android',
  android_native_symbols: 'android',
  apple_dsym: 'ios',
};

function platformsFor(kind: debugArtifactsApi.DebugArtifactKind): string[] {
  if (kind === 'javascript_sourcemap') return ['web', 'flutter'];
  if (kind === 'flutter_symbols') return ['android', 'ios'];
  if (kind === 'apple_dsym') return ['ios'];
  return ['android'];
}

export function UploadSourceMaps() {
  const { t } = useTranslation('rum');
  const { t: tc } = useTranslation('common');
  const navigate = useNavigate();
  const basePath = useRumBasePath();
  const [applicationId, setApplicationId] = React.useState('');
  const [service, setService] = React.useState('');
  const [release, setRelease] = React.useState('');
  const [kind, setKind] = React.useState<debugArtifactsApi.DebugArtifactKind>(
    'javascript_sourcemap',
  );
  const [platform, setPlatform] = React.useState('web');
  const [architecture, setArchitecture] = React.useState('');
  const [debugId, setDebugId] = React.useState('');
  const [file, setFile] = React.useState<File | null>(null);

  const upload = useMutation({
    mutationFn: () => {
      if (!file) throw new Error(t('upload_source_maps.file_required'));
      return debugArtifactsApi.upload({
        application_id: applicationId.trim(),
        service: service.trim(),
        release: release.trim(),
        kind,
        platform,
        ...(architecture.trim() ? { architecture: architecture.trim() } : {}),
        ...(debugId.trim() ? { debug_id: debugId.trim() } : {}),
        file,
      });
    },
    onSuccess: () => {
      toast.success(t('upload_source_maps.success'));
      navigate(`${basePath}/settings/source-maps`);
    },
    onError: (error) => {
      toast.error(toApiError(error).message);
    },
  });

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    upload.mutate();
  };

  const selectKind = (value: string) => {
    const next = value as debugArtifactsApi.DebugArtifactKind;
    setKind(next);
    setPlatform(DEFAULT_PLATFORM[next]);
  };

  return (
    <DetailPage
      title={t('upload_source_maps.title')}
      toolbar={
        <ChromeButton onClick={() => navigate(`${basePath}/settings/source-maps`)}>
          ← {t('upload_source_maps.back')}
        </ChromeButton>
      }
      metadata={[
        {
          label: t('source_maps.title'),
          value: (
            <button
              type="button"
              onClick={() => navigate(`${basePath}/settings/source-maps`)}
              className="rounded text-blue-soft outline-none hover:bg-bg-2 focus-visible:bg-bg-2"
            >
              {t('source_maps.title')}
            </button>
          ),
        },
      ]}
    >
      <div className="mx-auto max-w-2xl">
        <form onSubmit={submit} className="space-y-4">
          <FormSection>
            <div className="grid gap-4 md:grid-cols-2">
              <FormField label={t('upload_source_maps.application_label')} required>
                <FormInput
                  value={applicationId}
                  onChange={(event) => setApplicationId(event.target.value)}
                  placeholder="checkout-mobile"
                  pattern="[A-Za-z0-9._:-]{1,128}"
                  maxLength={128}
                  required
                />
              </FormField>
              <FormField label={t('upload_source_maps.service_label')} required>
                <FormInput
                  value={service}
                  onChange={(event) => setService(event.target.value)}
                  placeholder="mobile-app"
                  maxLength={255}
                  required
                />
              </FormField>
              <FormField label={t('upload_source_maps.release_label')} required>
                <FormInput
                  value={release}
                  onChange={(event) => setRelease(event.target.value)}
                  placeholder="1.4.0+104"
                  maxLength={64}
                  required
                />
              </FormField>
              <FormField label={t('upload_source_maps.kind_label')} required>
                <FormSelect
                  value={kind}
                  onChange={selectKind}
                  options={(
                    [
                      'javascript_sourcemap',
                      'flutter_symbols',
                      'android_mapping',
                      'android_native_symbols',
                      'apple_dsym',
                    ] as debugArtifactsApi.DebugArtifactKind[]
                  ).map((value) => ({
                    value,
                    label: t(`source_maps.kinds.${value}`),
                  }))}
                />
              </FormField>
              <FormField label={t('upload_source_maps.platform_label')} required>
                <FormSelect
                  value={platform}
                  onChange={setPlatform}
                  options={platformsFor(kind).map((value) => ({
                    value,
                    label: value.toUpperCase(),
                  }))}
                />
              </FormField>
              <FormField
                label={t('upload_source_maps.architecture_label')}
                hint={t('upload_source_maps.architecture_hint')}
              >
                <FormInput
                  value={architecture}
                  onChange={(event) => setArchitecture(event.target.value)}
                  placeholder="arm64"
                  maxLength={32}
                />
              </FormField>
              <FormField
                label={t('upload_source_maps.debug_id_label')}
                hint={t('upload_source_maps.debug_id_hint')}
                className="md:col-span-2"
              >
                <FormInput
                  value={debugId}
                  onChange={(event) => setDebugId(event.target.value)}
                  placeholder={t('upload_source_maps.debug_id_placeholder')}
                  maxLength={128}
                />
              </FormField>
            </div>
            <FormField
              label={t('upload_source_maps.file_label')}
              hint={t('upload_source_maps.file_hint')}
              required
            >
              <FilePicker
                buttonLabel={tc('actions.choose_file')}
                fileName={file?.name}
                onFile={setFile}
              />
            </FormField>
          </FormSection>
          <div className="flex justify-end">
            <ChromeButton
              type="submit"
              variant="primary"
              disabled={upload.isPending || !file}
            >
              {upload.isPending
                ? t('upload_source_maps.uploading')
                : t('upload_source_maps.submit')}
            </ChromeButton>
          </div>
        </form>
      </div>
    </DetailPage>
  );
}
