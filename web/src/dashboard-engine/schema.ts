export const DASHBOARD_ENGINE_KIND = 'molesignal-dashboard' as const;
export const DASHBOARD_SCHEMA_VERSION = 2;

export type DashboardTimezone = 'browser' | 'utc' | (string & {});

export interface DashboardTimeSettings {
  defaultFrom: string;
  defaultTo: string;
  timezone: DashboardTimezone;
  hideTimePicker?: boolean | undefined;
  fiscalYearStartMonth?: number | undefined;
}

export interface DashboardRefreshSettings {
  enabled: boolean;
  mode: 'off' | 'interval' | 'live';
  defaultInterval?: string | undefined;
  allowedIntervals: string[];
}

export type DashboardCursorSyncMode = 'off' | 'shared_crosshair';

export const DEFAULT_DASHBOARD_CURSOR_SYNC_MODE: DashboardCursorSyncMode =
  'off';

export interface DashboardInteractionSettings {
  cursorSync: DashboardCursorSyncMode;
}

export interface DashboardLayout {
  type: 'grid';
  columns: number;
  rowHeight: number;
  gap: number;
}

export interface GridPosition {
  x: number;
  y: number;
  w: number;
  h: number;
  minW?: number | undefined;
  minH?: number | undefined;
  maxW?: number | undefined;
  maxH?: number | undefined;
}

export type DashboardVariableType =
  | 'query'
  | 'custom'
  | 'constant'
  | 'text'
  | 'interval'
  | 'data_source';

export interface VariableOption {
  label: string;
  value: unknown;
  selected?: boolean | undefined;
}

export interface DashboardVariable {
  id: string;
  name: string;
  label: string;
  type: DashboardVariableType;
  query?: Record<string, unknown> | undefined;
  options?: VariableOption[] | undefined;
  defaultValue?: unknown;
  currentValue?: unknown;
  multi: boolean;
  includeAll: boolean;
  hide: 'none' | 'label' | 'variable';
  refresh: 'never' | 'dashboard_load' | 'time_range_change';
  dependsOn?: string[] | undefined;
}

export interface DashboardAnnotation {
  id: string;
  name: string;
  enabled: boolean;
  source:
    | 'alerts'
    | 'deployments'
    | 'incidents'
    | 'maintenance'
    | 'custom';
  query?: Record<string, unknown> | undefined;
  color?: string | undefined;
  display: 'line' | 'region' | 'marker';
}

export interface DashboardLink {
  id: string;
  title: string;
  type: 'dashboard' | 'external';
  url: string;
  includeTimeRange: boolean;
  includeVariables: boolean;
  openInNewTab: boolean;
}

export type PanelDataSourceType =
  | 'metrics'
  | 'logs'
  | 'traces'
  | 'profiles'
  | 'sql';

export interface SharedQuery {
  sourcePanelId: string;
  sourceRefId: string;
}

export interface PanelQuery {
  refId: string;
  enabled: boolean;
  dataSourceType: PanelDataSourceType;
  dataSourceId?: string | undefined;
  query: Record<string, unknown>;
  legend?: string | undefined;
  format?: string | undefined;
  sharedQuery?: SharedQuery | undefined;
}

export interface PanelQueryOptions {
  maxDataPoints?: number | undefined;
  minInterval?: string | undefined;
  cacheTimeout?: string | undefined;
  timeoutMs?: number | undefined;
}

export type TransformationType =
  | 'filter_fields'
  | 'rename_fields'
  | 'organize_fields'
  | 'calculate_field'
  | 'reduce'
  | 'group_by'
  | 'sort_by'
  | 'limit'
  | 'join'
  | 'merge'
  | 'labels_to_fields'
  | 'rows_to_fields'
  | 'time_series_to_table';

export interface TransformationConfig {
  id: string;
  type: TransformationType;
  disabled?: boolean | undefined;
  options: Record<string, unknown>;
}

export type VisualizationType =
  | 'time_series'
  | 'table'
  | 'logs'
  | 'stat'
  | 'gauge'
  | 'bar_gauge'
  | 'bar_chart'
  | 'heatmap'
  | 'state_timeline'
  | 'text';

export interface VisualizationConfig {
  type: VisualizationType;
  schemaVersion: number;
  options: Record<string, unknown>;
}

export interface ColorConfig {
  mode: 'fixed' | 'palette' | 'thresholds' | 'continuous';
  value?: string | undefined;
  scheme?: string | undefined;
}

export interface ThresholdStep {
  value: number | null;
  color: string;
  label?: string | undefined;
}

export interface ThresholdConfig {
  mode: 'absolute' | 'percentage';
  steps: ThresholdStep[];
}

export interface ValueMappingResult {
  text?: string | undefined;
  color?: string | undefined;
  icon?: string | undefined;
}

export type ValueMapping =
  | {
      type: 'value';
      value: unknown;
      result: ValueMappingResult;
    }
  | {
      type: 'range';
      from?: number | undefined;
      to?: number | undefined;
      result: ValueMappingResult;
    }
  | {
      type: 'regex';
      pattern: string;
      result: ValueMappingResult;
    }
  | {
      type: 'special';
      match: 'null' | 'nan' | 'true' | 'false' | 'empty';
      result: ValueMappingResult;
    };

