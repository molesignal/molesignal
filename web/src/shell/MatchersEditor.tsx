import { useTranslation } from 'react-i18next';

import { FieldArray, FormInput, FormSelect } from '@/shell/FormDrawer';
import type { LabelMatcher, MatchOp } from '@/types/alerting';

const OP_OPTIONS: Array<{ value: MatchOp; label: string }> = [
  { value: 'eq', label: '= (eq)' },
  { value: 'neq', label: '≠ (neq)' },
  { value: 're', label: '=~ (regex)' },
  { value: 'nre', label: '≠~ (neg regex)' },
];

/**
 * Shared label-matcher list editor for silence and routing policies. Each row is
 * a `{label, op, value}` triple; all rows AND together on the backend.
 */
export function MatchersEditor({
  matchers,
  onChange,
}: {
  matchers: LabelMatcher[];
  onChange: (next: LabelMatcher[]) => void;
}) {
  const { t } = useTranslation('alerts');
  return (
    <div className="flex flex-col gap-1.5">
      <span className="font-sans text-xs font-strong uppercase tracking-wide text-tx-3">
        {t('matchers.title', { defaultValue: 'Label matchers' })}
      </span>
      <FieldArray<LabelMatcher>
        items={matchers}
        onChange={onChange}
        minItems={0}
        addLabel={t('matchers.add', { defaultValue: 'Add matcher' })}
        removeLabel={t('matchers.remove', { defaultValue: 'Remove matcher' })}
        newItem={() => ({ label: '', op: 'eq', value: '' })}
        renderItem={(m, _i, setM) => (
          <div className="flex items-center gap-2">
            <FormInput
              value={m.label}
              onChange={(e) => setM({ ...m, label: e.target.value })}
              placeholder={t('matchers.label_placeholder', { defaultValue: 'label (e.g. service)' })}
              className="flex-1"
              aria-label={t('matchers.label_aria', { defaultValue: 'Matcher label' })}
            />
            <div className="w-32 shrink-0">
              <FormSelect value={m.op} onChange={(v) => setM({ ...m, op: v as MatchOp })} options={OP_OPTIONS} />
            </div>
            <FormInput
              value={m.value}
              onChange={(e) => setM({ ...m, value: e.target.value })}
              placeholder={t('matchers.value_placeholder', { defaultValue: 'value' })}
              className="flex-1"
              aria-label={t('matchers.value_aria', { defaultValue: 'Matcher value' })}
            />
          </div>
        )}
      />
      <span className="font-sans text-xs text-tx-3">
        {t('matchers.hint', { defaultValue: 'All matchers must hit (AND). =~ / ≠~ are regular expressions.' })}
      </span>
    </div>
  );
}
