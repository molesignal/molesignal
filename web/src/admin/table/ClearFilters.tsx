import type * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ChromeButton } from '@/shell/chrome';

/** The way back from a list that shows nothing because of what is set on it. */
export function ClearFiltersButton({ onClear }: { onClear: () => void }) {
  const { t } = useTranslation('common');

  return (
    <ChromeButton size="sm" onClick={onClear} data-testid="clear-filters">
      {t('table.clear_filters')}
    </ChromeButton>
  );
}

/** For a `DataTable`'s `emptyLabel`: what the filters left out, and the way back. */
export function TableNoMatch({ label, onClear }: { label: React.ReactNode; onClear: () => void }) {
  return (
    <span className="flex flex-col items-center gap-2">
      <span>{label}</span>
      <ClearFiltersButton onClear={onClear} />
    </span>
  );
}
