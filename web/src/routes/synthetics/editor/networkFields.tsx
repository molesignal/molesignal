import { useTranslation } from 'react-i18next';

import {
  FormField,
  FormInput,
  FormRow,
  FormSelect,
  FormTextarea,
} from '@/shell/FormDrawer';

import type { CheckDraft } from './model';

type DraftPatch = <K extends keyof CheckDraft>(key: K, value: CheckDraft[K]) => void;

export function SshFields({ draft, patch }: { draft: CheckDraft; patch: DraftPatch }) {
  const { t } = useTranslation('synthetics');
  return (
    <>
      <HostPortFields draft={draft} patch={patch} />
      <FormField
        label={t('editor.ssh_identification_regex')}
        hint={t('editor.ssh_identification_regex_hint')}
      >
        <FormInput
          className="font-code"
          value={draft.sshIdentificationRegex}
          onChange={(event) => patch('sshIdentificationRegex', event.target.value)}
          placeholder="^SSH-2\\.0-OpenSSH_"
        />
      </FormField>
      <FormField label={t('editor.ssh_authentication')}>
        <FormSelect
          value={draft.sshAuthMode}
          onChange={(value) => patch('sshAuthMode', value as CheckDraft['sshAuthMode'])}
          options={[
            { value: 'none', label: t('editor.ssh_auth_none') },
            { value: 'password', label: t('editor.ssh_auth_password') },
            { value: 'public_key', label: t('editor.ssh_auth_public_key') },
          ]}
        />
      </FormField>
      {draft.sshAuthMode !== 'none' && (
        <>
          <FormField label={t('editor.ssh_username')} required>
            <FormInput
              value={draft.sshUsername}
              onChange={(event) => patch('sshUsername', event.target.value)}
              placeholder="deploy or {{SSH_USERNAME}}"
            />
          </FormField>
          {draft.sshAuthMode === 'password' ? (
            <FormField label={t('editor.ssh_password')} required>
              <FormInput
                type="password"
                autoComplete="new-password"
                value={draft.sshPassword}
                onChange={(event) => patch('sshPassword', event.target.value)}
                placeholder="{{SSH_PASSWORD}}"
              />
            </FormField>
          ) : (
            <>
              <FormField label={t('editor.ssh_private_key')} required>
                <FormTextarea
                  className="min-h-24 font-code"
                  value={draft.sshPrivateKey}
                  onChange={(event) => patch('sshPrivateKey', event.target.value)}
                  placeholder="{{SSH_PRIVATE_KEY}}"
                />
              </FormField>
              <FormField label={t('editor.ssh_passphrase')}>
                <FormInput
                  type="password"
                  autoComplete="new-password"
                  value={draft.sshPassphrase}
                  onChange={(event) => patch('sshPassphrase', event.target.value)}
                  placeholder="{{SSH_KEY_PASSPHRASE}}"
                />
              </FormField>
            </>
          )}
          <FormField
            label={t('editor.ssh_host_key_sha256')}
            hint={t('editor.ssh_host_key_sha256_hint')}
            required
          >
            <FormInput
              className="font-code"
              value={draft.sshHostKeySha256}
              onChange={(event) => patch('sshHostKeySha256', event.target.value)}
              placeholder="SHA256:…"
            />
          </FormField>
          <FormField label={t('editor.ssh_command')}>
            <FormInput
              className="font-code"
              value={draft.sshCommand}
              onChange={(event) => patch('sshCommand', event.target.value)}
              placeholder="systemctl is-active my-service"
            />
          </FormField>
          {draft.sshCommand.trim() && (
            <FormRow className="grid-cols-1 sm:grid-cols-[minmax(0,1fr)_160px]">
              <FormField label={t('editor.ssh_output_regex')}>
                <FormInput
                  className="font-code"
                  value={draft.sshOutputRegex}
                  onChange={(event) => patch('sshOutputRegex', event.target.value)}
                  placeholder="^active$"
                />
              </FormField>
              <FormField label={t('editor.ssh_exit_status')}>
                <FormInput
                  type="number"
                  value={draft.sshExitStatus}
                  onChange={(event) => patch('sshExitStatus', event.target.value)}
                />
              </FormField>
            </FormRow>
          )}
        </>
      )}
    </>
  );
}

export function HostPortFields({ draft, patch }: { draft: CheckDraft; patch: DraftPatch }) {
  const { t } = useTranslation('synthetics');
  return (
    <FormRow className="grid-cols-1 sm:grid-cols-[minmax(0,1fr)_120px]">
      <FormField label={t('editor.host')} required>
        <FormInput
          value={draft.host}
          onChange={(event) => patch('host', event.target.value)}
        />
      </FormField>
      <FormField label={t('editor.port')}>
        <FormInput
          type="number"
          value={draft.port}
          onChange={(event) => patch('port', event.target.value)}
        />
      </FormField>
    </FormRow>
  );
}
