import { useTranslation } from 'react-i18next';

import { Checkbox } from '@/shell/ui/checkbox';

export function SourceRetention({ source, checked, onChange }: {
  source: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  const { t } = useTranslation('pipelines');
  return <>
    <label className="flex items-center gap-2 text-xs text-tx-1">
      <Checkbox checked={checked} onCheckedChange={(value) => onChange(value === true)} />
      {t('realtime.retain')}
    </label>
    <p className="text-xs text-tx-3">{t('realtime.retain_hint', { source })}</p>
  </>;
}
