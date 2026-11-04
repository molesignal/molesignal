import { useTranslation } from 'react-i18next';

import { uiLabelClass } from '@/shell/chrome';
import { FieldArray, FormField, FormInput } from '@/shell/FormDrawer';
import { ViewTabs } from '@/shell/tabs/ViewTabs';

import {
  FILLED_BY_POSITION,
  TEMPLATE_PROVIDERS,
  type ProviderDraft,
  type ProviderParam,
  type ProviderProblem,
  type TemplateProvider,
} from './providerDrafts';

/** What a provider's template code looks like. */
const CODE_PLACEHOLDER: Record<TemplateProvider, Record<'sms' | 'phone', string>> = {
  aliyun: { sms: 'SMS_123456789', phone: 'TTS_123456789' },
  tencent: { sms: '1110', phone: '4356' },
  huawei: { sms: '8ff55eac1d0b478ab3c06c3c6a492300', phone: 'alert_notice' },
};

/**
 * The approved templates of an SMS or phone template: Alibaba Cloud, Tencent Cloud and Huawei Cloud
 * send only templates they approved, so their connectors send this code with these variables, and
 * other providers the body. One provider is shown at a time; each keeps what was written for it.
 * Not a `<label>` around the variables: a click on their heading would land in the first one.
 */
export function ProviderTemplateFields({
  channel,
  provider,
  onProvider,
  draft,
  problem,
  problemElsewhere,
  onChange,
  onFocusValue,
}: {
  channel: 'sms' | 'phone';
  provider: TemplateProvider;
  onProvider: (provider: TemplateProvider) => void;
  draft: ProviderDraft;
  problem: ProviderProblem | null;
  /** Another provider's template is what keeps this one from being saved. */
  problemElsewhere: TemplateProvider | null;
  onChange: (draft: ProviderDraft) => void;
  /** A variable's value took the focus: placeholders picked from the panel go into it. */
  onFocusValue: (input: HTMLInputElement, index: number) => void;
}) {
  const { t } = useTranslation('notify');

  return (
    <section
      data-provider-template={provider}
      className="space-y-4 rounded-md border border-bd-0 bg-bg-2 p-4"
    >
      <div>
        <h3 className="text-xs font-semibold text-tx-0">{t('templates.provider.title')}</h3>
        <p className="mt-1 text-xs leading-relaxed text-tx-3">
          {t(`templates.provider.description_${channel}`)}
        </p>
      </div>
      <ViewTabs<TemplateProvider>
        label={t('templates.provider.title')}
        value={provider}
        onValueChange={onProvider}
        items={TEMPLATE_PROVIDERS.map((key) => ({ key, label: t(`templates.provider.names.${key}`) }))}
      />
      <div className="grid gap-4 sm:grid-cols-[minmax(0,1fr)_180px]">
        <FormField label={t('templates.provider.code')}>
          <FormInput
            className="font-mono"
            placeholder={CODE_PLACEHOLDER[provider][channel]}
            value={draft.code}
            onChange={(event) => onChange({ ...draft, code: event.target.value })}
          />
        </FormField>
        <FormField label={t('templates.provider.max_chars')}>
          <FormInput
            type="number"
            min={1}
            max={500}
            placeholder={t(`templates.provider.max_chars_placeholder.${provider}_${channel}`)}
            value={draft.maxChars}
            onChange={(event) => onChange({ ...draft, maxChars: event.target.value })}
          />
        </FormField>
      </div>
      <div data-provider-params className="flex flex-col gap-1.5">
        <span className={uiLabelClass}>{t('templates.provider.params')}</span>
        <FieldArray<ProviderParam>
          items={draft.params}
          onChange={(params) => onChange({ ...draft, params })}
          newItem={() => ({ name: '', value: '' })}
          addLabel={t('templates.provider.add_param')}
          removeLabel={t('templates.provider.remove_param')}
          renderItem={(param, index, setParam) => (
            <div className="grid grid-cols-[150px_minmax(0,1fr)] gap-2">
              <FormInput
                aria-label={t('templates.provider.param_name')}
                placeholder={FILLED_BY_POSITION.has(provider) ? '1' : t('templates.provider.param_name')}
                className="font-mono"
                value={param.name}
                onChange={(event) => setParam({ ...param, name: event.target.value })}
              />
              <FormInput
                aria-label={t('templates.provider.param_value')}
                placeholder="{{rule.name}}"
                className="font-mono"
                value={param.value}
                onFocus={(event) => onFocusValue(event.currentTarget, index)}
                onChange={(event) => setParam({ ...param, value: event.target.value })}
              />
            </div>
          )}
        />
        <span className="text-xs leading-relaxed text-tx-3">
          {FILLED_BY_POSITION.has(provider)
            ? t('templates.provider.params_hint_position')
            : t('templates.provider.params_hint', { variable: '${name}' })}
        </span>
      </div>
      {problem && (
        <p role="alert" className="text-xs text-red-soft">
          {t(`templates.provider.problems.${problem}`)}
        </p>
      )}
      {!problem && problemElsewhere && (
        <p role="alert" className="text-xs text-red-soft">
          {t('templates.provider.problem_elsewhere', { provider: t(`templates.provider.names.${problemElsewhere}`) })}
        </p>
      )}
    </section>
  );
}
