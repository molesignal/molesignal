import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { PageHeader } from '@/admin';
import * as licenseApi from '@/api/license';
import { toApiError } from '@/lib/http';
import { hasPermission, useProductAccess } from '@/product/access';
import { useActionAccess } from '@/product/actionAccess';
import { ProductState, productStateFor } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';
import { FilePicker } from '@/shell/FilePicker';
import { FormTextarea } from '@/shell/FormDrawer';
import { queryStateFor } from '@/shell/query/State';
import { toast } from '@/shell/ui/sonner';

import {
  KvRow,
  SectionBody,
  SettingsGroupStack,
  SettingsRow,
  SettingsSection,
} from './_atoms';
import { formatMicros } from '../rum/_helpers';

const BASE64_RE = /^[A-Za-z0-9+/=\s]+$/;

function isBase64(value: string): boolean {
  const trimmed = value.trim();
  return trimmed.length > 0 && BASE64_RE.test(trimmed);
}

export function License() {
  const { t } = useTranslation('settings-admin');
  const { t: tc } = useTranslation('common');
  const qc = useQueryClient();
  const access = useProductAccess();
  const canReadLicense = hasPermission('sys.licenses.read', access);
  const uploadAccess = useActionAccess({
    permission: 'sys.licenses.manage',
  });
  const canWriteLicense = uploadAccess.allowed;

  const q = useQuery({
    queryKey: ['license-snapshot'],
    queryFn: () => licenseApi.get(),
    enabled: canReadLicense,
    retry: false,
  });
  const data = q.data;
  const state = queryStateFor({ isLoading: q.isLoading, isError: q.isError, data });
  const pageState = productStateFor(state, {
    error: q.error,
    emptyTitle: t('license.title'),
    emptyDescription: t('license.empty_description'),
  });

  const [payload, setPayload] = React.useState('');
  const [signature, setSignature] = React.useState('');
  const [licenseFileName, setLicenseFileName] = React.useState('');
  const invalid = !isBase64(payload) || !isBase64(signature);

  const upload = useMutation({
    mutationFn: (input: licenseApi.LicenseUploadInput) => licenseApi.upload(input),
    onSuccess: (snapshot) => {
      if (snapshot.verified) {
        toast.success(t('license.upload.toast_uploaded'));
      } else {
        toast.error(t('license.upload.toast_uploaded_unverified'));
      }
      qc.setQueryData(['license-snapshot'], snapshot);
      setPayload('');
      setSignature('');
      setLicenseFileName('');
    },
    onError: (err) => toast.error(toApiError(err).message),
  });

  const onSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!canWriteLicense || upload.isPending) return;
    const trimmedPayload = payload.trim();
    const trimmedSig = signature.trim();
    if (!trimmedPayload || !trimmedSig) {
      toast.error(t('license.upload.validation_required'));
      return;
    }
    if (!isBase64(trimmedPayload) || !isBase64(trimmedSig)) {
      toast.error(t('license.upload.validation_base64'));
      return;
    }
    upload.mutate({
      payload_b64: trimmedPayload.replace(/\s+/g, ''),
      signature_b64: trimmedSig.replace(/\s+/g, ''),
    });
  };

  // Browser-side file loader. The `.license` format we support is a
  // newline-separated `payload_b64\nsignature_b64` blob — the simplest
  // shape a sysadmin can email around. Parsing here keeps the textarea
  // editable as a fallback for paste-based workflows.
  const onFile = async (file: File) => {
    setLicenseFileName(file.name);
    const raw = await file.text();
    const lines = raw
      .split(/\r?\n/)
      .map((line) => line.trim())
      .filter(Boolean);
    if (lines.length >= 2) {
      setPayload(lines[0]!);
      setSignature(lines[1]!);
    } else if (lines.length === 1) {
      setPayload(lines[0]!);
    }
  };

  return (
    <>
      <PageHeader title={t('license.title')} subtitle={t('license.subtitle') as string} />
      <SectionBody className="pb-10">
        <SettingsGroupStack>
          <SettingsSection
            title={t('license.current_title')}
            description={t('license.current_description')}
          >
            {pageState ? (
              <div className="py-6">
                <ProductState {...pageState} compact />
              </div>
            ) : data ? (
              <div className="py-4">
                <KvRow label={t('license.labels.edition')}>{data.edition}</KvRow>
                <KvRow label={t('license.labels.verified')}>
                  {data.verified ? tc('status.yes') : tc('status.no')}
                </KvRow>
                <KvRow label={t('license.labels.expired')}>
                  {data.expired ? tc('status.yes') : tc('status.no')}
                </KvRow>
                <KvRow label={t('license.labels.issued_to')}>{data.issued_to}</KvRow>
                <KvRow label={t('license.labels.features')}>
                  {data.features.length === 0 ? '—' : data.features.join(', ')}
                </KvRow>
                <KvRow label={t('license.labels.max_ingest_bytes_per_day')}>
                  {data.max_ingest_bytes_per_day ?? '—'}
                </KvRow>
                <KvRow label={t('license.labels.expires_at')}>
                  {data.expires_at_micros ? formatMicros(data.expires_at_micros) : '—'}
                </KvRow>
              </div>
            ) : null}
          </SettingsSection>

          <SettingsSection
            title={t('license.upload.title')}
            description={t('license.upload.subtitle')}
          >
            <form onSubmit={onSubmit}>
              <SettingsRow
                label={t('license.upload.payload_label')}
                controlClassName="w-full"
              >
                <FormTextarea
                  value={payload}
                  onChange={(e) => setPayload(e.target.value)}
                  placeholder={t('license.upload.payload_placeholder')}
                  rows={4}
                  className="font-sans text-xs"
                  disabled={!canWriteLicense || upload.isPending}
                  disabledReason={
                    !canWriteLicense
                      ? uploadAccess.reason
                      : upload.isPending
                        ? tc('access.operation_pending')
                        : undefined
                  }
                />
              </SettingsRow>
              <SettingsRow
                label={t('license.upload.signature_label')}
                controlClassName="w-full"
              >
                <FormTextarea
                  value={signature}
                  onChange={(e) => setSignature(e.target.value)}
                  placeholder={t('license.upload.signature_placeholder')}
                  rows={3}
                  className="font-sans text-xs"
                  disabled={!canWriteLicense || upload.isPending}
                  disabledReason={
                    !canWriteLicense
                      ? uploadAccess.reason
                      : upload.isPending
                        ? tc('access.operation_pending')
                        : undefined
                  }
                />
              </SettingsRow>
              <SettingsRow
                label={t('license.upload.from_file')}
                controlClassName="w-full"
              >
                <FilePicker
                  buttonLabel={tc('actions.choose_file')}
                  fileName={licenseFileName}
                  accept=".license,application/octet-stream,text/plain"
                  disabled={!canWriteLicense || upload.isPending}
                  disabledReason={
                    !canWriteLicense
                      ? uploadAccess.reason
                      : upload.isPending
                        ? tc('access.operation_pending')
                        : undefined
                  }
                  onFile={onFile}
                />
              </SettingsRow>
              <div className="flex justify-end py-4">
                <ChromeButton
                  type="submit"
                  variant="primary"
                  disabled={!canWriteLicense || invalid || upload.isPending}
                  disabledReason={
                    !canWriteLicense
                      ? uploadAccess.reason
                      : invalid
                        ? tc('access.form_invalid')
                        : upload.isPending
                          ? tc('access.operation_pending')
                          : undefined
                  }
                >
                  {upload.isPending ? t('license.upload.submitting') : t('license.upload.submit')}
                </ChromeButton>
              </div>
            </form>
          </SettingsSection>
        </SettingsGroupStack>
      </SectionBody>
    </>
  );
}
