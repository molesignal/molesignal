import { useTranslation } from 'react-i18next';

import type { ModelProvider } from '@/api/agent/modelProviders';

import { DEFAULT_CURRENCY, formatPrice, type SymbolOf } from '../money';

/** A model provider's unit prices on one line, in its currency; or that it has none. */
export function ProviderPrices({
  provider,
  symbolOf,
}: {
  provider: ModelProvider;
  symbolOf: SymbolOf;
}) {
  const { t } = useTranslation('agent');
  const currency = provider.priceCurrency || DEFAULT_CURRENCY;
  const price = (value: number | null | undefined) =>
    formatPrice(value ?? 0, currency, symbolOf);

  if (provider.inputPricePerMtok == null && provider.outputPricePerMtok == null) {
    return (
      <div data-agent-provider-prices="" className="mt-0.5 text-xs text-tx-3">
        {t('settings.models.unpriced')}
      </div>
    );
  }

  return (
    <div data-agent-provider-prices={currency} className="mt-0.5 text-xs text-tx-2">
      {provider.cachedInputPricePerMtok == null
        ? t('settings.models.price_summary_flat', {
            input: price(provider.inputPricePerMtok),
            output: price(provider.outputPricePerMtok),
          })
        : t('settings.models.price_summary', {
            input: price(provider.inputPricePerMtok),
            cached: price(provider.cachedInputPricePerMtok),
            output: price(provider.outputPricePerMtok),
          })}
    </div>
  );
}
