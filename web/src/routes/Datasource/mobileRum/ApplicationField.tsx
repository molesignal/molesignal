import { useTranslation } from 'react-i18next';

import { ChromeButton } from '@/shell/chrome';
import { FormInput } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';

/**
 * The application a RUM token is bound to. It is typed, then confirmed: a
 * token is issued per application, so one is not asked for on every keystroke.
 */
export function ApplicationField({
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
    <>
      <div className="flex min-w-0 items-center gap-2">
        <FormInput
          id="rum-application-id"
          value={value}
          onChange={(event) => onChange(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter' && valid && !confirmed) onConfirm();
          }}
          placeholder="checkout-mobile"
          pattern="[A-Za-z0-9._:-]{1,128}"
          maxLength={128}
          autoComplete="off"
          aria-invalid={showError || undefined}
          className="font-mono text-xs"
        />
        <ChromeButton type="button" disabled={!valid || confirmed} onClick={onConfirm}>
          {confirmed
            ? t('datasource_page.rum_application_confirmed')
            : t('datasource_page.rum_application_confirm')}
        </ChromeButton>
      </div>
      <p
        className={cn(
          'm-0 mt-[6px] font-sans text-xs leading-relaxed',
          showError ? 'text-red-soft' : 'text-tx-3',
        )}
      >
        {showError
          ? t('datasource_page.rum_application_invalid')
          : valid && !confirmed
            ? t('datasource_page.rum_application_confirm_hint')
            : t('datasource_page.rum_application_hint')}
      </p>
    </>
  );
}
