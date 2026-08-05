import { http } from '@/lib/http';

export interface CipherKey {
  id: string;
  name: string;
  alg: string;
  version: number;
  created_at_micros: number;
  rotated_at_micros?: number;
}

export interface CreateCipherKeyPayload {
  name: string;
  /** Base64-encoded 32-byte key material. */
  key_material_b64: string;
}

export interface RotateCipherKeyPayload {
  key_material_b64: string;
}

export async function list(): Promise<CipherKey[]> {
  const { data } = await http.get<CipherKey[]>('/cipher_keys');
  return data;
}

export async function create(payload: CreateCipherKeyPayload): Promise<CipherKey> {
  const { data } = await http.post<CipherKey>('/cipher_keys', payload);
  return data;
}

export async function rotate(name: string, payload: RotateCipherKeyPayload): Promise<CipherKey> {
  const { data } = await http.post<CipherKey>(
    `/cipher_keys/${encodeURIComponent(name)}/rotate`,
    payload,
  );
  return data;
}

export async function remove(name: string): Promise<void> {
  await http.delete(`/cipher_keys/${encodeURIComponent(name)}`);
}
