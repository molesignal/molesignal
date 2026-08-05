import type { Dashboard } from '@/types/dashboard';

import { validateDashboardModelContract } from './contracts';
import {
  DEFAULT_DASHBOARD_CURSOR_SYNC_MODE,
  DASHBOARD_ENGINE_KIND,
  DASHBOARD_SCHEMA_VERSION,
  type DashboardDefinition,
  type DashboardElement,
  type DashboardPanel,
  type DashboardVariable,
  type GridPosition,
  type PanelDataSourceType,
  type PanelQuery,
  type VisualizationType,
} from './schema';

export interface DashboardValidationResult {
  valid: boolean;
  issues: string[];
}

export function createEmptyDashboardDefinition(
  title = 'Untitled dashboard',
): DashboardDefinition {
  const now = new Date().toISOString();
  return {
    engine: DASHBOARD_ENGINE_KIND,
    schemaVersion: DASHBOARD_SCHEMA_VERSION,
    id: '',
    uid: createId('dashboard'),
    title,
    tags: [],
    editable: true,
    defaultDashboard: false,
    timeSettings: {
      defaultFrom: 'now-6h',
      defaultTo: 'now',
      timezone: 'browser',
    },
    refreshSettings: {
      enabled: true,
      mode: 'interval',
      defaultInterval: '30s',
      allowedIntervals: ['off', '5s', '10s', '30s', '1m', '5m'],
    },
    interactionSettings: {
      cursorSync: DEFAULT_DASHBOARD_CURSOR_SYNC_MODE,
    },
    variables: [],
    annotations: [],
    links: [],
    layout: {
      type: 'grid',
      columns: 24,
      rowHeight: 8,
      gap: 8,
    },
    elements: [],
    version: 1,
    createdAt: now,
    updatedAt: now,
    createdBy: '',
    updatedBy: '',
  };
}

export function dashboardDefinitionFromApi(
  dashboard: Dashboard,
): DashboardDefinition {
  const definition = parseStoredDashboardDefinition(
    dashboard.model,
    dashboard.title,
    dashboard.uid,
  );
  return {
    ...definition,
    id: dashboard.id,
    uid: dashboard.uid || definition.uid,
    title: dashboard.title || definition.title,
    folderId: dashboard.folder_id,
    tags: dashboard.tags.length > 0 ? dashboard.tags : definition.tags,
    version: dashboard.version,
    createdAt: timestampToIso(dashboard.created_at),
    updatedAt: timestampToIso(dashboard.updated_at),
    createdBy: dashboard.created_by ?? '',
    updatedBy: dashboard.updated_by ?? '',
  };
}

export function dashboardDefinitionFromStoredModel(
  value: unknown,
  fallbackTitle: string,
  fallbackUid: string,
): DashboardDefinition {
  return parseStoredDashboardDefinition(value, fallbackTitle, fallbackUid);
}

/**
 * API records can outlive a dashboard schema rollout. Only persisted API data
 * is upgraded here; JSON imports remain strict so unsupported documents are
 * never silently accepted as new dashboards.
 */
