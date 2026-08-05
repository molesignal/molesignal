import { http } from '@/lib/http';

export interface ModelPrice {
  provider: string;
  model: string;
  prompt_usd_per_1k: number;
  completion_usd_per_1k: number;
  updated_at_micros: number;
}

interface ModelPriceWire extends Omit<ModelPrice, 'updated_at_micros'> {
  updated_at?: number;
  updated_at_micros?: number;
}

export interface UpsertModelPriceInput {
  provider: string;
  model: string;
  prompt_usd_per_1k: number;
  completion_usd_per_1k: number;
}

export async function list(): Promise<ModelPrice[]> {
  const { data } = await http.get<ModelPriceWire[]>('/model_prices');
  return data.map(normalize);
}

export async function upsert(input: UpsertModelPriceInput): Promise<ModelPrice> {
  const { data } = await http.post<ModelPriceWire>('/model_prices', input);
  return normalize(data);
}

export async function remove(provider: string, model: string): Promise<void> {
  await http.delete(
    `/model_prices/${encodeURIComponent(provider)}/${encodeURIComponent(model)}`,
  );
}

function normalize(row: ModelPriceWire): ModelPrice {
  return {
    ...row,
    updated_at_micros: row.updated_at_micros ?? row.updated_at ?? 0,
  };
}
