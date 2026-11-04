export interface Dashboard {
  id: string;
  orgId: string;
  folderId?: string;
  uid: string;
  title: string;
  tags: string[];
  /** MoleSignal Dashboard Engine JSON。 */
  model: Record<string, unknown>;
  version: number;
  createdAt: number;
  updatedAt: number;
  createdBy?: string;
  updatedBy?: string;
}
