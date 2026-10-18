import { useQuery } from '@tanstack/react-query';
import * as React from 'react';

import * as currenciesApi from '@/api/agent/currencies';

import type { SymbolOf } from './money';

export const CURRENCIES_KEY = ['agent', 'currencies'] as const;

/** The organization's currencies, and the symbols it set for them. */
export function useCurrencies() {
  const query = useQuery({
    queryKey: CURRENCIES_KEY,
    queryFn: currenciesApi.list,
    staleTime: 60_000,
    retry: false,
  });
  const currencies = React.useMemo(() => query.data ?? [], [query.data]);
  const symbolOf = React.useCallback<SymbolOf>(
    (code) => currencies.find((currency) => currency.code === code)?.symbol,
    [currencies],
  );
  return { query, currencies, symbolOf };
}