function parseStoredDashboardDefinition(
  value: unknown,
  fallbackTitle: string,
  fallbackUid: string,
): DashboardDefinition {
  const validation = validateDashboardDefinition(value);
  if (validation.valid) {
    return globalThis.structuredClone(value as DashboardDefinition);
  }
  if (!isRecord(value)) {
    throw new Error('stored dashboard model must be an object');
  }

  const definition = createEmptyDashboardDefinition(
    nonEmptyString(value.title) ?? fallbackTitle,
  );
  definition.uid =
    nonEmptyString(value.uid) || fallbackUid || definition.uid;
  definition.description = optionalString(value.description);
  definition.editable =
    typeof value.editable === 'boolean' ? value.editable : definition.editable;
  definition.defaultDashboard =
    typeof value.defaultDashboard === 'boolean'
      ? value.defaultDashboard
      : definition.defaultDashboard;
  definition.tags = stringArray(value.tags);

  const timeSettings = recordValue(value.timeSettings);
  const legacyTime = recordValue(value.time);
  definition.timeSettings = {
    ...definition.timeSettings,
    defaultFrom:
      nonEmptyString(timeSettings?.defaultFrom) ??
      nonEmptyString(legacyTime?.from) ??
      definition.timeSettings.defaultFrom,
    defaultTo:
      nonEmptyString(timeSettings?.defaultTo) ??
      nonEmptyString(legacyTime?.to) ??
      definition.timeSettings.defaultTo,
    timezone:
      nonEmptyString(timeSettings?.timezone) ??
      nonEmptyString(value.timezone) ??
      definition.timeSettings.timezone,
    ...(typeof timeSettings?.hideTimePicker === 'boolean'
      ? { hideTimePicker: timeSettings.hideTimePicker }
      : {}),
  };

  definition.refreshSettings = storedRefreshSettings(
    value.refreshSettings,
    value.refresh,
    definition.refreshSettings,
  );
  definition.interactionSettings = storedInteractionSettings(
    value.interactionSettings,
  );
  definition.layout = storedLayout(value.layout, definition.layout);

  const hasLegacyPanels = Array.isArray(value.panels);
  definition.variables = hasLegacyPanels
    ? migrateLegacyVariables(value.templating)
    : cloneArray<DashboardVariable>(value.variables);
  definition.annotations = hasLegacyPanels
    ? []
    : cloneArray(value.annotations);
  definition.links = hasLegacyPanels ? [] : cloneArray(value.links);
  definition.elements = Array.isArray(value.elements)
    ? globalThis.structuredClone(value.elements as DashboardElement[])
    : migrateLegacyPanels(value.panels, definition.layout.columns);

  const upgradedValidation = validateDashboardDefinition(definition);
  if (!upgradedValidation.valid) {
    throw new Error(
      `stored dashboard model cannot be upgraded: ${upgradedValidation.issues.join('; ')}`,
    );
  }
  return definition;
}

export function parseDashboardDefinitionJson(
  json: string,
): DashboardDefinition {
  return parseDashboardDefinition(JSON.parse(json));
}

export function parseDashboardDefinition(
  value: unknown,
): DashboardDefinition {
  const validation = validateDashboardDefinition(value);
  if (!validation.valid) {
    throw new Error(validation.issues.join('; '));
  }
  return globalThis.structuredClone(value as DashboardDefinition);
}

