import { http } from '@/lib/http';

/** A currency the organization prices model providers in. */
export interface PriceCurrency {
  /** ISO 4217: three capital letters. */
  code: string;
  /** The organization's own name for it; null means the standard name. */
  name: string | null;
  /** The organization's own symbol; null means the standard symbol. */
  symbol: string | null;
  /** The model providers priced in it: it cannot be deleted while there are any. */
  used_by: string[];
  updated_at_micros: number;
}

/** The organization's currencies, the first added first; the common ones on a first read. */
export async function list(): Promise<PriceCurrency[]> {
  const { data } = await http.get<{ currencies: PriceCurrency[] }>('/agent/settings/currencies');
  return data.currencies;
}

/** Adds a currency, or changes its name and symbol; empty means the standard ones. */
export async function save(
  code: string,
  input: { name: string | null; symbol: string | null },
): Promise<PriceCurrency> {
  const { data } = await http.put<PriceCurrency>(
    `/agent/settings/currencies/${encodeURIComponent(code)}`,
    input,
  );
  return data;
}

export async function remove(code: string): Promise<{ deleted: boolean }> {
  const { data } = await http.delete<{ deleted: boolean }>(
    `/agent/settings/currencies/${encodeURIComponent(code)}`,
  );
  return data;
}
