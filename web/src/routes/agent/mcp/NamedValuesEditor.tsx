import { Lock, LockOpen, Plus, X } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { FormInput } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import { Button } from '@/shell/ui/button';

import { emptyRow, looksSecret, type NamedValueRow } from './namedValues';

/**
 * Rows of names and values: the headers of a server, or the environment of a
 * stdio one. A name that reads as a secret keeps its value hidden; a saved
 * secret shows only its last four characters and stays when left empty.
 */
export function NamedValuesEditor({
  rows,
  onChange,
  kind,
}: {
  rows: NamedValueRow[];
  onChange: (rows: NamedValueRow[]) => void;
  kind: 'header' | 'env';
}) {
  const { t } = useTranslation('agent');
  const update = (key: string, change: Partial<NamedValueRow>) =>
    onChange(rows.map((row) => (row.key === key ? { ...row, ...change } : row)));

  return (
    <div className="space-y-2" data-agent-mcp-values={kind}>
      {rows.map((row) => {
        const forced = looksSecret(row.name);
        const secret = row.secret || forced;
        return (
          <div key={row.key} className="flex items-center gap-2">
            <FormInput
              aria-label={t(`settings.tools.mcp.values.${kind}_name`)}
              value={row.name}
              onChange={(event) => update(row.key, { name: event.target.value })}
              placeholder={t(`settings.tools.mcp.values.${kind}_name_placeholder`)}
              className="w-[38%] min-w-0 font-mono"
            />
            <FormInput
              aria-label={t('settings.tools.mcp.values.value')}
              type={secret ? 'password' : 'text'}
              autoComplete="new-password"
              value={row.value}
              onChange={(event) => update(row.key, { value: event.target.value })}
              placeholder={
                row.saved
                  ? t('settings.tools.mcp.values.saved', { last4: row.saved.last4 ?? '' })
                  : t('settings.tools.mcp.values.value_placeholder')
              }
              className="min-w-0 flex-1 font-mono"
            />
            <Button
              type="button"
              variant="ghost"
              size="icon"
              aria-pressed={secret}
              disabled={forced}
              title={
                forced
                  ? t('settings.tools.mcp.values.secret_forced')
                  : t('settings.tools.mcp.values.secret_toggle')
              }
              aria-label={t('settings.tools.mcp.values.secret_toggle')}
              onClick={() => update(row.key, { secret: !row.secret })}
              className={cn('shrink-0', secret ? 'text-indigo' : 'text-tx-3')}
            >
              {secret ? <Lock className="h-3.5 w-3.5" /> : <LockOpen className="h-3.5 w-3.5" />}
            </Button>
            <Button
              type="button"
              variant="ghost"
              size="icon"
              aria-label={t('settings.tools.mcp.values.remove')}
              onClick={() => onChange(rows.filter((item) => item.key !== row.key))}
              className="shrink-0 text-tx-3"
            >
              <X className="h-3.5 w-3.5" />
            </Button>
          </div>
        );
      })}
      <Button
        type="button"
        size="sm"
        variant="outline"
        onClick={() => onChange([...rows, emptyRow()])}
      >
        <Plus className="h-3.5 w-3.5" />
        {t(`settings.tools.mcp.values.add_${kind}`)}
      </Button>
    </div>
  );
}
