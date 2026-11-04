import { useTranslation } from 'react-i18next';

import type { RumApplicationSettings, RumPrivacyLevel, RumSdkSettings } from '@/api/rum';
import { FormField, FormInput, FormSection, FormSelect } from '@/shell/FormDrawer';

const CONSOLE_LEVELS = ['debug', 'log', 'info', 'warn', 'error'] as const;
const NOT_SAID = '';

/** A setting in or out of what an SDK is told: what is left out, the SDK takes from its page. */
function withSetting<K extends keyof RumSdkSettings>(sdk: RumSdkSettings, name: K, value: RumSdkSettings[K] | undefined): RumSdkSettings {
  const next = { ...sdk };

  if (value === undefined) delete next[name];
  else next[name] = value;

  return next;
}

/**
 * What an SDK that asks at start (`remoteConfiguration: true`) goes by, so that a share or a privacy
 * level can change without a new release of the page. What is left empty, the SDK takes from its page.
 */
export function SdkSection({
  settings,
  onChange,
  disabled,
}: {
  settings: RumApplicationSettings;
  onChange: (next: RumApplicationSettings) => void;
  disabled: boolean;
}) {
  const { t } = useTranslation('rum');
  const sdk = settings.sdk ?? {};
  const set = (next: RumSdkSettings) => onChange({ ...settings, sdk: next });

  const share = (name: 'sessionSampleRate' | 'sessionReplaySampleRate', label: string, hint: string) => (
    <FormField label={label} hint={hint}>
      <FormInput
        type="number"
        min={0}
        max={100}
        value={sdk[name] === undefined ? NOT_SAID : String(sdk[name])}
        disabled={disabled}
        placeholder={t('applications.drawer.sdk.not_said')}
        onChange={(event) => {
          const value = event.target.value.trim();

          set(withSetting(sdk, name, value === NOT_SAID ? undefined : Number(value)));
        }}
      />
    </FormField>
  );

  return (
    <FormSection title={t('applications.drawer.sdk.title')} description={t('applications.drawer.sdk.description')}>
      {share('sessionSampleRate', t('applications.drawer.sdk.session_sample_rate'), t('applications.drawer.sdk.session_sample_rate_hint'))}
      {share('sessionReplaySampleRate', t('applications.drawer.sdk.session_replay_sample_rate'), t('applications.drawer.sdk.session_replay_sample_rate_hint'))}
      <FormField label={t('applications.drawer.sdk.privacy')}>
        <FormSelect
          ariaLabel={t('applications.drawer.sdk.privacy')}
          disabled={disabled}
          value={sdk.defaultPrivacyLevel ?? NOT_SAID}
          onChange={(value) => set(withSetting(sdk, 'defaultPrivacyLevel', value === NOT_SAID ? undefined : (value as RumPrivacyLevel)))}
          options={[
            { value: NOT_SAID, label: t('applications.drawer.sdk.not_said') },
            { value: 'mask', label: t('applications.drawer.privacy_mask') },
            { value: 'mask-user-input', label: t('applications.drawer.privacy_mask_user_input') },
            { value: 'allow', label: t('applications.drawer.privacy_allow') },
          ]}
        />
      </FormField>
      <FormField label={t('applications.drawer.sdk.clicks')}>
        <FormSelect
          ariaLabel={t('applications.drawer.sdk.clicks')}
          disabled={disabled}
          value={sdk.trackUserInteractions === undefined ? NOT_SAID : String(sdk.trackUserInteractions)}
          onChange={(value) => set(withSetting(sdk, 'trackUserInteractions', value === NOT_SAID ? undefined : value === 'true'))}
          options={[
            { value: NOT_SAID, label: t('applications.drawer.sdk.not_said') },
            { value: 'true', label: t('applications.drawer.sdk.on') },
            { value: 'false', label: t('applications.drawer.sdk.off') },
          ]}
        />
      </FormField>
      <FormField label={t('applications.drawer.sdk.console')} hint={t('applications.drawer.sdk.console_hint')}>
        <FormSelect
          ariaLabel={t('applications.drawer.sdk.console')}
          disabled={disabled}
          value={sdk.forwardConsoleLogs === undefined ? NOT_SAID : (sdk.forwardConsoleLogs[0] ?? 'none')}
          onChange={(value) => {
            const from = CONSOLE_LEVELS.indexOf(value as (typeof CONSOLE_LEVELS)[number]);

            set(withSetting(sdk, 'forwardConsoleLogs', value === NOT_SAID ? undefined : value === 'none' ? [] : CONSOLE_LEVELS.slice(from)));
          }}
          options={[
            { value: NOT_SAID, label: t('applications.drawer.sdk.not_said') },
            { value: 'none', label: t('applications.drawer.sdk.console_none') },
            { value: 'error', label: t('applications.drawer.sdk.console_from', { level: 'error' }) },
            { value: 'warn', label: t('applications.drawer.sdk.console_from', { level: 'warn' }) },
            { value: 'info', label: t('applications.drawer.sdk.console_from', { level: 'info' }) },
            { value: 'debug', label: t('applications.drawer.sdk.console_from', { level: 'debug' }) },
          ]}
        />
      </FormField>
    </FormSection>
  );
}