export interface FieldConfig {
  displayName?: string | undefined;
  unit?: string | undefined;
  decimals?: number | undefined;
  min?: number | undefined;
  max?: number | undefined;
  softMin?: number | undefined;
  softMax?: number | undefined;
  color?: ColorConfig | undefined;
  thresholds?: ThresholdConfig | undefined;
  mappings?: ValueMapping[] | undefined;
  noValue?: string | undefined;
  custom?: Record<string, unknown> | undefined;
}

export type FieldType =
  | 'time'
  | 'number'
  | 'string'
  | 'boolean'
  | 'json'
  | 'enum';

export type FieldOverrideMatcher =
  | { type: 'field_name'; value: string }
  | { type: 'field_regex'; value: string }
  | { type: 'field_type'; value: FieldType }
  | { type: 'query_ref'; value: string };

export interface FieldOverrideProperty {
  id: keyof FieldConfig | (string & {});
  value: unknown;
}

export interface FieldOverride {
  id: string;
  matcher: FieldOverrideMatcher;
  properties: FieldOverrideProperty[];
}

export interface DataLink {
  id: string;
  title: string;
  target:
    | 'logs'
    | 'metrics'
    | 'traces'
    | 'profiles'
    | 'dashboard'
    | 'external';
  url?: string | undefined;
  variables: Record<string, string>;
  includeTimeRange: boolean;
  includeDashboardVariables: boolean;
  openInNewTab: boolean;
}

export interface PanelRepeatConfig {
  variable: string;
  direction: 'horizontal' | 'vertical' | 'grid';
  maxPerRow?: number | undefined;
}

export interface PanelTimeOverride {
  relativeTime?: string | undefined;
  timeShift?: string | undefined;
  hideTimeInfo?: boolean | undefined;
}

interface DashboardElementBase {
  id: string;
  title: string;
  description?: string | undefined;
  gridPos: GridPosition;
}

export interface DashboardPanel extends DashboardElementBase {
  kind: 'panel';
  queryOptions: PanelQueryOptions;
  queries: PanelQuery[];
  transformations: TransformationConfig[];
  visualization: VisualizationConfig;
  fieldConfig: FieldConfig;
  overrides: FieldOverride[];
  links: DataLink[];
  repeat?: PanelRepeatConfig | undefined;
  timeOverride?: PanelTimeOverride | undefined;
  transparent?: boolean | undefined;
  collapsed?: boolean | undefined;
}

export interface DashboardTextElement extends DashboardElementBase {
  kind: 'text';
  content: string;
  mode: 'markdown' | 'plain';
  transparent?: boolean | undefined;
}

export interface DashboardGroup extends DashboardElementBase {
  kind: 'group';
  collapsed?: boolean | undefined;
  repeat?: PanelRepeatConfig | undefined;
  elements: DashboardElement[];
}

export interface DashboardRow extends DashboardElementBase {
  kind: 'row';
  collapsed: boolean;
  elements: DashboardElement[];
}

export interface DashboardTabItem {
  id: string;
  title: string;
  elements: DashboardElement[];
}

export interface DashboardTab extends DashboardElementBase {
  kind: 'tab';
  defaultTabId?: string | undefined;
  tabs: DashboardTabItem[];
}

export type DashboardElement =
  | DashboardPanel
  | DashboardGroup
  | DashboardRow
  | DashboardTab
  | DashboardTextElement;

export interface DashboardDefinition {
  engine: typeof DASHBOARD_ENGINE_KIND;
  schemaVersion: typeof DASHBOARD_SCHEMA_VERSION;
  id: string;
  uid: string;
  title: string;
  description?: string | undefined;
  folderId?: string | undefined;
  tags: string[];
  editable: boolean;
  defaultDashboard: boolean;
  timeSettings: DashboardTimeSettings;
  refreshSettings: DashboardRefreshSettings;
  interactionSettings?: DashboardInteractionSettings | undefined;
  variables: DashboardVariable[];
  annotations: DashboardAnnotation[];
  links: DashboardLink[];
  layout: DashboardLayout;
  elements: DashboardElement[];
  version: number;
  createdAt: string;
  updatedAt: string;
  createdBy: string;
  updatedBy: string;
}

export interface DataField<T = unknown> {
  id: string;
  name: string;
  type: FieldType;
  values: T[];
  labels?: Record<string, string> | undefined;
  config?: FieldConfig | undefined;
  meta?: Record<string, unknown> | undefined;
}

export interface DataFrame {
  refId: string;
  name?: string | undefined;
  length: number;
  fields: DataField[];
  meta?: {
    sourceType?: string | undefined;
    preferredVisualization?: VisualizationType | undefined;
    queryDurationMs?: number | undefined;
    scannedRows?: number | undefined;
    [key: string]: unknown;
  } | undefined;
}

export interface PanelQueryError {
  refId?: string | undefined;
  message: string;
  cause?: unknown;
}

export interface DashboardTimeRange {
  from: number;
  to: number;
}

export interface PanelData {
  state: 'loading' | 'streaming' | 'done' | 'error';
  frames: DataFrame[];
  error?: PanelQueryError | undefined;
  timeRange: DashboardTimeRange;
}
