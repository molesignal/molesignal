import { ChevronDown } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ChromeButton, uiLabelClass } from '@/shell/chrome';
import { FormField, FormInput } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';

import { CopyableValue } from '../../_atoms';

/**
 * A preset's endpoints, which follow from its inputs: shown on request, read-only,
 * with the scopes that stay editable.
 */
export function EndpointList({
  endpoints,
  scopes,
  onScopesChange,
}: {
  endpoints: Array<{ label: string; value: string }>;
  scopes: string[];
  onScopesChange: (scopes: string[]) => void;
}) {
  const { t } = useTranslation('settings-admin');
  const [open, setOpen] = React.useState(false);
  const shown = endpoints.filter((endpoint) => endpoint.value);
  return (
    <div className="flex flex-col gap-3">
      <div>
        <ChromeButton
          type="button"
          variant="ghost"
          size="sm"
          aria-expanded={open}
          onClick={() => setOpen((value) => !value)}
          className="-ml-2"
        >
          {t('sso_providers.drawer.fields.endpoints')}
          <ChevronDown
            aria-hidden
            className={cn('transition-transform duration-200', open && 'rotate-180')}
          />
        </ChromeButton>
      </div>
      {open && (
        <div className="flex flex-col gap-3">
          {shown.length === 0 ? (
            <div className="text-xs leading-relaxed text-tx-3">
              {t('sso_providers.drawer.fields.endpoints_pending')}
            </div>
          ) : (
            shown.map((endpoint) => (
              <div key={endpoint.label} className="flex min-w-0 cursor-default flex-col gap-1.5">
                <span className={uiLabelClass}>{endpoint.label}</span>
                <CopyableValue
                  value={endpoint.value}
                  copyLabel={t('sso_providers.drawer.fields.copy_endpoint')}
                  copiedLabel={t('sso_providers.drawer.fields.endpoint_copied')}
                />
              </div>
            ))
          )}
          <ScopesField scopes={scopes} onChange={onScopesChange} />
        </div>
      )}
    </div>
  );
}

/** Scopes as one line of words; the draft keeps the words. */
export function ScopesField({
  scopes,
  onChange,
}: {
  scopes: string[];
  onChange: (scopes: string[]) => void;
}) {
  const { t } = useTranslation('settings-admin');
  const [text, setText] = React.useState(scopes.join(' '));
  const joined = scopes.join(' ');
  // Keep typed spacing while editing; follow the draft when it changes elsewhere.
  React.useEffect(() => {
    setText((current) =>
      current.split(/\s+/).filter(Boolean).join(' ') === joined ? current : joined,
    );
  }, [joined]);
  return (
    <FormField
      label={t('sso_providers.drawer.fields.scopes')}
      hint={t('sso_providers.drawer.fields.scopes_hint')}
    >
      <FormInput
        value={text}
        onChange={(event) => {
          setText(event.target.value);
          onChange(event.target.value.split(/\s+/).filter(Boolean));
        }}
        spellCheck={false}
      />
    </FormField>
  );
}
