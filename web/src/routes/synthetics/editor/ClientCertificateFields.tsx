import { useTranslation } from 'react-i18next';

import { FilePicker } from '@/shell/FilePicker';
import { FormField, FormRow, FormTextarea } from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';

import type { CheckDraft } from './model';
import { MAX_PEM_BYTES, PEM_FILES, pemParts } from './pem';

type DraftPatch = <K extends keyof CheckDraft>(key: K, value: CheckDraft[K]) => void;

/**
 * The certificate an HTTP check shows a server that asks for one, and its private key, uploaded
 * or pasted as PEM. A saved key never comes back from the server: its field says it is there.
 */
export function ClientCertificateFields({ draft, patch }: { draft: CheckDraft; patch: DraftPatch }) {
  const { t } = useTranslation('synthetics');

  // A file can hold the certificate and the key together: each goes to its own field. The other
  // field takes its part only while it is empty.
  const upload = async (file: File, field: 'certificate' | 'key') => {
    if (file.size > MAX_PEM_BYTES) {
      toast.error(t('editor.pem_file_too_large'));
      return;
    }

    const { certificates, key } = pemParts(await file.text());

    if (!(field === 'certificate' ? certificates : key)) {
      toast.error(t(field === 'certificate' ? 'editor.no_certificate_in_file' : 'editor.no_key_in_file'));
    }

    if (certificates && (field === 'certificate' || !draft.clientCertificate.trim())) {
      patch('clientCertificate', certificates);
    }

    if (key && (field === 'key' || !draft.clientKey.trim())) {
      patch('clientKey', key);
    }
  };

  return (
    <div className="flex flex-col gap-1.5">
      <FormRow className="grid-cols-1 sm:grid-cols-2">
        <div className="flex min-w-0 flex-col gap-1.5">
          <FormField label={t('editor.client_certificate')}>
            <FormTextarea
              className="min-h-20 font-code"
              spellCheck={false}
              value={draft.clientCertificate}
              onChange={(event) => patch('clientCertificate', event.target.value)}
              placeholder="-----BEGIN CERTIFICATE-----"
            />
          </FormField>
          <FilePicker
            buttonLabel={t('actions.upload_certificate')}
            accept={PEM_FILES}
            onFile={(file) => upload(file, 'certificate')}
          />
        </div>
        <div className="flex min-w-0 flex-col gap-1.5">
          <FormField label={t('editor.client_key')}>
            <FormTextarea
              className="min-h-20 font-code"
              spellCheck={false}
              autoComplete="off"
              value={draft.clientKey}
              onChange={(event) => patch('clientKey', event.target.value)}
              placeholder={draft.clientKeyRevision ? t('editor.client_key_saved') : '-----BEGIN PRIVATE KEY-----'}
            />
          </FormField>
          <FilePicker
            buttonLabel={t('actions.upload_key')}
            accept={PEM_FILES}
            onFile={(file) => upload(file, 'key')}
          />
        </div>
      </FormRow>
      <p className="text-xs leading-relaxed text-tx-3">{t('editor.client_certificate_hint')}</p>
    </div>
  );
}