export function validateDashboardDefinition(
  value: unknown,
): DashboardValidationResult {
  const issues: string[] = [];
  if (!isRecord(value)) {
    return { valid: false, issues: ['dashboard must be an object'] };
  }
  if (value.engine !== DASHBOARD_ENGINE_KIND) {
    issues.push(`engine must be ${DASHBOARD_ENGINE_KIND}`);
  }
  if (value.schemaVersion !== DASHBOARD_SCHEMA_VERSION) {
    issues.push(`schemaVersion must be ${DASHBOARD_SCHEMA_VERSION}`);
  }
  if (typeof value.title !== 'string' || !value.title.trim()) {
    issues.push('title must not be empty');
  }
  if (typeof value.uid !== 'string' || !value.uid.trim()) {
    issues.push('uid must not be empty');
  }
  for (const key of ['tags', 'variables', 'annotations', 'links', 'elements']) {
    if (!Array.isArray(value[key])) issues.push(`${key} must be an array`);
  }
  if (!isRecord(value.timeSettings)) {
    issues.push('timeSettings must be an object');
  }
  if (!isRecord(value.refreshSettings)) {
    issues.push('refreshSettings must be an object');
  } else {
    if (typeof value.refreshSettings.enabled !== 'boolean') {
      issues.push('refreshSettings.enabled must be a boolean');
    }
    if (
      !['off', 'interval', 'live'].includes(
        String(value.refreshSettings.mode),
      )
    ) {
      issues.push('refreshSettings.mode must be off, interval or live');
    }
    if (!Array.isArray(value.refreshSettings.allowedIntervals)) {
      issues.push('refreshSettings.allowedIntervals must be an array');
    }
    if (
      value.refreshSettings.mode === 'interval' &&
      (typeof value.refreshSettings.defaultInterval !== 'string' ||
        !value.refreshSettings.defaultInterval.trim())
    ) {
      issues.push(
        'refreshSettings.defaultInterval is required for interval mode',
      );
    }
  }
  if (value.interactionSettings !== undefined) {
    if (!isRecord(value.interactionSettings)) {
      issues.push('interactionSettings must be an object');
    } else if (
      value.interactionSettings.cursorSync !== 'off' &&
      value.interactionSettings.cursorSync !== 'shared_crosshair'
    ) {
      issues.push(
        'interactionSettings.cursorSync must be off or shared_crosshair',
      );
    }
  }
  if (!isRecord(value.layout)) {
    issues.push('layout must be an object');
    return { valid: false, issues };
  }
  const columns = value.layout.columns;
  if (
    typeof columns !== 'number' ||
    !Number.isInteger(columns) ||
    columns < 1 ||
    columns > 48
  ) {
    issues.push('layout.columns must be between 1 and 48');
  }
  if (
    typeof value.layout.rowHeight !== 'number' ||
    value.layout.rowHeight < 2
  ) {
    issues.push('layout.rowHeight must be at least 2');
  }
  if (typeof value.layout.gap !== 'number' || value.layout.gap < 0) {
    issues.push('layout.gap must be zero or greater');
  }

  const ids = new Set<string>();
  if (Array.isArray(value.elements)) {
    validateElements(value.elements, Number(columns) || 24, ids, issues);
  }
  if (issues.length === 0) {
    const contract = validateDashboardModelContract(value);
    if (!contract.valid) {
      issues.push(
        ...contract.issues.map(
          (issue) => `${issue.path || '/'}: ${issue.message}`,
        ),
      );
    }
  }
  return { valid: issues.length === 0, issues };
}

export function dashboardDefinitionToModel(
  definition: DashboardDefinition,
): Record<string, unknown> {
  const validation = validateDashboardDefinition(definition);
  if (!validation.valid) throw new Error(validation.issues.join('; '));
  return {
    ...globalThis.structuredClone(definition),
    schemaVersion: DASHBOARD_SCHEMA_VERSION,
    updatedAt: new Date().toISOString(),
  };
}

export function serializeDashboardDefinition(
  definition: DashboardDefinition,
): string {
  return JSON.stringify(dashboardDefinitionToModel(definition), null, 2);
}

export function flattenElements(
  elements: readonly DashboardElement[],
): DashboardElement[] {
  const out: DashboardElement[] = [];
  for (const element of elements) {
    out.push(element);
    if (element.kind === 'group' || element.kind === 'row') {
      out.push(...flattenElements(element.elements));
    } else if (element.kind === 'tab') {
      for (const tab of element.tabs) {
        out.push(...flattenElements(tab.elements));
      }
    }
  }
  return out;
}

export function walkElements(
  elements: readonly DashboardElement[],
  visit: (element: DashboardElement) => void,
): void {
  for (const element of elements) {
    visit(element);
    if (element.kind === 'group' || element.kind === 'row') {
      walkElements(element.elements, visit);
    } else if (element.kind === 'tab') {
      for (const tab of element.tabs) walkElements(tab.elements, visit);
    }
  }
}

