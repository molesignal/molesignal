export interface Dashboard {
  id: string;
  org_id: string;
  folder_id?: string;
  uid: string;
  title: string;
  tags: string[];
  /** MoleSignal Dashboard Engine JSON。 */
  model: Record<string, unknown>;
  version: number;
  created_at: number;
  updated_at: number;
  created_by?: string;
  updated_by?: string;
}
