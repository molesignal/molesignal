import { useTranslation } from 'react-i18next';

import type { ModelProvider } from '@/api/agent/modelProviders';

import { DEFAULT_CURRENCY, formatPrice, type SymbolOf } from './money';

/** A model provider's unit prices on one line, in its currency; or that it has none. */
export function ProviderPrices({
  provider,
  symbolOf,
}: {
  provider: ModelProvider;
  symbolOf: SymbolOf;
}) {
  const { t } = useTranslation('agent');
  const currency = provider.price_currency || DEFAULT_CURRENCY;
  const price = (value: number | null | undefined) =>
    formatPrice(value ?? 0, currency, symbolOf);

  if (provider.input_price_per_mtok == null && provider.output_price_per_mtok == null) {
    return (
      <div data-agent-provider-prices="" className="mt-0.5 text-xs text-tx-3">
        {t('settings.models.unpriced')}
      </div>
    );
  }

  return (
    <div data-agent-provider-prices={currency} className="mt-0.5 text-xs text-tx-2">
      {provider.cached_input_price_per_mtok == null
        ? t('settings.models.price_summary_flat', {
            input: price(provider.input_price_per_mtok),
            output: price(provider.output_price_per_mtok),
          })
        : t('settings.models.price_summary', {
            input: price(provider.input_price_per_mtok),
            cached: price(provider.cached_input_price_per_mtok),
            output: price(provider.output_price_per_mtok),
          })}
    </div>
  );
}