function validateElements(
  elements: unknown[],
  columns: number,
  ids: Set<string>,
  issues: string[],
): void {
  for (const raw of elements) {
    if (!isRecord(raw)) {
      issues.push('dashboard element must be an object');
      continue;
    }
    const id = typeof raw.id === 'string' ? raw.id : '';
    if (!id.trim()) issues.push('element id must not be empty');
    if (ids.has(id)) issues.push(`duplicate element id: ${id}`);
    ids.add(id);
    if (!['panel', 'text', 'group', 'row', 'tab'].includes(String(raw.kind))) {
      issues.push(`invalid element kind for ${id || 'unknown element'}`);
    }
    if (!isRecord(raw.gridPos)) {
      issues.push(`gridPos is required for ${id || 'unknown element'}`);
      continue;
    }
    const { x, y, w, h } = raw.gridPos;
    if (
      ![x, y, w, h].every(
        (entry) => typeof entry === 'number' && Number.isFinite(entry),
      ) ||
      Number(x) < 0 ||
      Number(y) < 0 ||
      Number(w) < 1 ||
      Number(h) < 1
    ) {
      issues.push(`invalid grid position for ${id || 'unknown element'}`);
    } else if (Number(x) + Number(w) > columns) {
      issues.push(`element ${id} exceeds the configured grid`);
    }
    if (raw.kind === 'panel') {
      for (const key of [
        'queries',
        'transformations',
        'overrides',
        'links',
      ]) {
        if (!Array.isArray(raw[key])) {
          issues.push(`${key} must be an array for panel ${id}`);
        }
      }
      if (!isRecord(raw.visualization)) {
        issues.push(`visualization is required for panel ${id}`);
      }
    } else if (raw.kind === 'group' || raw.kind === 'row') {
      if (!Array.isArray(raw.elements)) {
        issues.push(`elements must be an array for ${id}`);
      } else {
        validateElements(raw.elements, columns, ids, issues);
      }
    } else if (raw.kind === 'tab') {
      if (!Array.isArray(raw.tabs)) {
        issues.push(`tabs must be an array for ${id}`);
      } else {
        for (const tab of raw.tabs) {
          if (!isRecord(tab) || !Array.isArray(tab.elements)) {
            issues.push(`tab entries must contain elements for ${id}`);
          } else {
            validateElements(tab.elements, columns, ids, issues);
          }
        }
      }
    }
  }
}

function storedRefreshSettings(
  currentValue: unknown,
  legacyValue: unknown,
  fallback: DashboardDefinition['refreshSettings'],
): DashboardDefinition['refreshSettings'] {
  const current = recordValue(currentValue);
  const configuredMode = nonEmptyString(current?.mode);
  const mode =
    configuredMode === 'off' ||
    configuredMode === 'interval' ||
    configuredMode === 'live'
      ? configuredMode
      : nonEmptyString(legacyValue) &&
          nonEmptyString(legacyValue)?.toLowerCase() !== 'off'
        ? 'interval'
        : fallback.mode;
  const defaultInterval =
    nonEmptyString(current?.defaultInterval) ??
    nonEmptyString(legacyValue) ??
    fallback.defaultInterval;
  return {
    enabled:
      typeof current?.enabled === 'boolean'
        ? current.enabled
        : mode !== 'off',
    mode,
    ...(mode === 'interval'
      ? { defaultInterval: defaultInterval || '30s' }
      : {}),
    allowedIntervals:
      stringArray(current?.allowedIntervals).length > 0
        ? stringArray(current?.allowedIntervals)
        : [...fallback.allowedIntervals],
  };
}

function storedInteractionSettings(
  value: unknown,
): NonNullable<DashboardDefinition['interactionSettings']> {
  const cursorSync = recordValue(value)?.cursorSync;
  return {
    cursorSync:
      cursorSync === 'off' || cursorSync === 'shared_crosshair'
        ? cursorSync
        : DEFAULT_DASHBOARD_CURSOR_SYNC_MODE,
  };
}

