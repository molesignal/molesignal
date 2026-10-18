/**
 * Prices and costs in the currency a model provider is priced in. The
 * organization keeps its currencies (Settings → Models); a symbol it set is
 * used as it is, otherwise symbols and names come from `Intl`, in the reader's
 * language.
 */

export const DEFAULT_CURRENCY = 'USD';

/** The symbol the organization set for a currency, if any. */
export type SymbolOf = (currency: string) => string | null | undefined;

const formats = new Map<string, Intl.NumberFormat>();

function currencyFormat(currency: string, fractionDigits?: number): Intl.NumberFormat {
  const key = `${currency}:${fractionDigits ?? ''}`;
  let format = formats.get(key);
  if (!format) {
    format = new Intl.NumberFormat(undefined, {
      style: 'currency',
      currency,
      ...(fractionDigits === undefined
        ? {}
        : { minimumFractionDigits: 0, maximumFractionDigits: fractionDigits }),
    });
    formats.set(key, format);
  }
  return format;
}

/** The smallest amount a currency is written to: a cent, or a whole yen or won. */
function smallestStep(currency: string): number {
  const digits = currencyFormat(currency).resolvedOptions().maximumFractionDigits ?? 2;
  return 10 ** -digits;
}

/** An amount with a symbol the organization set: the number formatted like the currency's. */
function withSymbol(amount: number, currency: string, symbol: string, fractionDigits?: number): string {
  const number = currencyFormat(currency, fractionDigits)
    .formatToParts(amount)
    .filter((part) => part.type !== 'currency' && part.type !== 'literal')
    .map((part) => part.value)
    .join('');
  return `${symbol}${number}`;
}

/**
 * An amount in its currency: "$1.20", "¥8.50". Below the smallest step it says
 * so ("< $0.01") rather than rounding to nothing.
 */
export function formatMoney(amount: number, currency: string, symbolOf?: SymbolOf): string {
  const symbol = symbolOf?.(currency);
  const write = (value: number) =>
    symbol ? withSymbol(value, currency, symbol) : currencyFormat(currency).format(value);
  const step = smallestStep(currency);
  if (amount > 0 && amount < step) return `< ${write(step)}`;
  return write(Math.max(0, amount));
}

/** A unit price as it was typed, to four decimals: "¥0.5", "$2.5", "$0.075". */
export function formatPrice(amount: number, currency: string, symbolOf?: SymbolOf): string {
  const symbol = symbolOf?.(currency);
  return symbol
    ? withSymbol(amount, currency, symbol, 4)
    : currencyFormat(currency, 4).format(amount);
}

/** The standard symbol of a currency in the reader's locale: "$", "¥", "€", or its code. */
export function standardSymbol(currency: string): string {
  return (
    currencyFormat(currency)
      .formatToParts(0)
      .find((part) => part.type === 'currency')?.value ?? currency
  );
}

/** The symbol of a currency: the one the organization set, else the standard one. */
export function currencySymbol(currency: string, symbolOf?: SymbolOf): string {
  return symbolOf?.(currency) || standardSymbol(currency);
}

/** The standard name of a currency in the reader's language, or its code. */
export function standardName(currency: string, language: string): string {
  try {
    return new Intl.DisplayNames([language], { type: 'currency' }).of(currency) ?? currency;
  } catch {
    return currency;
  }
}

/** "CNY · 人民币": the code and its name, the organization's own when it set one. */
export function currencyLabel(currency: string, language: string, name?: string | null): string {
  const shown = name || standardName(currency, language);
  return shown && shown !== currency ? `${currency} · ${shown}` : currency;
}

/** Costs in more than one currency are never added up: one amount per currency. */
export type Costs = Record<string, number>;

/**
 * Costs as one line, a currency at a time: "$1.20 · ¥8.50". A cost that leaves
 * out calls without prices is only a floor, and says "≥"; no priced call at all
 * is "—", in no currency.
 */
export function formatCosts(costs: Costs, partial = false, symbolOf?: SymbolOf): string {
  const parts = Object.entries(costs)
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([currency, amount]) => formatMoney(amount, currency, symbolOf));
  if (parts.length === 0) return '—';
  const text = parts.join(' · ');
  return partial ? `≥ ${text}` : text;
}
