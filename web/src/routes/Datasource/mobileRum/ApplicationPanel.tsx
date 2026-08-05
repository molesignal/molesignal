import { useTranslation } from 'react-i18next';

import { ChromeButton } from '@/shell/chrome';
import { FormInput } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';

export function ApplicationPanel({
  value,
  valid,
  confirmed,
  onChange,
  onConfirm,
}: {
  value: string;
  valid: boolean;
  confirmed: boolean;
  onChange: (value: string) => void;
  onConfirm: () => void;
}) {
  const { t } = useTranslation('onboarding');
  const showError = value.trim().length > 0 && !valid;
  return (
    <div className="min-w-0 rounded-md border border-bd-0 bg-bg-1 p-3">
      <div className="mb-2 flex items-center justify-between gap-2">
        <label htmlFor="rum-application-id" className="font-sans text-xs font-strong text-tx-2">
          {t('datasource_page.rum_application_id')}
        </label>
        <span className={cn('font-sans text-xs', confirmed ? 'text-green-soft' : 'text-tx-3')}>
          {confirmed ? t('datasource_page.ready') : t('datasource_page.required')}
        </span>
      </div>
      <FormInput
        id="rum-application-id"
        value={value}
        onChange={(event) => onChange(event.target.value)}
        placeholder="checkout-mobile"
        pattern="[A-Za-z0-9._:-]{1,128}"
        maxLength={128}
        autoComplete="off"
        aria-invalid={showError || undefined}
      />
      <div className="mt-2 flex justify-end">
        <ChromeButton
          type="button"
          size="sm"
          disabled={!valid || confirmed}
          onClick={onConfirm}
        >
          {confirmed
            ? t('datasource_page.rum_application_confirmed')
            : t('datasource_page.rum_application_confirm')}
        </ChromeButton>
      </div>
      <p className={cn('mt-2 font-sans text-xs leading-relaxed', showError ? 'text-red-soft' : 'text-tx-2')}>
        {showError
          ? t('datasource_page.rum_application_invalid')
          : valid && !confirmed
            ? t('datasource_page.rum_application_confirm_hint')
            : t('datasource_page.rum_application_hint')}
      </p>
    </div>
  );
}