function storedLayout(
  value: unknown,
  fallback: DashboardDefinition['layout'],
): DashboardDefinition['layout'] {
  const layout = recordValue(value);
  return {
    type: 'grid',
    columns: integerBetween(layout?.columns, 1, 48) ?? fallback.columns,
    rowHeight: finiteAtLeast(layout?.rowHeight, 2) ?? fallback.rowHeight,
    gap: finiteAtLeast(layout?.gap, 0) ?? fallback.gap,
  };
}

function migrateLegacyPanels(
  value: unknown,
  columns: number,
): DashboardElement[] {
  if (!Array.isArray(value)) return [];
  return value.flatMap((raw, index) => {
    const panel = recordValue(raw);
    if (!panel) return [];
    return [migrateLegacyPanel(panel, index, columns)];
  });
}

function migrateLegacyPanel(
  panel: Record<string, unknown>,
  index: number,
  columns: number,
): DashboardElement {
  const type = nonEmptyString(panel.type)?.toLowerCase() ?? 'timeseries';
  const id = `legacy-${safeIdPart(panel.id, index + 1)}-${index + 1}`;
  const title = nonEmptyString(panel.title) ?? `Panel ${index + 1}`;
  const gridPos = migrateLegacyGridPosition(panel.gridPos, index, columns);

  if (type === 'text') {
    const options = recordValue(panel.options);
    return {
      kind: 'text',
      id,
      title,
      gridPos,
      content:
        optionalString(options?.content) ??
        optionalString(panel.content) ??
        '',
      mode: options?.mode === 'plain' ? 'plain' : 'markdown',
      transparent:
        typeof panel.transparent === 'boolean'
          ? panel.transparent
          : undefined,
    };
  }

  if (type === 'row') {
    return {
      kind: 'row',
      id,
      title,
      gridPos,
      collapsed: Boolean(panel.collapsed),
      elements: migrateLegacyPanels(panel.panels, columns),
    };
  }

  const fieldConfig = recordValue(panel.fieldConfig);
  const fieldDefaults = recordValue(fieldConfig?.defaults);
  const migrated: DashboardPanel = {
    kind: 'panel',
    id,
    title,
    gridPos,
    queryOptions: {},
    queries: migrateLegacyQueries(panel.targets, panel),
    transformations: [],
    visualization: {
      type: migrateLegacyVisualization(type),
      schemaVersion: 1,
      options: globalThis.structuredClone(recordValue(panel.options) ?? {}),
    },
    fieldConfig: migrateLegacyFieldConfig(fieldDefaults),
    overrides: [],
    links: [],
  };
  if (typeof panel.description === 'string') {
    migrated.description = panel.description;
  }
  if (typeof panel.transparent === 'boolean') {
    migrated.transparent = panel.transparent;
  }
  return migrated;
}

function migrateLegacyQueries(
  value: unknown,
  panel: Record<string, unknown>,
): PanelQuery[] {
  if (!Array.isArray(value)) return [];
  return value.flatMap((raw, index) => {
    const target = recordValue(raw);
    if (!target) return [];
    const statement =
      nonEmptyString(target.expr) ??
      nonEmptyString(target.rawSql) ??
      nonEmptyString(target.query);
    if (!statement) return [];
    const language = inferLegacyLanguage(target, statement);
    const dataSourceType = inferLegacyDataSourceType(
      target,
      panel,
      language,
      statement,
    );
    const query: Record<string, unknown> = {
      language,
      ...(language === 'promql'
        ? { expression: statement }
        : { statement }),
    };
    const streamName =
      nonEmptyString(target.stream_name) ??
      nonEmptyString(recordValue(target.stream)?.name);
    const streamType =
      nonEmptyString(target.stream_type) ??
      nonEmptyString(target.streamType) ??
      nonEmptyString(recordValue(target.stream)?.stream_type);
    if (streamName) query.streamName = streamName;
    if (streamType) query.streamType = streamType;

    return [
      {
        refId: nonEmptyString(target.refId) ?? refIdForIndex(index),
        enabled: target.hide !== true,
        dataSourceType,
        query,
        ...(nonEmptyString(target.legendFormat)
          ? { legend: nonEmptyString(target.legendFormat) }
          : {}),
        ...(nonEmptyString(target.format)
          ? { format: nonEmptyString(target.format) }
          : {}),
      },
    ];
  });
}

