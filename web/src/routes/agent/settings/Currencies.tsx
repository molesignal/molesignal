import { useMutation, useQueryClient } from '@tanstack/react-query';
import { Pencil, Plus, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog } from '@/admin';
import * as currenciesApi from '@/api/agent/currencies';
import { toApiError } from '@/lib/http';
import { ProductState } from '@/product/states';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormRow,
  FormSection,
  FormSubmitFooter,
} from '@/shell/FormDrawer';
import { Button } from '@/shell/ui/button';
import { toast } from '@/shell/ui/sonner';

import { standardName, standardSymbol } from './money';
import { AgentSettingsSection } from './Surfaces';
import { CURRENCIES_KEY, useCurrencies } from './useCurrencies';

type EditorTarget = currenciesApi.PriceCurrency | 'new' | null;

/**
 * The currencies model providers can be priced in: the organization's own list,
 * with its own names and symbols where the standard ones do not suit. One in
 * use by a model provider cannot be deleted.
 */
export function CurrenciesSection({ canManage }: { canManage: boolean }) {
  const { t, i18n } = useTranslation('agent');
  const queryClient = useQueryClient();
  const { query, currencies } = useCurrencies();
  const [editing, setEditing] = React.useState<EditorTarget>(null);
  const [deleting, setDeleting] = React.useState<currenciesApi.PriceCurrency | null>(null);

  const remove = useMutation({
    mutationFn: (code: string) => currenciesApi.remove(code),
    onSuccess: async () => {
      setDeleting(null);
      toast.success(t('settings.currencies.deleted'));
      await queryClient.invalidateQueries({ queryKey: CURRENCIES_KEY });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  return (
    <AgentSettingsSection
      title={t('settings.currencies.title')}
      description={t('settings.currencies.description')}
      action={
        canManage && (
          <Button size="sm" onClick={() => setEditing('new')}>
            <Plus />
            {t('settings.currencies.add')}
          </Button>
        )
      }
    >
      {query.isLoading ? (
        <ProductState variant="loading" />
      ) : query.isError ? (
        <ProductState variant="error" error={query.error} />
      ) : (
        <ul data-agent-currencies className="divide-y divide-bd-0 overflow-hidden rounded-lg bg-bg-2">
          {currencies.map((currency) => (
            <li
              key={currency.code}
              data-agent-currency={currency.code}
              className="flex min-h-11 items-center gap-3 px-4 py-2"
            >
              <span className="w-12 shrink-0 font-mono text-xs font-strong text-tx-0">
                {currency.code}
              </span>
              <span className="w-8 shrink-0 text-xs text-tx-1">
                {currency.symbol || standardSymbol(currency.code)}
              </span>
              <span className="min-w-0 flex-1 truncate text-xs text-tx-1">
                {currency.name || standardName(currency.code, i18n.language)}
              </span>
              <span className="shrink-0 text-xs text-tx-3">
                {currency.used_by.length > 0
                  ? t('settings.currencies.used_by', { count: currency.used_by.length })
                  : t('settings.currencies.unused')}
              </span>
              {canManage && (
                <span className="flex shrink-0 items-center gap-0.5">
                  <Button
                    variant="ghost"
                    size="icon"
                    aria-label={t('settings.currencies.edit', { code: currency.code })}
                    onClick={() => setEditing(currency)}
                  >
                    <Pencil />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    aria-label={t('settings.currencies.delete', { code: currency.code })}
                    disabled={currency.used_by.length > 0}
                    title={
                      currency.used_by.length > 0
                        ? t('settings.currencies.in_use', { names: currency.used_by.join('、') })
                        : undefined
                    }
                    onClick={() => setDeleting(currency)}
                  >
                    <Trash2 />
                  </Button>
                </span>
              )}
            </li>
          ))}
        </ul>
      )}

      {editing && <CurrencyEditor target={editing} onClose={() => setEditing(null)} />}
      <ConfirmDialog
        open={deleting !== null}
        onOpenChange={(open) => !open && setDeleting(null)}
        destructive
        busy={remove.isPending}
        title={t('settings.currencies.delete_title', { code: deleting?.code ?? '' })}
        description={t('settings.currencies.delete_confirm')}
        cancelLabel={t('common.cancel')}
        confirmLabel={t('settings.currencies.delete_action')}
        busyLabel={t('settings.currencies.deleting')}
        onConfirm={() => deleting && remove.mutate(deleting.code)}
      />
    </AgentSettingsSection>
  );
}

/** Adds a currency, or changes the name and symbol of one; the code is set once. */
function CurrencyEditor({
  target,
  onClose,
}: {
  target: Exclude<EditorTarget, null>;
  onClose: () => void;
}) {
  const { t, i18n } = useTranslation('agent');
  const queryClient = useQueryClient();
  const creating = target === 'new';
  const [code, setCode] = React.useState(creating ? '' : target.code);
  const [name, setName] = React.useState(creating ? '' : (target.name ?? ''));
  const [symbol, setSymbol] = React.useState(creating ? '' : (target.symbol ?? ''));
  const validCode = /^[A-Z]{3}$/.test(code);

  const save = useMutation({
    mutationFn: () =>
      currenciesApi.save(code, { name: name.trim() || null, symbol: symbol.trim() || null }),
    onSuccess: async () => {
      toast.success(t('settings.currencies.saved'));
      await queryClient.invalidateQueries({ queryKey: CURRENCIES_KEY });
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const formId = 'agent-currency-editor';
  return (
    <FormDrawer
      open
      onOpenChange={(open) => {
        if (!open && !save.isPending) onClose();
      }}
      title={t(creating ? 'settings.currencies.add' : 'settings.currencies.edit_title')}
      subtitle={t('settings.currencies.editor_description')}
      width={480}
      footer={
        <FormSubmitFooter
          busy={save.isPending}
          onCancel={onClose}
          formId={formId}
          submitLabel={t(creating ? 'common.create' : 'common.save')}
        />
      }
    >
      <form
        id={formId}
        onSubmit={(event) => {
          event.preventDefault();
          if (validCode) save.mutate();
        }}
      >
        <FormSection title={t('settings.currencies.title')}>
          <FormRow>
            <FormField label={t('settings.currencies.fields.code')} required>
              <FormInput
                value={code}
                disabled={!creating}
                maxLength={3}
                autoComplete="off"
                placeholder="AUD"
                onChange={(event) => setCode(event.target.value.toUpperCase().replace(/[^A-Z]/g, ''))}
              />
            </FormField>
          </FormRow>
          <FormRow>
            <FormField label={t('settings.currencies.fields.name')}>
              <FormInput
                value={name}
                maxLength={64}
                placeholder={validCode ? standardName(code, i18n.language) : ''}
                onChange={(event) => setName(event.target.value)}
              />
            </FormField>
            <FormField label={t('settings.currencies.fields.symbol')}>
              <FormInput
                value={symbol}
                maxLength={8}
                placeholder={validCode ? standardSymbol(code) : ''}
                onChange={(event) => setSymbol(event.target.value)}
              />
            </FormField>
          </FormRow>
        </FormSection>
      </form>
    </FormDrawer>
  );
}