function inferLegacyLanguage(
  target: Record<string, unknown>,
  statement: string,
): 'promql' | 'sql' {
  const dataSource = recordValue(target.datasource);
  const configured = (
    nonEmptyString(target.language) ??
    nonEmptyString(dataSource?.type) ??
    ''
  ).toLowerCase();
  if (configured.includes('prom') || configured === 'metrics') return 'promql';
  if (
    configured.includes('sql') ||
    configured.includes('postgres') ||
    configured.includes('mysql') ||
    configured.includes('datafusion')
  ) {
    return 'sql';
  }
  return /\bselect\b/i.test(statement) ? 'sql' : 'promql';
}

function inferLegacyDataSourceType(
  target: Record<string, unknown>,
  panel: Record<string, unknown>,
  language: 'promql' | 'sql',
  statement: string,
): PanelDataSourceType {
  const dataSource = recordValue(target.datasource);
  const configured = [
    nonEmptyString(dataSource?.type),
    nonEmptyString(target.stream_type),
    nonEmptyString(target.streamType),
  ]
    .filter(Boolean)
    .join(' ')
    .toLowerCase();
  if (configured.includes('trace') || configured.includes('tempo')) {
    return 'traces';
  }
  if (configured.includes('profile')) return 'profiles';
  if (
    configured.includes('log') ||
    configured.includes('loki') ||
    configured.includes('elasticsearch')
  ) {
    return 'logs';
  }
  if (configured.includes('metric') || configured.includes('prom')) {
    return 'metrics';
  }
  if (language === 'promql') return 'metrics';
  const hint = `${nonEmptyString(panel.title) ?? ''} ${statement}`.toLowerCase();
  if (hint.includes('trace')) return 'traces';
  if (hint.includes('profile')) return 'profiles';
  if (hint.includes('log')) return 'logs';
  return 'sql';
}

function migrateLegacyVisualization(value: string): VisualizationType {
  switch (value.replaceAll('-', '_')) {
    case 'timeseries':
    case 'graph':
    case 'line':
      return 'time_series';
    case 'barchart':
    case 'bar':
      return 'bar_chart';
    case 'bargauge':
    case 'bar_gauge':
      return 'bar_gauge';
    case 'state_timeline':
    case 'table':
    case 'logs':
    case 'stat':
    case 'gauge':
    case 'heatmap':
      return value.replaceAll('-', '_') as VisualizationType;
    default:
      return 'time_series';
  }
}

function migrateLegacyFieldConfig(
  defaults: Record<string, unknown> | undefined,
): DashboardPanel['fieldConfig'] {
  if (!defaults) return {};
  const output: DashboardPanel['fieldConfig'] = {};
  if (typeof defaults.displayName === 'string') {
    output.displayName = defaults.displayName;
  }
  if (typeof defaults.unit === 'string') output.unit = defaults.unit;
  for (const key of ['decimals', 'min', 'max', 'softMin', 'softMax'] as const) {
    if (typeof defaults[key] === 'number' && Number.isFinite(defaults[key])) {
      output[key] = defaults[key];
    }
  }
  if (isRecord(defaults.custom)) {
    output.custom = globalThis.structuredClone(defaults.custom);
  }
  return output;
}

function migrateLegacyGridPosition(
  value: unknown,
  index: number,
  columns: number,
): GridPosition {
  const position = recordValue(value);
  const width = integerBetween(position?.w, 1, columns) ?? 12;
  const x = integerBetween(position?.x, 0, columns - 1) ?? 0;
  return {
    x,
    y: integerAtLeast(position?.y, 0) ?? index * 12,
    w: Math.min(width, columns - x),
    h: integerAtLeast(position?.h, 1) ?? 12,
    minW: 2,
    minH: 4,
  };
}

function migrateLegacyVariables(value: unknown): DashboardVariable[] {
  const templating = recordValue(value);
  const list = templating?.list;
  if (!Array.isArray(list)) return [];
  return list.flatMap((raw, index) => {
    const variable = recordValue(raw);
    const name = nonEmptyString(variable?.name);
    if (!variable || !name) return [];
    const rawType = nonEmptyString(variable.type)?.toLowerCase();
    const type: DashboardVariable['type'] =
      rawType === 'custom' ||
      rawType === 'constant' ||
      rawType === 'text' ||
      rawType === 'interval' ||
      rawType === 'data_source'
        ? rawType
        : 'query';
    const queryText =
      nonEmptyString(variable.query) ??
      nonEmptyString(recordValue(variable.query)?.query) ??
      '';
    const current = recordValue(variable.current);
    const currentValue = current?.value ?? current?.text;
    return [
      {
        id: `legacy-variable-${safeIdPart(name, index + 1)}`,
        name,
        label: nonEmptyString(variable.label) ?? name,
        type,
        ...(queryText ? { query: { expression: queryText } } : {}),
        currentValue,
        defaultValue: currentValue,
        multi: Boolean(variable.multi),
        includeAll: Boolean(variable.includeAll),
        hide:
          variable.hide === 'label' || variable.hide === 'variable'
            ? variable.hide
            : 'none',
        refresh:
          variable.refresh === 'time_range_change'
            ? 'time_range_change'
            : variable.refresh === 'dashboard_load' || variable.refresh === 1
              ? 'dashboard_load'
              : 'never',
      },
    ];
  });
}

function cloneArray<T>(value: unknown): T[] {
  return Array.isArray(value)
    ? globalThis.structuredClone(value as T[])
    : [];
}

function recordValue(
  value: unknown,
): Record<string, unknown> | undefined {
  return isRecord(value) ? value : undefined;
}

function optionalString(value: unknown): string | undefined {
  return typeof value === 'string' ? value : undefined;
}

function nonEmptyString(value: unknown): string | undefined {
  return typeof value === 'string' && value.trim() ? value.trim() : undefined;
}

function stringArray(value: unknown): string[] {
  return Array.isArray(value)
    ? value.filter((item): item is string => typeof item === 'string')
    : [];
}

function integerBetween(
  value: unknown,
  minimum: number,
  maximum: number,
): number | undefined {
  return typeof value === 'number' &&
    Number.isInteger(value) &&
    value >= minimum &&
    value <= maximum
    ? value
    : undefined;
}

function integerAtLeast(
  value: unknown,
  minimum: number,
): number | undefined {
  return typeof value === 'number' &&
    Number.isInteger(value) &&
    value >= minimum
    ? value
    : undefined;
}

function finiteAtLeast(
  value: unknown,
  minimum: number,
): number | undefined {
  return typeof value === 'number' &&
    Number.isFinite(value) &&
    value >= minimum
    ? value
    : undefined;
}

function safeIdPart(value: unknown, fallback: number): string {
  const raw = String(value ?? fallback)
    .trim()
    .replaceAll(/[^a-zA-Z0-9_-]/g, '-');
  return raw || String(fallback);
}

function refIdForIndex(index: number): string {
  return index < 26 ? String.fromCharCode(65 + index) : `Q${index + 1}`;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

function timestampToIso(value: number): string {
  if (!Number.isFinite(value) || value <= 0) return new Date(0).toISOString();
  const milliseconds = value > 10_000_000_000_000 ? value / 1000 : value;
  return new Date(milliseconds).toISOString();
}

function createId(prefix: string): string {
  const random =
    typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function'
      ? crypto.randomUUID()
      : `${Date.now()}-${Math.random().toString(16).slice(2)}`;
  return `${prefix}-${random}`;
}
