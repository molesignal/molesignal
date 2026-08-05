import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import {
  ArrowLeft,
  Plus,
  Redo2,
  Save,
  Settings2,
  Trash2,
  Undo2,
  X,
} from 'lucide-react';
import { nanoid } from 'nanoid';
import * as React from 'react';
import {
  useLocation,
  useNavigate,
  useParams,
  useSearchParams,
} from 'react-router-dom';

import * as dashboardsApi from '@/api/dashboards';
import { toApiError } from '@/lib/http';
import { ChromeButton, TimeRangeChip } from '@/shell/chrome';
import { CodeEditor } from '@/shell/codeEditor';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/shell/ui/dialog';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';
import {
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
} from '@/shell/ui/tabs';
import { useAuthStore } from '@/stores/auth';

import { DashboardRenderer } from './DashboardRenderer';
import { AnnotationEventsEditor } from './editor/configuration/AnnotationEventsEditor';
import {
  EditorField,
  EditorInput,
  EditorNumber,
  EditorSectionTitle,
  EditorSelect,
  EditorTextarea,
  OptionalNumberInput,
  ToggleField,
} from './editor/configuration/controls';
import { DashboardInteractionSettingsEditor } from './editor/configuration/DashboardInteractionSettingsEditor';
import { OverridePropertiesEditor } from './editor/configuration/OverridePropertiesEditor';
import { StringMapEditor } from './editor/configuration/StringMapEditor';
import { ThresholdsEditor } from './editor/configuration/ThresholdsEditor';
import { TransformationOptionsEditor } from './editor/configuration/TransformationOptionsEditor';
import { ValueMappingsEditor } from './editor/configuration/ValueMappingsEditor';
import { DashboardEditCanvas } from './editor/DashboardEditCanvas';
import { VariableQueryEditor } from './editor/variables/VariableQueryEditor';
import {
  createDashboardGroup,
  createDashboardPanel,
  createDashboardRow,
  createDashboardTab,
  createDashboardText,
  duplicateDashboardElement,
} from './factories';
import { isDashboardEngineEnabled } from './featureFlag';
import { useDashboardText } from './i18n';
import {
  clampGridPosition,
  findElement,
  gridPositionsCollide,
  removeElementFromTree,
  updateElementInTree,
} from './layout';
import {
  createEmptyDashboardDefinition,
  dashboardDefinitionFromApi,
  dashboardDefinitionToModel,
  serializeDashboardDefinition,
  validateDashboardDefinition,
} from './model';
import { QueryLegendControl } from './query/editor/QueryLegendControl';
import { QUERY_LEGEND_AUTO } from './query/legend';
import type {
  DashboardAnnotation,
  DashboardDefinition,
  DashboardElement,
  DashboardGroup,
  DashboardLink,
  DashboardPanel,
  DashboardRow,
  DashboardTab,
  DashboardVariable,
  DataLink,
  FieldOverride,
  PanelDataSourceType,
  PanelQuery,
  TransformationConfig,
  TransformationType,
  VisualizationType,
} from './schema';
import { visualizationRegistry } from './visualizations';
import {
  resolveVisualizationOptions,
  transitionVisualizationOptions,
} from './visualizations/options';

interface HistoryState {
  past: DashboardDefinition[];
  present: DashboardDefinition;
  future: DashboardDefinition[];
}

export function DashboardEditor() {
  const tr = useDashboardText();
  const { id } = useParams<{ id: string }>();
  const location = useLocation();
  const nav = useNavigate();
  const qc = useQueryClient();
  const [searchParams, setSearchParams] = useSearchParams();
  const orgId = useAuthStore((state) => state.ctx?.org_id ?? '');
  const existingId = id && id !== 'new' ? id : undefined;
  const isNewPanelRoute = location.pathname.endsWith('/panels/new');
  const [history, setHistory] = React.useState<HistoryState>(() => ({
    past: [],
    present: createEmptyDashboardDefinition(tr('Untitled dashboard')),
    future: [],
  }));
  const [loadedKey, setLoadedKey] = React.useState('');
  const [selectedIds, setSelectedIds] = React.useState<Set<string>>(
    () => new Set(),
  );
  const [clipboard, setClipboard] = React.useState<DashboardElement[]>([]);
  const [settingsOpen, setSettingsOpen] = React.useState(false);
  const selectedPanelId = searchParams.get('panel') ?? '';

  const dashboardQuery = useQuery({
    queryKey: ['dashboards', 'get', existingId],
    queryFn: () => dashboardsApi.get(existingId!),
    enabled: Boolean(existingId),
  });

  React.useEffect(() => {
    if (existingId && !dashboardQuery.data) return;
    const key = existingId
      ? `${existingId}:${dashboardQuery.data?.version ?? 0}`
      : 'new';
    if (key === loadedKey) return;
    let definition = dashboardQuery.data
      ? dashboardDefinitionFromApi(dashboardQuery.data)
      : createEmptyDashboardDefinition(tr('Untitled dashboard'));
    let panelToSelect = searchParams.get('panel') ?? '';
    if (isNewPanelRoute) {
      const panel = localizeNewElement(
        createDashboardPanel(definition.elements),
        tr,
      ) as DashboardPanel;
      definition = {
        ...definition,
        elements: [...definition.elements, panel],
      };
      panelToSelect = panel.id;
      setSearchParams({ panel: panel.id }, { replace: true });
    }
    setHistory({ past: [], present: definition, future: [] });
    setSelectedIds(panelToSelect ? new Set([panelToSelect]) : new Set());
    setLoadedKey(key);
  }, [
    dashboardQuery.data,
    existingId,
    isNewPanelRoute,
    loadedKey,
    searchParams,
    setSearchParams,
    tr,
  ]);

  const definition = history.present;
  const selectedPanel = React.useMemo(() => {
    const element = findElement(definition.elements, selectedPanelId);
    return element?.kind === 'panel' ? element : undefined;
  }, [definition.elements, selectedPanelId]);
  const inspectedElement = React.useMemo(() => {
    const elementId = [...selectedIds][0];
    if (!elementId) return undefined;
    const element = findElement(definition.elements, elementId);
    return element?.kind === 'panel' ? undefined : element;
  }, [definition.elements, selectedIds]);

  const commit = React.useCallback(
    (
      next:
        | DashboardDefinition
        | ((current: DashboardDefinition) => DashboardDefinition),
    ) => {
      setHistory((current) => {
        const present =
          typeof next === 'function' ? next(current.present) : next;
        if (present === current.present) return current;
        return {
          past: [...current.past.slice(-99), current.present],
          present: { ...present, updatedAt: new Date().toISOString() },
          future: [],
        };
      });
    },
    [],
  );
  const undo = React.useCallback(() => {
    setHistory((current) => {
      const present = current.past.at(-1);
      if (!present) return current;
      return {
        past: current.past.slice(0, -1),
        present,
        future: [current.present, ...current.future],
      };
    });
  }, []);
  const redo = React.useCallback(() => {
    setHistory((current) => {
      const present = current.future[0];
      if (!present) return current;
      return {
        past: [...current.past, current.present],
        present,
        future: current.future.slice(1),
      };
    });
  }, []);

  const saveMutation = useMutation({
    mutationFn: async () => {
      const validation = validateDashboardDefinition(definition);
      if (!validation.valid) throw new Error(validation.issues.join('\n'));
      const model = dashboardDefinitionToModel(definition);
      return existingId
        ? dashboardsApi.update(
            existingId,
            model,
            definition.folderId,
          )
        : dashboardsApi.create(model, definition.folderId);
    },
    onSuccess: async (saved) => {
      await qc.invalidateQueries({ queryKey: ['dashboards'] });
      toast.success(tr('Dashboard saved'));
      nav(`/dashboards/${saved.id}`);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const openPanel = React.useCallback(
    (panelId: string) => {
      setSearchParams({ panel: panelId });
      setSelectedIds(new Set([panelId]));
    },
    [setSearchParams],
  );

  const addElement = React.useCallback(
    (kind: DashboardElement['kind']) => {
      const factory = {
        panel: createDashboardPanel,
        text: createDashboardText,
        row: createDashboardRow,
        group: createDashboardGroup,
        tab: createDashboardTab,
      }[kind];
      const element = localizeNewElement(factory(definition.elements), tr);
      commit((current) => ({
        ...current,
        elements: [...current.elements, element],
      }));
      setSelectedIds(new Set([element.id]));
      if (element.kind === 'panel') openPanel(element.id);
    },
    [commit, definition.elements, openPanel, tr],
  );

  const duplicateElement = React.useCallback(
    (elementId: string) => {
      const source = findElement(definition.elements, elementId);
      if (!source) return;
      const copy = duplicateDashboardElement(source, 1, tr('Copy suffix'));
      commit((current) => ({
        ...current,
        elements: [
          ...current.elements,
          {
            ...copy,
            gridPos: clampGridPosition(
              copy.gridPos,
              current.layout.columns,
            ),
          },
        ],
      }));
      setSelectedIds(new Set([copy.id]));
    },
    [commit, definition.elements, tr],
  );

  const copySelection = React.useCallback(() => {
    const elements = [...selectedIds]
      .map((elementId) => findElement(definition.elements, elementId))
      .filter((element): element is DashboardElement => Boolean(element))
      .map((element) => globalThis.structuredClone(element));
    if (elements.length > 0) setClipboard(elements);
  }, [definition.elements, selectedIds]);

  const pasteClipboard = React.useCallback(() => {
    if (clipboard.length === 0) return;
    const copies = clipboard.map((element) =>
      duplicateDashboardElement(element, 1, tr('Copy suffix')),
    );
    commit((current) => {
      const elements = [...current.elements];
      for (const copy of copies) {
        let position = clampGridPosition(
          copy.gridPos,
          current.layout.columns,
        );
        while (
          elements.some((element) =>
            gridPositionsCollide(position, element.gridPos),
          )
        ) {
          position = { ...position, y: position.y + 1 };
        }
        elements.push({ ...copy, gridPos: position });
      }
      return { ...current, elements };
    });
    setSelectedIds(new Set(copies.map((element) => element.id)));
  }, [clipboard, commit, tr]);

  const removeElement = React.useCallback(
    (elementId: string) => {
      commit((current) => ({
        ...current,
        elements: removeElementFromTree(current.elements, elementId),
      }));
      setSelectedIds((current) => {
        const next = new Set(current);
        next.delete(elementId);
        return next;
      });
      if (selectedPanelId === elementId) {
        setSearchParams({});
      }
    },
    [commit, selectedPanelId, setSearchParams],
  );

  const updateElement = React.useCallback(
    (elementId: string, next: DashboardElement) => {
      commit((current) => ({
        ...current,
        elements: updateElementInTree(
          current.elements,
          elementId,
          () => next,
        ),
      }));
    },
    [commit],
  );

  React.useEffect(() => {
    if (selectedPanel) return;
    const handleShortcut = (event: KeyboardEvent) => {
      const target = event.target;
      if (
        target instanceof HTMLElement &&
        (target.isContentEditable ||
          ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName))
      ) {
        return;
      }
      const command = event.metaKey || event.ctrlKey;
      const key = event.key.toLowerCase();
      if (command && key === 'c' && selectedIds.size > 0) {
        event.preventDefault();
        copySelection();
      } else if (command && key === 'v' && clipboard.length > 0) {
        event.preventDefault();
        pasteClipboard();
      } else if (command && key === 'z') {
        event.preventDefault();
        if (event.shiftKey) redo();
        else undo();
      } else if (
        (event.key === 'Delete' || event.key === 'Backspace') &&
        selectedIds.size > 0
      ) {
        event.preventDefault();
        [...selectedIds].forEach(removeElement);
      }
    };
    window.addEventListener('keydown', handleShortcut);
    return () => window.removeEventListener('keydown', handleShortcut);
  }, [
    clipboard.length,
    copySelection,
    pasteClipboard,
    redo,
    removeElement,
    selectedIds,
    selectedPanel,
    undo,
  ]);

  if (!isDashboardEngineEnabled()) {
    return (
      <div className="grid min-h-[70vh] place-items-center p-8">
        <div className="max-w-lg rounded-md border border-bd-1 bg-bg-1 p-6 font-sans text-sm text-tx-2">
          {tr('Dashboard Engine is disabled by')}
          <code className="mx-1 font-mono text-xs text-tx-0">
            VITE_DASHBOARD_ENGINE
          </code>
          .
        </div>
      </div>
    );
  }

  if (dashboardQuery.isLoading) {
    return (
      <div className="grid min-h-[70vh] place-items-center font-sans text-sm text-tx-3">
        {tr('Loading dashboard…')}
      </div>
    );
  }
  if (dashboardQuery.isError) {
    return (
      <div className="grid min-h-[70vh] place-items-center font-sans text-sm text-danger">
        {toApiError(dashboardQuery.error).message}
      </div>
    );
  }

  return (
    <main className="dashboard-editor flex h-[calc(100vh-var(--app-topbar-h,0px))] min-h-[680px] flex-col overflow-hidden bg-bg-0">
      <header className="flex h-12 shrink-0 items-center gap-2 border-b border-bd-0 bg-bg-1 px-3">
        <ChromeButton
          aria-label={tr('Back to dashboard')}
          onClick={() =>
            nav(existingId ? `/dashboards/${existingId}` : '/dashboards')
          }
        >
          <ArrowLeft className="h-3.5 w-3.5" />
        </ChromeButton>
        <input
          value={definition.title}
          onChange={(event) =>
            commit((current) => ({ ...current, title: event.target.value }))
          }
          aria-label={tr('Dashboard title')}
          className="min-w-0 flex-1 border-0 bg-transparent px-2 font-sans text-sm font-semibold text-tx-0 outline-none"
        />
        {!definition.timeSettings.hideTimePicker && <TimeRangeChip />}
        <ChromeButton
          aria-label={tr('Undo')}
          disabled={history.past.length === 0}
          onClick={undo}
        >
          <Undo2 className="h-3.5 w-3.5" />
        </ChromeButton>
        <ChromeButton
          aria-label={tr('Redo')}
          disabled={history.future.length === 0}
          onClick={redo}
        >
          <Redo2 className="h-3.5 w-3.5" />
        </ChromeButton>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <ChromeButton>
              <Plus className="h-3.5 w-3.5" /> {tr('Add')}
            </ChromeButton>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuItem onSelect={() => addElement('panel')}>
              {tr('Panel')}
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={() => addElement('text')}>
              {tr('Text')}
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem onSelect={() => addElement('row')}>
              {tr('Row')}
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={() => addElement('group')}>
              {tr('Group')}
            </DropdownMenuItem>
            <DropdownMenuItem onSelect={() => addElement('tab')}>
              {tr('Tabs')}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
        <ChromeButton onClick={() => setSettingsOpen(true)}>
          <Settings2 className="h-3.5 w-3.5" /> {tr('Settings')}
        </ChromeButton>
        <ChromeButton
          variant="primary"
          disabled={saveMutation.isPending}
          onClick={() => saveMutation.mutate()}
        >
          <Save className="h-3.5 w-3.5" />
          {saveMutation.isPending ? tr('Saving…') : tr('Save')}
        </ChromeButton>
      </header>

      {selectedPanel ? (
        <PanelEditor
          dashboard={definition}
          panel={selectedPanel}
          orgId={orgId}
          onBack={() => setSearchParams({})}
          onChange={(panel) => updateElement(selectedPanel.id, panel)}
          onRemove={() => removeElement(selectedPanel.id)}
        />
      ) : (
        <div
          className={
            inspectedElement
              ? 'grid min-h-0 flex-1 grid-cols-1 lg:grid-cols-[minmax(0,1fr)_300px]'
              : 'grid min-h-0 flex-1 grid-cols-1'
          }
        >
          <DashboardEditCanvas
            definition={definition}
            orgId={orgId}
            selectedIds={selectedIds}
            clipboardSize={clipboard.length}
            onSelect={setSelectedIds}
            onOpenPanel={openPanel}
            onCommitElements={(elements) =>
              commit((current) => ({ ...current, elements }))
            }
            onCopy={copySelection}
            onPaste={pasteClipboard}
            onDuplicateElement={duplicateElement}
            onRemoveElement={removeElement}
            onExport={() => exportDashboardJson(definition)}
          />
          {inspectedElement && (
            <ElementInspector
              definition={definition}
              element={inspectedElement}
              onChange={updateElement}
              onOpenPanel={openPanel}
              onRemove={removeElement}
            />
          )}
        </div>
      )}

      <DashboardSettingsDialog
        open={settingsOpen}
        onOpenChange={setSettingsOpen}
        definition={definition}
        onChange={commit}
      />
    </main>
  );
}

function ElementInspector({
  definition,
  element,
  onChange,
  onOpenPanel,
  onRemove,
}: {
  definition: DashboardDefinition;
  element: DashboardElement;
  onChange: (id: string, element: DashboardElement) => void;
  onOpenPanel: (id: string) => void;
  onRemove: (id: string) => void;
}) {
  const tr = useDashboardText();
  return (
    <aside className="min-h-0 overflow-auto border-l border-bd-0 bg-bg-1">
      <div className="sticky top-0 z-10 border-b border-bd-0 bg-bg-1 px-4 py-3">
        <div className="font-sans text-xs font-semibold text-tx-1">
          {tr('Element')}
        </div>
        <div className="mt-0.5 font-mono text-type-micro uppercase tracking-wide text-tx-3">
          {tr(element.kind)}
        </div>
      </div>
      <div className="space-y-5 p-4">
          <EditorField label="Title">
            <EditorInput
              value={element.title}
              onChange={(value) =>
                onChange(element.id, { ...element, title: value })
              }
            />
          </EditorField>
          <EditorField label="Description">
            <EditorTextarea
              value={element.description ?? ''}
              rows={3}
              onChange={(value) =>
                onChange(element.id, {
                  ...element,
                  description: value || undefined,
                })
              }
            />
          </EditorField>
          <div>
            <EditorSectionTitle>Grid position</EditorSectionTitle>
            <div className="grid grid-cols-2 gap-2">
              {(['x', 'y', 'w', 'h'] as const).map((key) => (
                <EditorField key={key} label={key.toUpperCase()}>
                  <EditorNumber
                    value={element.gridPos[key]}
                    min={key === 'w' || key === 'h' ? 1 : 0}
                    onChange={(value) =>
                      onChange(element.id, {
                        ...element,
                        gridPos: clampGridPosition(
                          { ...element.gridPos, [key]: value },
                          definition.layout.columns,
                        ),
                      })
                    }
                  />
                </EditorField>
              ))}
            </div>
          </div>
          {element.kind === 'text' && (
            <>
              <EditorField label="Mode">
                <EditorSelect
                  value={element.mode}
                  options={[
                    ['markdown', 'Markdown'],
                    ['plain', 'Plain text'],
                  ]}
                  onChange={(value) =>
                    onChange(element.id, {
                      ...element,
                      mode: value as 'markdown' | 'plain',
                    })
                  }
                />
              </EditorField>
              <EditorField label="Content">
                <EditorTextarea
                  value={element.content}
                  rows={10}
                  mono
                  onChange={(value) =>
                    onChange(element.id, { ...element, content: value })
                  }
                />
              </EditorField>
            </>
          )}
          {(element.kind === 'row' || element.kind === 'group') && (
            <ContainerInspector
              element={element}
              onChange={(next) => onChange(element.id, next)}
              onOpenPanel={onOpenPanel}
            />
          )}
          {element.kind === 'tab' && (
            <TabInspector
              element={element}
              onChange={(next) => onChange(element.id, next)}
              onOpenPanel={onOpenPanel}
            />
          )}
          <div className="border-t border-bd-0 pt-4">
            <ChromeButton
              className="w-full justify-center text-danger"
              onClick={() => onRemove(element.id)}
            >
              <Trash2 className="h-3.5 w-3.5" /> {tr('Remove element')}
            </ChromeButton>
          </div>
      </div>
    </aside>
  );
}

function ContainerInspector({
  element,
  onChange,
  onOpenPanel,
}: {
  element: DashboardRow | DashboardGroup;
  onChange: (element: DashboardRow | DashboardGroup) => void;
  onOpenPanel: (id: string) => void;
}) {
  const tr = useDashboardText();
  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between">
        <span className="font-sans text-xs text-tx-2">
          {tr('Collapsed initially')}
        </span>
        <Switch
          checked={element.collapsed ?? false}
          onCheckedChange={(checked) => onChange({ ...element, collapsed: checked })}
        />
      </div>
      {element.kind === 'group' && (
        <EditorField label="Repeat variable">
          <EditorInput
            value={element.repeat?.variable ?? ''}
            placeholder="service"
            onChange={(value) =>
              onChange({
                ...element,
                repeat: value
                  ? {
                      variable: value,
                      direction: element.repeat?.direction ?? 'horizontal',
                    }
                  : undefined,
              })
            }
          />
        </EditorField>
      )}
      <EditorSectionTitle>
        {tr('Children')} · {element.elements.length}
      </EditorSectionTitle>
      <div className="space-y-1">
        {element.elements.map((child) => (
          <div
            key={child.id}
            className="flex items-center gap-2 rounded-md border border-bd-0 px-2 py-1.5"
          >
            <span className="min-w-0 flex-1 truncate font-sans text-xs text-tx-1">
              {child.title}
            </span>
            {child.kind === 'panel' && (
              <button
                type="button"
                onClick={() => onOpenPanel(child.id)}
                className="text-type-micro text-accent hover:underline"
              >
                {tr('Edit')}
              </button>
            )}
            <button
              type="button"
              aria-label={`${tr('Remove')} ${child.title}`}
              onClick={() =>
                onChange({
                  ...element,
                  elements: element.elements.filter(
                    (candidate) => candidate.id !== child.id,
                  ),
                })
              }
              className="text-tx-3 hover:text-danger"
            >
              <X className="h-3.5 w-3.5" />
            </button>
          </div>
        ))}
      </div>
      <ChromeButton
        className="w-full justify-center"
        onClick={() => {
          const panel = localizeNewElement(
            createDashboardPanel(element.elements),
            tr,
          ) as DashboardPanel;
          onChange({ ...element, elements: [...element.elements, panel] });
          onOpenPanel(panel.id);
        }}
      >
        <Plus className="h-3.5 w-3.5" /> {tr('Add child panel')}
      </ChromeButton>
    </div>
  );
}

function TabInspector({
  element,
  onChange,
  onOpenPanel,
}: {
  element: DashboardTab;
  onChange: (element: DashboardTab) => void;
  onOpenPanel: (id: string) => void;
}) {
  const tr = useDashboardText();
  return (
    <div className="space-y-3">
      <EditorSectionTitle>
        {tr('Tabs')} · {element.tabs.length}
      </EditorSectionTitle>
      {element.tabs.map((tab, tabIndex) => (
        <div
          key={tab.id}
          className="space-y-2 rounded-md border border-bd-0 bg-bg-0 p-2"
        >
          <div className="flex gap-1">
            <EditorInput
              value={tab.title}
              onChange={(title) =>
                onChange({
                  ...element,
                  tabs: element.tabs.map((candidate) =>
                    candidate.id === tab.id
                      ? { ...candidate, title }
                      : candidate,
                  ),
                })
              }
            />
            <button
              type="button"
              aria-label={`${tr('Remove')} ${tab.title}`}
              disabled={element.tabs.length === 1}
              onClick={() => {
                const tabs = element.tabs.filter(
                  (candidate) => candidate.id !== tab.id,
                );
                onChange({
                  ...element,
                  tabs,
                  defaultTabId:
                    element.defaultTabId === tab.id
                      ? tabs[0]?.id
                      : element.defaultTabId,
                });
              }}
              className="grid h-8 w-8 place-items-center rounded text-tx-3 hover:bg-bg-2 hover:text-danger disabled:opacity-40"
            >
              <Trash2 className="h-3.5 w-3.5" />
            </button>
          </div>
          <div className="font-mono text-type-micro text-tx-3">
            {tab.elements.length} {tr('elements')}
          </div>
          <ChromeButton
            className="w-full justify-center"
            onClick={() => {
              const panel = localizeNewElement(
                createDashboardPanel(tab.elements),
                tr,
              ) as DashboardPanel;
              onChange({
                ...element,
                tabs: element.tabs.map((candidate, index) =>
                  index === tabIndex
                    ? {
                        ...candidate,
                        elements: [...candidate.elements, panel],
                      }
                    : candidate,
                ),
              });
              onOpenPanel(panel.id);
            }}
          >
            <Plus className="h-3.5 w-3.5" /> {tr('Add panel')}
          </ChromeButton>
        </div>
      ))}
      <ChromeButton
        className="w-full justify-center"
        onClick={() => {
          const tab = {
            id: `tab-item-${nanoid(8)}`,
            title: `${tr('Tab')} ${element.tabs.length + 1}`,
            elements: [],
          };
          onChange({ ...element, tabs: [...element.tabs, tab] });
        }}
      >
        <Plus className="h-3.5 w-3.5" /> {tr('Add tab')}
      </ChromeButton>
    </div>
  );
}

function PanelEditor({
  dashboard,
  panel,
  orgId,
  onBack,
  onChange,
  onRemove,
}: {
  dashboard: DashboardDefinition;
  panel: DashboardPanel;
  orgId: string;
  onBack: () => void;
  onChange: (panel: DashboardPanel) => void;
  onRemove: () => void;
}) {
  const tr = useDashboardText();
  const previewDashboard = React.useMemo<DashboardDefinition>(
    () => ({
      ...dashboard,
      elements: [
        {
          ...panel,
          gridPos: {
            ...panel.gridPos,
            x: 0,
            y: 0,
            w: dashboard.layout.columns,
            h: 24,
          },
        },
      ],
    }),
    [dashboard, panel],
  );
  const plugin = visualizationRegistry.get(panel.visualization.type);
  const PluginEditor = plugin.editor;

  return (
    <div className="grid min-h-0 flex-1 grid-cols-1 grid-rows-[auto_minmax(260px,40vh)_minmax(320px,1fr)_minmax(320px,1fr)] lg:grid-cols-[minmax(0,1fr)_360px] lg:grid-rows-[auto_minmax(260px,42vh)_minmax(0,1fr)]">
      <header className="flex h-11 items-center gap-2 border-b border-bd-0 bg-bg-1 px-3 lg:col-span-2">
        <ChromeButton onClick={onBack}>
          <ArrowLeft className="h-3.5 w-3.5" /> {tr('Back to dashboard')}
        </ChromeButton>
        <div className="min-w-0">
          <div className="truncate font-sans text-xs font-semibold text-tx-1">
            {panel.title}
          </div>
          <div className="font-sans text-type-micro text-tx-3">
            {tr('Edit queries and visualization · resize on dashboard')}
          </div>
        </div>
        <ChromeButton className="ml-auto text-danger" onClick={onRemove}>
          <Trash2 className="h-3.5 w-3.5" /> {tr('Remove')}
        </ChromeButton>
      </header>

      <section className="min-h-0 overflow-auto border-b border-bd-0 bg-bg-0 p-3 lg:col-start-1 lg:row-start-2">
        <div className="mb-2 flex items-center justify-between">
          <span className="font-mono text-type-micro font-semibold uppercase tracking-wider text-tx-3">
            {tr('Live preview')}
          </span>
          <span className="rounded-sm bg-bg-2 px-2 py-1 font-mono text-type-micro text-tx-3">
            {panel.gridPos.w} × {panel.gridPos.h}
          </span>
        </div>
        <DashboardRenderer dashboard={previewDashboard} orgId={orgId} />
      </section>

        <Tabs
          defaultValue="queries"
          className="grid min-h-0 grid-rows-[auto_minmax(0,1fr)] border-b border-bd-0 bg-bg-0 lg:col-start-1 lg:row-start-3 lg:border-b-0"
        >
          <TabsList className="m-3 mb-0 h-8 w-fit">
            <TabsTrigger value="queries" className="h-6 text-xs">
              {tr('Queries')}
            </TabsTrigger>
            <TabsTrigger value="transformations" className="h-6 text-xs">
              {tr('Transformations')}
            </TabsTrigger>
            <TabsTrigger value="links" className="h-6 text-xs">
              {tr('Data links')}
            </TabsTrigger>
          </TabsList>
          <TabsContent value="queries" className="min-h-0 overflow-auto p-3">
            <QueryEditor
              queries={panel.queries}
              onChange={(queries) => onChange({ ...panel, queries })}
            />
          </TabsContent>
          <TabsContent
            value="transformations"
            className="min-h-0 overflow-auto p-3"
          >
            <TransformationEditor
              transformations={panel.transformations}
              onChange={(transformations) =>
                onChange({ ...panel, transformations })
              }
            />
          </TabsContent>
          <TabsContent value="links" className="min-h-0 overflow-auto p-3">
            <DataLinksEditor
              links={panel.links}
              onChange={(links) => onChange({ ...panel, links })}
            />
          </TabsContent>
        </Tabs>

        <Tabs
          defaultValue="panel"
          className="grid min-h-0 grid-rows-[auto_minmax(0,1fr)] border-l border-bd-0 bg-bg-1 lg:col-start-2 lg:row-span-2 lg:row-start-2"
        >
          <TabsList className="m-3 mb-0 h-8 w-fit">
            <TabsTrigger value="panel" className="h-6 text-xs">
              {tr('Panel')}
            </TabsTrigger>
            <TabsTrigger value="field" className="h-6 text-xs">
              {tr('Field')}
            </TabsTrigger>
            <TabsTrigger value="overrides" className="h-6 text-xs">
              {tr('Overrides')}
            </TabsTrigger>
          </TabsList>
          <TabsContent value="panel" className="min-h-0 overflow-auto p-4">
            <div className="space-y-4">
              <EditorField label="Title">
                <EditorInput
                  value={panel.title}
                  onChange={(title) => onChange({ ...panel, title })}
                />
              </EditorField>
              <EditorField label="Description">
                <EditorTextarea
                  value={panel.description ?? ''}
                  rows={3}
                  onChange={(description) =>
                    onChange({
                      ...panel,
                      description: description || undefined,
                    })
                  }
                />
              </EditorField>
              <div>
                <EditorSectionTitle>Panel size</EditorSectionTitle>
                <div className="grid grid-cols-2 gap-2">
                  <EditorField label="Width">
                    <EditorNumber
                      value={panel.gridPos.w}
                      min={1}
                      max={dashboard.layout.columns}
                      onChange={(w) =>
                        onChange({
                          ...panel,
                          gridPos: clampGridPosition(
                            { ...panel.gridPos, w },
                            dashboard.layout.columns,
                          ),
                        })
                      }
                    />
                  </EditorField>
                  <EditorField label="Height">
                    <EditorNumber
                      value={panel.gridPos.h}
                      min={1}
                      onChange={(h) =>
                        onChange({
                          ...panel,
                          gridPos: clampGridPosition(
                            { ...panel.gridPos, h },
                            dashboard.layout.columns,
                          ),
                        })
                      }
                    />
                  </EditorField>
                </div>
              </div>
              <EditorField label="Visualization">
                <EditorSelect
                  value={panel.visualization.type}
                  options={visualizationRegistry
                    .list()
                    .map((item) => [item.id, item.name])}
                  onChange={(value) => {
                    const type = value as VisualizationType;
                    const nextPlugin = visualizationRegistry.get(type);
                    onChange({
                      ...panel,
                      visualization: {
                        type,
                        schemaVersion: nextPlugin.optionSchemaVersion,
                        options: transitionVisualizationOptions(
                          nextPlugin.defaultOptions,
                          panel.visualization.options,
                        ),
                      },
                    });
                  }}
                />
              </EditorField>
              <div>
                <EditorSectionTitle>
                  {tr(plugin.name)} {tr('Options')}
                </EditorSectionTitle>
                <PluginEditor
                  options={resolveVisualizationOptions(
                    plugin.defaultOptions,
                    panel.visualization.options,
                  )}
                  onChange={(options) =>
                    onChange({
                      ...panel,
                      visualization: { ...panel.visualization, options },
                    })
                  }
                />
              </div>
              <EditorField label="Repeat variable">
                <EditorInput
                  value={panel.repeat?.variable ?? ''}
                  placeholder="service"
                  onChange={(variable) =>
                    onChange({
                      ...panel,
                      repeat: variable
                        ? {
                            variable,
                            direction:
                              panel.repeat?.direction ?? 'horizontal',
                            maxPerRow: panel.repeat?.maxPerRow,
                          }
                        : undefined,
                    })
                  }
                />
              </EditorField>
              {panel.repeat && (
                <div className="grid grid-cols-2 gap-2">
                  <EditorField label="Repeat direction">
                    <EditorSelect
                      value={panel.repeat.direction}
                      options={[
                        ['horizontal', 'Horizontal'],
                        ['vertical', 'Vertical'],
                        ['grid', 'Grid'],
                      ]}
                      onChange={(direction) =>
                        onChange({
                          ...panel,
                          repeat: {
                            ...panel.repeat!,
                            direction: direction as
                              | 'horizontal'
                              | 'vertical'
                              | 'grid',
                          },
                        })
                      }
                    />
                  </EditorField>
                  <EditorField label="Max per row">
                    <EditorNumber
                      value={panel.repeat.maxPerRow ?? 4}
                      min={1}
                      onChange={(maxPerRow) =>
                        onChange({
                          ...panel,
                          repeat: { ...panel.repeat!, maxPerRow },
                        })
                      }
                    />
                  </EditorField>
                </div>
              )}
              <div className="flex items-center justify-between">
                <span className="font-sans text-xs text-tx-2">
                  {tr('Transparent')}
                </span>
                <Switch
                  checked={panel.transparent ?? false}
                  onCheckedChange={(transparent) =>
                    onChange({ ...panel, transparent })
                  }
                />
              </div>
            </div>
          </TabsContent>
          <TabsContent value="field" className="min-h-0 overflow-auto p-4">
            <FieldConfigEditor
              panel={panel}
              onChange={onChange}
            />
          </TabsContent>
          <TabsContent value="overrides" className="min-h-0 overflow-auto p-4">
            <OverridesEditor
              overrides={panel.overrides}
              onChange={(overrides) => onChange({ ...panel, overrides })}
            />
          </TabsContent>
        </Tabs>
    </div>
  );
}

function QueryEditor({
  queries,
  onChange,
}: {
  queries: PanelQuery[];
  onChange: (queries: PanelQuery[]) => void;
}) {
  const tr = useDashboardText();
  const update = (index: number, query: PanelQuery) =>
    onChange(
      queries.map((candidate, candidateIndex) =>
        candidateIndex === index ? query : candidate,
      ),
    );
  return (
    <div className="space-y-3">
      {queries.map((query, index) => (
        <div
          key={`${query.refId}-${index}`}
          className="rounded-md border border-bd-0 bg-bg-1"
        >
          <div className="flex h-9 items-center gap-2 border-b border-bd-0 px-3">
            <span className="rounded-sm bg-bg-3 px-1.5 py-0.5 font-mono text-type-micro font-semibold text-tx-1">
              {query.refId}
            </span>
            <span className="font-sans text-xs text-tx-2">
              {tr(query.dataSourceType)}
            </span>
            <Switch
              className="ml-auto"
              checked={query.enabled}
              onCheckedChange={(enabled) =>
                update(index, { ...query, enabled })
              }
            />
            <button
              type="button"
              aria-label={`${tr('Remove query')} ${query.refId}`}
              onClick={() =>
                onChange(
                  queries.filter(
                    (_, candidateIndex) => candidateIndex !== index,
                  ),
                )
              }
              className="text-tx-3 hover:text-danger"
            >
              <Trash2 className="h-3.5 w-3.5" />
            </button>
          </div>
          <div className="grid gap-3 p-3">
            <div className="grid grid-cols-[72px_1fr_1fr] gap-2">
              <EditorField label="Ref ID">
                <EditorInput
                  value={query.refId}
                  mono
                  onChange={(refId) => update(index, { ...query, refId })}
                />
              </EditorField>
              <EditorField label="Data source">
                <EditorSelect
                  value={query.dataSourceType}
                  options={DATA_SOURCE_TYPES.map((type) => [type, type])}
                  onChange={(value) => {
                    const dataSourceType = value as PanelDataSourceType;
                    const language =
                      dataSourceType === 'metrics' ? 'promql' : 'sql';
                    update(index, {
                      ...query,
                      dataSourceType,
                      query: { ...query.query, language },
                    });
                  }}
                />
              </EditorField>
              <EditorField label="Language">
                <EditorSelect
                  value={queryEditorLanguage(query)}
                  options={[
                    ['promql', 'PromQL'],
                    ['sql', 'SQL'],
                  ]}
                  onChange={(language) =>
                    update(index, {
                      ...query,
                      query: { ...query.query, language },
                    })
                  }
                />
              </EditorField>
            </div>
            <EditorField label="Expression">
              <CodeEditor
                value={queryExpressionValue(query)}
                language={queryEditorLanguage(query)}
                ariaLabel={tr('Expression')}
                placeholder={
                  queryEditorLanguage(query) === 'promql'
                    ? tr('PromQL expression')
                    : tr('SQL expression')
                }
                minHeight={112}
                maxHeight={240}
                compact
                highlightCurrentLine={false}
                showHeader={false}
                showStatus={false}
                onChange={(expression) =>
                  update(index, {
                    ...query,
                    query: { ...query.query, expression },
                  })
                }
              />
            </EditorField>
            {query.dataSourceType !== 'metrics' &&
              query.dataSourceType !== 'profiles' && (
                <div className="grid grid-cols-2 gap-2">
                  <EditorField label="Stream name">
                    <EditorInput
                      value={stringValue(
                        query.query.streamName ?? query.query.stream,
                      )}
                      placeholder="stream"
                      mono
                      onChange={(streamName) =>
                        update(index, {
                          ...query,
                          query: { ...query.query, streamName },
                        })
                      }
                    />
                  </EditorField>
                  <EditorField label="Stream type">
                    <EditorSelect
                      value={
                        stringValue(query.query.streamType) ||
                        (query.dataSourceType === 'sql'
                          ? 'logs'
                          : query.dataSourceType)
                      }
                      options={[
                        ['logs', 'Logs'],
                        ['metrics', 'Metrics'],
                        ['traces', 'Traces'],
                      ]}
                      onChange={(streamType) =>
                        update(index, {
                          ...query,
                          query: { ...query.query, streamType },
                        })
                      }
                    />
                  </EditorField>
                </div>
              )}
            {query.dataSourceType === 'profiles' && (
              <div className="grid grid-cols-2 gap-2">
                <EditorField label="Service">
                  <EditorInput
                    value={stringValue(query.query.service)}
                    placeholder="$service"
                    onChange={(service) =>
                      update(index, {
                        ...query,
                        query: { ...query.query, service },
                      })
                    }
                  />
                </EditorField>
                <EditorField label="Profile type">
                  <EditorInput
                    value={stringValue(query.query.profileType)}
                    placeholder="cpu"
                    onChange={(profileType) =>
                      update(index, {
                        ...query,
                        query: { ...query.query, profileType },
                      })
                    }
                  />
                </EditorField>
              </div>
            )}
            <div className="grid grid-cols-2 gap-2">
              <EditorField label="Legend">
                {query.dataSourceType === 'metrics' ? (
                  <QueryLegendControl
                    value={query.legend}
                    onChange={(legend) =>
                      update(index, { ...query, legend })
                    }
                  />
                ) : (
                  <EditorInput
                    value={query.legend ?? ''}
                    placeholder="{{service}}"
                    onChange={(legend) =>
                      update(index, { ...query, legend: legend || undefined })
                    }
                  />
                )}
              </EditorField>
              <EditorField label="Shared query">
                <EditorInput
                  value={
                    query.sharedQuery
                      ? `${query.sharedQuery.sourcePanelId}:${query.sharedQuery.sourceRefId}`
                      : ''
                  }
                  placeholder="panel-id:A"
                  mono
                  onChange={(value) => {
                    const separator = value.lastIndexOf(':');
                    update(index, {
                      ...query,
                      sharedQuery:
                        separator > 0
                          ? {
                              sourcePanelId: value.slice(0, separator),
                              sourceRefId: value.slice(separator + 1),
                            }
                          : undefined,
                    });
                  }}
                />
              </EditorField>
            </div>
          </div>
        </div>
      ))}
      <ChromeButton
        className="w-full justify-center"
        onClick={() =>
          onChange([
            ...queries,
            {
              refId: nextRefId(queries),
              enabled: true,
              dataSourceType: 'metrics',
              legend: QUERY_LEGEND_AUTO,
              query: { language: 'promql', expression: '' },
            },
          ])
        }
      >
        <Plus className="h-3.5 w-3.5" /> {tr('Add query')}
      </ChromeButton>
    </div>
  );
}

function TransformationEditor({
  transformations,
  onChange,
}: {
  transformations: TransformationConfig[];
  onChange: (transformations: TransformationConfig[]) => void;
}) {
  const tr = useDashboardText();
  return (
    <div className="space-y-3">
      {transformations.map((transformation, index) => (
        <div
          key={transformation.id}
          className="rounded-md border border-bd-0 bg-bg-1 p-3"
        >
          <div className="mb-3 flex items-end gap-2">
            <EditorField label={`Transformation ${index + 1}`}>
              <EditorSelect
                value={transformation.type}
                options={TRANSFORMATION_TYPES.map((type) => [type, type])}
                onChange={(value) =>
                  onChange(
                    transformations.map((candidate) =>
                      candidate.id === transformation.id
                        ? {
                            ...candidate,
                            type: value as TransformationType,
                          }
                        : candidate,
                    ),
                  )
                }
              />
            </EditorField>
            <Switch
              checked={!transformation.disabled}
              onCheckedChange={(enabled) =>
                onChange(
                  transformations.map((candidate) =>
                    candidate.id === transformation.id
                      ? { ...candidate, disabled: !enabled }
                      : candidate,
                  ),
                )
              }
            />
            <button
              type="button"
              aria-label={`${tr('Remove transformation')} ${index + 1}`}
              onClick={() =>
                onChange(
                  transformations.filter(
                    (candidate) => candidate.id !== transformation.id,
                  ),
                )
              }
              className="grid h-8 w-8 place-items-center rounded text-tx-3 hover:bg-bg-2 hover:text-danger"
            >
              <Trash2 className="h-3.5 w-3.5" />
            </button>
          </div>
          <TransformationOptionsEditor
            type={transformation.type}
            value={transformation.options}
            onChange={(options) =>
              onChange(
                transformations.map((candidate) =>
                  candidate.id === transformation.id
                    ? { ...candidate, options }
                    : candidate,
                ),
              )
            }
          />
        </div>
      ))}
      <ChromeButton
        className="w-full justify-center"
        onClick={() =>
          onChange([
            ...transformations,
            {
              id: `transformation-${nanoid(8)}`,
              type: 'filter_fields',
              options: {},
            },
          ])
        }
      >
        <Plus className="h-3.5 w-3.5" /> {tr('Add transformation')}
      </ChromeButton>
    </div>
  );
}

function DataLinksEditor({
  links,
  onChange,
}: {
  links: DataLink[];
  onChange: (links: DataLink[]) => void;
}) {
  const tr = useDashboardText();
  return (
    <div className="space-y-3">
      {links.map((link, index) => (
        <div
          key={link.id}
          className="space-y-3 rounded-md border border-bd-0 bg-bg-1 p-3"
        >
          <div className="flex items-center gap-2">
            <span className="font-sans text-xs font-semibold text-tx-1">
              {tr('Link')} {index + 1}
            </span>
            <button
              type="button"
              aria-label={`${tr('Remove link')} ${index + 1}`}
              onClick={() =>
                onChange(links.filter((candidate) => candidate.id !== link.id))
              }
              className="ml-auto text-tx-3 hover:text-danger"
            >
              <Trash2 className="h-3.5 w-3.5" />
            </button>
          </div>
          <div className="grid grid-cols-2 gap-2">
            <EditorField label="Title">
              <EditorInput
                value={link.title}
                onChange={(title) =>
                  onChange(
                    links.map((candidate) =>
                      candidate.id === link.id
                        ? { ...candidate, title }
                        : candidate,
                    ),
                  )
                }
              />
            </EditorField>
            <EditorField label="Target">
              <EditorSelect
                value={link.target}
                options={DATA_LINK_TARGETS.map((target) => [target, target])}
                onChange={(target) =>
                  onChange(
                    links.map((candidate) =>
                      candidate.id === link.id
                        ? { ...candidate, target: target as DataLink['target'] }
                        : candidate,
                    ),
                  )
                }
              />
            </EditorField>
          </div>
          <EditorField label="URL">
            <EditorInput
              value={link.url ?? ''}
              placeholder="/logs?service=$service"
              mono
              onChange={(url) =>
                onChange(
                  links.map((candidate) =>
                    candidate.id === link.id
                      ? { ...candidate, url: url || undefined }
                      : candidate,
                  ),
                )
              }
            />
          </EditorField>
          <div>
            <EditorSectionTitle>Variables</EditorSectionTitle>
            <StringMapEditor
              value={link.variables}
              onChange={(variables) =>
                onChange(
                  links.map((candidate) =>
                    candidate.id === link.id
                      ? { ...candidate, variables }
                      : candidate,
                  ),
                )
              }
              keyLabel="Variable"
              valueLabel="Value"
              addLabel="Add variable"
            />
          </div>
          <div className="grid grid-cols-3 gap-3">
            <ToggleField
              label="Time range"
              checked={link.includeTimeRange}
              onChange={(includeTimeRange) =>
                onChange(
                  links.map((candidate) =>
                    candidate.id === link.id
                      ? { ...candidate, includeTimeRange }
                      : candidate,
                  ),
                )
              }
            />
            <ToggleField
              label="Variables"
              checked={link.includeDashboardVariables}
              onChange={(includeDashboardVariables) =>
                onChange(
                  links.map((candidate) =>
                    candidate.id === link.id
                      ? { ...candidate, includeDashboardVariables }
                      : candidate,
                  ),
                )
              }
            />
            <ToggleField
              label="New tab"
              checked={link.openInNewTab}
              onChange={(openInNewTab) =>
                onChange(
                  links.map((candidate) =>
                    candidate.id === link.id
                      ? { ...candidate, openInNewTab }
                      : candidate,
                  ),
                )
              }
            />
          </div>
        </div>
      ))}
      <ChromeButton
        className="w-full justify-center"
        onClick={() =>
          onChange([
            ...links,
            {
              id: `data-link-${nanoid(8)}`,
              title: tr('Open related data'),
              target: 'logs',
              variables: {},
              includeTimeRange: true,
              includeDashboardVariables: true,
              openInNewTab: false,
            },
          ])
        }
      >
        <Plus className="h-3.5 w-3.5" /> {tr('Add data link')}
      </ChromeButton>
    </div>
  );
}

function FieldConfigEditor({
  panel,
  onChange,
}: {
  panel: DashboardPanel;
  onChange: (panel: DashboardPanel) => void;
}) {
  const config = panel.fieldConfig;
  return (
    <div className="space-y-4">
      <EditorField label="Display name">
        <EditorInput
          value={config.displayName ?? ''}
          onChange={(displayName) =>
            onChange({
              ...panel,
              fieldConfig: {
                ...config,
                displayName: displayName || undefined,
              },
            })
          }
        />
      </EditorField>
      <div className="grid grid-cols-2 gap-2">
        <EditorField label="Unit">
          <EditorInput
            value={config.unit ?? ''}
            placeholder="short"
            onChange={(unit) =>
              onChange({
                ...panel,
                fieldConfig: { ...config, unit: unit || undefined },
              })
            }
          />
        </EditorField>
        <EditorField label="Decimals">
          <EditorNumber
            value={config.decimals ?? 2}
            min={0}
            onChange={(decimals) =>
              onChange({
                ...panel,
                fieldConfig: { ...config, decimals },
              })
            }
          />
        </EditorField>
        <EditorField label="Min">
          <OptionalNumberInput
            value={config.min}
            onChange={(min) =>
              onChange({
                ...panel,
                fieldConfig: { ...config, min },
              })
            }
          />
        </EditorField>
        <EditorField label="Max">
          <OptionalNumberInput
            value={config.max}
            onChange={(max) =>
              onChange({
                ...panel,
                fieldConfig: { ...config, max },
              })
            }
          />
        </EditorField>
      </div>
      <EditorField label="No-value text">
        <EditorInput
          value={config.noValue ?? ''}
          placeholder="—"
          onChange={(noValue) =>
            onChange({
              ...panel,
              fieldConfig: { ...config, noValue: noValue || undefined },
            })
          }
        />
      </EditorField>
      <EditorField label="Color">
        <div className="grid grid-cols-2 gap-2">
          <EditorSelect
            value={config.color?.mode ?? 'palette'}
            options={[
              ['palette', 'Palette'],
              ['fixed', 'Fixed'],
              ['thresholds', 'Thresholds'],
              ['continuous', 'Continuous'],
            ]}
            onChange={(mode) =>
              onChange({
                ...panel,
                fieldConfig: {
                  ...config,
                  color: {
                    ...config.color,
                    mode: mode as NonNullable<
                      DashboardPanel['fieldConfig']['color']
                    >['mode'],
                  },
                },
              })
            }
          />
          <EditorInput
            value={config.color?.value ?? ''}
            placeholder="var(--accent)"
            onChange={(value) =>
              onChange({
                ...panel,
                fieldConfig: {
                  ...config,
                  color: {
                    mode: config.color?.mode ?? 'fixed',
                    value: value || undefined,
                  },
                },
              })
            }
          />
        </div>
      </EditorField>
      <div>
        <EditorSectionTitle>Thresholds</EditorSectionTitle>
        <ThresholdsEditor
          value={config.thresholds ?? { mode: 'absolute', steps: [] }}
          onChange={(thresholds) =>
            onChange({
              ...panel,
              fieldConfig: {
                ...config,
                thresholds,
              },
            })
          }
        />
      </div>
      <div>
        <EditorSectionTitle>Value mappings</EditorSectionTitle>
        <ValueMappingsEditor
          value={config.mappings ?? []}
          onChange={(mappings) =>
            onChange({
              ...panel,
              fieldConfig: {
                ...config,
                mappings,
              },
            })
          }
        />
      </div>
    </div>
  );
}

function OverridesEditor({
  overrides,
  onChange,
}: {
  overrides: FieldOverride[];
  onChange: (overrides: FieldOverride[]) => void;
}) {
  const tr = useDashboardText();
  return (
    <div className="space-y-3">
      {overrides.map((override, index) => (
        <div
          key={override.id}
          className="space-y-3 rounded-md border border-bd-0 bg-bg-0 p-3"
        >
          <div className="flex items-center gap-2">
            <span className="font-sans text-xs font-semibold text-tx-1">
              {tr('Override')} {index + 1}
            </span>
            <button
              type="button"
              aria-label={`${tr('Remove override')} ${index + 1}`}
              onClick={() =>
                onChange(
                  overrides.filter(
                    (candidate) => candidate.id !== override.id,
                  ),
                )
              }
              className="ml-auto text-tx-3 hover:text-danger"
            >
              <Trash2 className="h-3.5 w-3.5" />
            </button>
          </div>
          <div className="grid grid-cols-2 gap-2">
            <EditorField label="Matcher">
              <EditorSelect
                value={override.matcher.type}
                options={[
                  ['field_name', 'Field name'],
                  ['field_regex', 'Field regex'],
                  ['field_type', 'Field type'],
                  ['query_ref', 'Query ref'],
                ]}
                onChange={(type) =>
                  onChange(
                    overrides.map((candidate) =>
                      candidate.id === override.id
                        ? {
                            ...candidate,
                            matcher: {
                              type,
                              value: candidate.matcher.value,
                            } as FieldOverride['matcher'],
                          }
                        : candidate,
                    ),
                  )
                }
              />
            </EditorField>
            <EditorField label="Value">
              <EditorInput
                value={override.matcher.value}
                mono
                onChange={(value) =>
                  onChange(
                    overrides.map((candidate) =>
                      candidate.id === override.id
                        ? {
                            ...candidate,
                            matcher: {
                              ...candidate.matcher,
                              value,
                            } as FieldOverride['matcher'],
                          }
                        : candidate,
                    ),
                  )
                }
              />
            </EditorField>
          </div>
          <div>
            <EditorSectionTitle>Properties</EditorSectionTitle>
            <OverridePropertiesEditor
              value={override.properties}
              onChange={(properties) =>
                onChange(
                  overrides.map((candidate) =>
                    candidate.id === override.id
                      ? {
                          ...candidate,
                          properties,
                        }
                      : candidate,
                  ),
                )
              }
            />
          </div>
        </div>
      ))}
      <ChromeButton
        className="w-full justify-center"
        onClick={() =>
          onChange([
            ...overrides,
            {
              id: `override-${nanoid(8)}`,
              matcher: { type: 'field_name', value: '' },
              properties: [],
            },
          ])
        }
      >
        <Plus className="h-3.5 w-3.5" /> {tr('Add override')}
      </ChromeButton>
    </div>
  );
}

function DashboardSettingsDialog({
  open,
  onOpenChange,
  definition,
  onChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  definition: DashboardDefinition;
  onChange: (
    next:
      | DashboardDefinition
      | ((current: DashboardDefinition) => DashboardDefinition),
  ) => void;
}) {
  const tr = useDashboardText();
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="dashboard-editor h-[88vh] max-w-5xl grid-rows-[auto_minmax(0,1fr)]">
        <DialogHeader>
          <DialogTitle>{tr('Dashboard settings')}</DialogTitle>
          <DialogDescription className="sr-only">
            {tr(
              'Configure Dashboard general settings, variables, annotations and links.',
            )}
          </DialogDescription>
        </DialogHeader>
        <Tabs
          defaultValue="general"
          className="grid min-h-0 grid-cols-[180px_minmax(0,1fr)]"
          orientation="vertical"
        >
          <TabsList className="h-fit flex-col items-stretch justify-start bg-bg-0 p-1">
            {([
              ['general', 'General'],
              ['variables', 'Variables'],
              ['annotations', 'Annotations'],
              ['links', 'Links'],
            ] as const).map(([value, label]) => (
              <TabsTrigger
                key={value}
                value={value}
                className="justify-start"
              >
                {tr(label)}
              </TabsTrigger>
            ))}
          </TabsList>
          <TabsContent
            value="general"
            className="mt-0 min-h-0 overflow-auto px-5"
          >
            <div className="mx-auto max-w-2xl space-y-5 pb-8">
              <EditorField label="Title">
                <EditorInput
                  value={definition.title}
                  onChange={(title) => onChange({ ...definition, title })}
                />
              </EditorField>
              <EditorField label="Description">
                <EditorTextarea
                  value={definition.description ?? ''}
                  rows={4}
                  onChange={(description) =>
                    onChange({
                      ...definition,
                      description: description || undefined,
                    })
                  }
                />
              </EditorField>
              <EditorField label="Tags">
                <EditorInput
                  value={definition.tags.join(', ')}
                  placeholder="operations, capacity"
                  onChange={(value) =>
                    onChange({
                      ...definition,
                      tags: value
                        .split(',')
                        .map((tag) => tag.trim())
                        .filter(Boolean),
                    })
                  }
                />
              </EditorField>
              <div>
                <EditorSectionTitle>Time</EditorSectionTitle>
                <div className="grid grid-cols-3 gap-2">
                  <EditorField label="From">
                    <EditorInput
                      value={definition.timeSettings.defaultFrom}
                      mono
                      onChange={(defaultFrom) =>
                        onChange({
                          ...definition,
                          timeSettings: {
                            ...definition.timeSettings,
                            defaultFrom,
                          },
                        })
                      }
                    />
                  </EditorField>
                  <EditorField label="To">
                    <EditorInput
                      value={definition.timeSettings.defaultTo}
                      mono
                      onChange={(defaultTo) =>
                        onChange({
                          ...definition,
                          timeSettings: {
                            ...definition.timeSettings,
                            defaultTo,
                          },
                        })
                      }
                    />
                  </EditorField>
                  <EditorField label="Timezone">
                    <EditorInput
                      value={definition.timeSettings.timezone}
                      onChange={(timezone) =>
                        onChange({
                          ...definition,
                          timeSettings: {
                            ...definition.timeSettings,
                            timezone,
                          },
                        })
                      }
                    />
                  </EditorField>
                </div>
              </div>
              <div>
                <EditorSectionTitle>Grid</EditorSectionTitle>
                <div className="grid grid-cols-3 gap-2">
                  <EditorField label="Columns">
                    <EditorNumber
                      value={definition.layout.columns}
                      min={1}
                      max={48}
                      onChange={(columns) =>
                        onChange({
                          ...definition,
                          layout: { ...definition.layout, columns },
                          elements: definition.elements.map((element) => ({
                            ...element,
                            gridPos: clampGridPosition(
                              element.gridPos,
                              columns,
                            ),
                          })),
                        })
                      }
                    />
                  </EditorField>
                  <EditorField label="Row height">
                    <EditorNumber
                      value={definition.layout.rowHeight}
                      min={2}
                      max={64}
                      onChange={(rowHeight) =>
                        onChange({
                          ...definition,
                          layout: { ...definition.layout, rowHeight },
                        })
                      }
                    />
                  </EditorField>
                  <EditorField label="Gap">
                    <EditorNumber
                      value={definition.layout.gap}
                      min={0}
                      max={64}
                      onChange={(gap) =>
                        onChange({
                          ...definition,
                          layout: { ...definition.layout, gap },
                        })
                      }
                    />
                  </EditorField>
                </div>
              </div>
              <div>
                <EditorSectionTitle>Refresh</EditorSectionTitle>
                <div className="grid grid-cols-2 gap-3">
                  <EditorField label="Refresh mode">
                    <EditorSelect
                      value={definition.refreshSettings.mode}
                      options={[
                        ['off', 'Off'],
                        ['interval', 'Interval'],
                        ['live', 'Auto'],
                      ]}
                      onChange={(value) =>
                        onChange({
                          ...definition,
                          refreshSettings: {
                            ...definition.refreshSettings,
                            enabled: value !== 'off',
                            mode: value as
                              | 'off'
                              | 'interval'
                              | 'live',
                          },
                        })
                      }
                    />
                  </EditorField>
                  {definition.refreshSettings.mode === 'interval' && (
                    <EditorField label="Default interval">
                      <EditorSelect
                        value={
                          definition.refreshSettings.defaultInterval ?? '30s'
                        }
                        options={definition.refreshSettings.allowedIntervals
                          .filter((value) => value !== 'off')
                          .map((value) => [value, value])}
                        onChange={(defaultInterval) =>
                          onChange({
                            ...definition,
                            refreshSettings: {
                              ...definition.refreshSettings,
                              enabled: true,
                              mode: 'interval',
                              defaultInterval,
                            },
                          })
                        }
                      />
                    </EditorField>
                  )}
                  {definition.refreshSettings.mode === 'live' && (
                    <div className="flex items-center rounded-md border border-bd-0 bg-bg-0 px-3 font-sans text-xs text-tx-2">
                      <span className="mr-2 h-1.5 w-1.5 rounded-full bg-success" />
                      {tr('Refresh interval adapts to time range and panel width')}
                    </div>
                  )}
                  {definition.refreshSettings.mode === 'off' && (
                    <div className="flex items-center rounded-md border border-bd-0 bg-bg-0 px-3 font-sans text-xs text-tx-3">
                      {tr('Use the refresh button to update panels manually')}
                    </div>
                  )}
                </div>
              </div>
              <DashboardInteractionSettingsEditor
                settings={definition.interactionSettings}
                onChange={(interactionSettings) =>
                  onChange({ ...definition, interactionSettings })
                }
              />
              <div className="grid grid-cols-2 gap-3 border-t border-bd-0 pt-4">
                <ToggleField
                  label="Editable"
                  checked={definition.editable}
                  onChange={(editable) =>
                    onChange({ ...definition, editable })
                  }
                />
                <ToggleField
                  label="Default dashboard"
                  checked={definition.defaultDashboard}
                  onChange={(defaultDashboard) =>
                    onChange({ ...definition, defaultDashboard })
                  }
                />
              </div>
            </div>
          </TabsContent>
          <TabsContent
            value="variables"
            className="mt-0 min-h-0 overflow-auto px-5"
          >
            <VariablesSettings
              variables={definition.variables}
              onChange={(variables) => onChange({ ...definition, variables })}
            />
          </TabsContent>
          <TabsContent
            value="annotations"
            className="mt-0 min-h-0 overflow-auto px-5"
          >
            <AnnotationsSettings
              annotations={definition.annotations}
              onChange={(annotations) =>
                onChange({ ...definition, annotations })
              }
            />
          </TabsContent>
          <TabsContent
            value="links"
            className="mt-0 min-h-0 overflow-auto px-5"
          >
            <DashboardLinksSettings
              links={definition.links}
              onChange={(links) => onChange({ ...definition, links })}
            />
          </TabsContent>
        </Tabs>
      </DialogContent>
    </Dialog>
  );
}

function VariablesSettings({
  variables,
  onChange,
}: {
  variables: DashboardVariable[];
  onChange: (variables: DashboardVariable[]) => void;
}) {
  const tr = useDashboardText();
  return (
    <SettingsCollection
      title="Variables"
      description="Variables can be reused in titles, queries, links and repeat rules."
      onAdd={() =>
        onChange([
          ...variables,
          {
            id: `variable-${nanoid(8)}`,
            name: `variable_${variables.length + 1}`,
            label: `${tr('Variable')} ${variables.length + 1}`,
            type: 'custom',
            query: {},
            options: [],
            multi: false,
            includeAll: false,
            hide: 'none',
            refresh: 'never',
          },
        ])
      }
    >
      {variables.map((variable, index) => (
        <div
          key={variable.id}
          className="space-y-3 rounded-md border border-bd-0 bg-bg-0 p-3"
        >
          <CollectionHeader
            title={variable.label || `${tr('Variable')} ${index + 1}`}
            onRemove={() =>
              onChange(
                variables.filter(
                  (candidate) => candidate.id !== variable.id,
                ),
              )
            }
          />
          <div className="grid grid-cols-3 gap-2">
            <EditorField label="Name">
              <EditorInput
                value={variable.name}
                mono
                onChange={(name) =>
                  updateCollection(
                    variables,
                    variable.id,
                    { ...variable, name },
                    onChange,
                  )
                }
              />
            </EditorField>
            <EditorField label="Label">
              <EditorInput
                value={variable.label}
                onChange={(label) =>
                  updateCollection(
                    variables,
                    variable.id,
                    { ...variable, label },
                    onChange,
                  )
                }
              />
            </EditorField>
            <EditorField label="Type">
              <EditorSelect
                value={variable.type}
                options={VARIABLE_TYPES.map((type) => [type, type])}
                onChange={(type) =>
                  updateCollection(
                    variables,
                    variable.id,
                    {
                      ...variable,
                      type: type as DashboardVariable['type'],
                    },
                    onChange,
                  )
                }
              />
            </EditorField>
          </div>
          {(variable.type === 'query' || variable.type === 'custom') &&
            (variable.type === 'query' ? (
              <VariableQueryEditor
                value={variable.query ?? {}}
                onChange={(query) =>
                  updateCollection(
                    variables,
                    variable.id,
                    { ...variable, query },
                    onChange,
                  )
                }
              />
            ) : (
              <EditorField label="Query / options">
                <EditorInput
                  value={(variable.options ?? [])
                    .map((option) => String(option.value))
                    .join(', ')}
                  placeholder="prod, staging, dev"
                  onChange={(value) =>
                    updateCollection(
                      variables,
                      variable.id,
                      {
                        ...variable,
                        options: value
                          .split(',')
                          .map((entry) => entry.trim())
                          .filter(Boolean)
                          .map((entry) => ({
                            label: entry,
                            value: entry,
                          })),
                      },
                      onChange,
                    )
                  }
                />
              </EditorField>
            ))}
          <div className="grid grid-cols-4 gap-3">
            <ToggleField
              label="Multi"
              checked={variable.multi}
              onChange={(multi) =>
                updateCollection(
                  variables,
                  variable.id,
                  { ...variable, multi },
                  onChange,
                )
              }
            />
            <ToggleField
              label="Include all"
              checked={variable.includeAll}
              onChange={(includeAll) =>
                updateCollection(
                  variables,
                  variable.id,
                  { ...variable, includeAll },
                  onChange,
                )
              }
            />
            <EditorField label="Hide">
              <EditorSelect
                value={variable.hide}
                options={[
                  ['none', 'None'],
                  ['label', 'Label'],
                  ['variable', 'Variable'],
                ]}
                onChange={(hide) =>
                  updateCollection(
                    variables,
                    variable.id,
                    {
                      ...variable,
                      hide: hide as DashboardVariable['hide'],
                    },
                    onChange,
                  )
                }
              />
            </EditorField>
            <EditorField label="Refresh">
              <EditorSelect
                value={variable.refresh}
                options={[
                  ['never', 'Never'],
                  ['dashboard_load', 'On load'],
                  ['time_range_change', 'Time range'],
                ]}
                onChange={(refresh) =>
                  updateCollection(
                    variables,
                    variable.id,
                    {
                      ...variable,
                      refresh: refresh as DashboardVariable['refresh'],
                    },
                    onChange,
                  )
                }
              />
            </EditorField>
          </div>
        </div>
      ))}
    </SettingsCollection>
  );
}

function AnnotationsSettings({
  annotations,
  onChange,
}: {
  annotations: DashboardAnnotation[];
  onChange: (annotations: DashboardAnnotation[]) => void;
}) {
  const tr = useDashboardText();
  return (
    <SettingsCollection
      title="Annotations"
      description="Overlay alerts, deployments, incidents, maintenance or custom events."
      onAdd={() =>
        onChange([
          ...annotations,
          {
            id: `annotation-${nanoid(8)}`,
            name: `${tr('Annotation')} ${annotations.length + 1}`,
            enabled: true,
            source: 'custom',
            query: {},
            display: 'line',
          },
        ])
      }
    >
      {annotations.map((annotation, index) => (
        <div
          key={annotation.id}
          className="space-y-3 rounded-md border border-bd-0 bg-bg-0 p-3"
        >
          <CollectionHeader
            title={annotation.name || `${tr('Annotation')} ${index + 1}`}
            onRemove={() =>
              onChange(
                annotations.filter(
                  (candidate) => candidate.id !== annotation.id,
                ),
              )
            }
          />
          <div className="grid grid-cols-3 gap-2">
            <EditorField label="Name">
              <EditorInput
                value={annotation.name}
                onChange={(name) =>
                  updateCollection(
                    annotations,
                    annotation.id,
                    { ...annotation, name },
                    onChange,
                  )
                }
              />
            </EditorField>
            <EditorField label="Source">
              <EditorSelect
                value={annotation.source}
                options={ANNOTATION_SOURCES.map((source) => [source, source])}
                onChange={(source) =>
                  updateCollection(
                    annotations,
                    annotation.id,
                    {
                      ...annotation,
                      source: source as DashboardAnnotation['source'],
                    },
                    onChange,
                  )
                }
              />
            </EditorField>
            <EditorField label="Display">
              <EditorSelect
                value={annotation.display}
                options={[
                  ['line', 'Line'],
                  ['region', 'Region'],
                  ['marker', 'Marker'],
                ]}
                onChange={(display) =>
                  updateCollection(
                    annotations,
                    annotation.id,
                    {
                      ...annotation,
                      display: display as DashboardAnnotation['display'],
                    },
                    onChange,
                  )
                }
              />
            </EditorField>
          </div>
          <div className="grid grid-cols-[1fr_160px] gap-2">
            <AnnotationEventsEditor
              value={annotation.query ?? {}}
              onChange={(query) =>
                updateCollection(
                  annotations,
                  annotation.id,
                  { ...annotation, query },
                  onChange,
                )
              }
            />
            <div className="space-y-3">
              <EditorField label="Color">
                <EditorInput
                  value={annotation.color ?? ''}
                  placeholder="var(--accent)"
                  onChange={(color) =>
                    updateCollection(
                      annotations,
                      annotation.id,
                      { ...annotation, color: color || undefined },
                      onChange,
                    )
                  }
                />
              </EditorField>
              <ToggleField
                label="Enabled"
                checked={annotation.enabled}
                onChange={(enabled) =>
                  updateCollection(
                    annotations,
                    annotation.id,
                    { ...annotation, enabled },
                    onChange,
                  )
                }
              />
            </div>
          </div>
        </div>
      ))}
    </SettingsCollection>
  );
}

function DashboardLinksSettings({
  links,
  onChange,
}: {
  links: DashboardLink[];
  onChange: (links: DashboardLink[]) => void;
}) {
  const tr = useDashboardText();
  return (
    <SettingsCollection
      title="Dashboard links"
      description="Link this dashboard to related dashboards or external runbooks."
      onAdd={() =>
        onChange([
          ...links,
          {
            id: `dashboard-link-${nanoid(8)}`,
            title: `${tr('Link')} ${links.length + 1}`,
            type: 'external',
            url: '',
            includeTimeRange: true,
            includeVariables: true,
            openInNewTab: false,
          },
        ])
      }
    >
      {links.map((link, index) => (
        <div
          key={link.id}
          className="space-y-3 rounded-md border border-bd-0 bg-bg-0 p-3"
        >
          <CollectionHeader
            title={link.title || `Link ${index + 1}`}
            onRemove={() =>
              onChange(
                links.filter((candidate) => candidate.id !== link.id),
              )
            }
          />
          <div className="grid grid-cols-[1fr_1fr_2fr] gap-2">
            <EditorField label="Title">
              <EditorInput
                value={link.title}
                onChange={(title) =>
                  updateCollection(
                    links,
                    link.id,
                    { ...link, title },
                    onChange,
                  )
                }
              />
            </EditorField>
            <EditorField label="Type">
              <EditorSelect
                value={link.type}
                options={[
                  ['external', 'External'],
                  ['dashboard', 'Dashboard'],
                ]}
                onChange={(type) =>
                  updateCollection(
                    links,
                    link.id,
                    {
                      ...link,
                      type: type as DashboardLink['type'],
                    },
                    onChange,
                  )
                }
              />
            </EditorField>
            <EditorField label="URL">
              <EditorInput
                value={link.url}
                mono
                onChange={(url) =>
                  updateCollection(
                    links,
                    link.id,
                    { ...link, url },
                    onChange,
                  )
                }
              />
            </EditorField>
          </div>
          <div className="grid grid-cols-3 gap-3">
            <ToggleField
              label="Time range"
              checked={link.includeTimeRange}
              onChange={(includeTimeRange) =>
                updateCollection(
                  links,
                  link.id,
                  { ...link, includeTimeRange },
                  onChange,
                )
              }
            />
            <ToggleField
              label="Variables"
              checked={link.includeVariables}
              onChange={(includeVariables) =>
                updateCollection(
                  links,
                  link.id,
                  { ...link, includeVariables },
                  onChange,
                )
              }
            />
            <ToggleField
              label="New tab"
              checked={link.openInNewTab}
              onChange={(openInNewTab) =>
                updateCollection(
                  links,
                  link.id,
                  { ...link, openInNewTab },
                  onChange,
                )
              }
            />
          </div>
        </div>
      ))}
    </SettingsCollection>
  );
}

function SettingsCollection({
  title,
  description,
  onAdd,
  children,
}: {
  title: string;
  description: string;
  onAdd: () => void;
  children: React.ReactNode;
}) {
  const tr = useDashboardText();
  return (
    <div className="mx-auto max-w-3xl space-y-4 pb-8">
      <div className="flex items-start gap-3">
        <div>
          <div className="font-sans text-sm font-semibold text-tx-1">
            {tr(title)}
          </div>
          <div className="mt-1 font-sans text-xs leading-5 text-tx-3">
            {tr(description)}
          </div>
        </div>
        <ChromeButton className="ml-auto" onClick={onAdd}>
          <Plus className="h-3.5 w-3.5" /> {tr('Add')}
        </ChromeButton>
      </div>
      {children}
    </div>
  );
}

function CollectionHeader({
  title,
  onRemove,
}: {
  title: string;
  onRemove: () => void;
}) {
  const tr = useDashboardText();
  return (
    <div className="flex items-center gap-2">
      <span className="min-w-0 flex-1 truncate font-sans text-xs font-semibold text-tx-1">
        {title}
      </span>
      <button
        type="button"
        aria-label={`${tr('Remove')} ${title}`}
        onClick={onRemove}
        className="text-tx-3 hover:text-danger"
      >
        <Trash2 className="h-3.5 w-3.5" />
      </button>
    </div>
  );
}

function updateCollection<T extends { id: string }>(
  values: T[],
  id: string,
  next: T,
  onChange: (values: T[]) => void,
): void {
  onChange(values.map((value) => (value.id === id ? next : value)));
}

function queryExpressionValue(query: PanelQuery): string {
  for (const key of ['expression', 'statement', 'sql', 'query']) {
    const value = query.query[key];
    if (typeof value === 'string') return value;
  }
  return '';
}

function queryEditorLanguage(query: PanelQuery): 'promql' | 'sql' {
  return stringValue(query.query.language).toLowerCase() === 'sql'
    ? 'sql'
    : 'promql';
}

function nextRefId(queries: readonly PanelQuery[]): string {
  const used = new Set(queries.map((query) => query.refId));
  for (let code = 65; code <= 90; code += 1) {
    const refId = String.fromCharCode(code);
    if (!used.has(refId)) return refId;
  }
  return `Q${queries.length + 1}`;
}

function localizeNewElement(
  element: DashboardElement,
  tr: (value: string) => string,
): DashboardElement {
  const title = {
    panel: 'New panel',
    text: 'Text',
    row: 'New row',
    group: 'New group',
    tab: 'New tabs',
  }[element.kind];
  const localized = { ...element, title: tr(title) };
  if (localized.kind === 'tab') {
    return {
      ...localized,
      tabs: localized.tabs.map((tab, index) => ({
        ...tab,
        title: `${tr('Tab')} ${index + 1}`,
      })),
    };
  }
  return localized;
}

function exportDashboardJson(definition: DashboardDefinition): void {
  const blob = new Blob([serializeDashboardDefinition(definition)], {
    type: 'application/json',
  });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = `${definition.title.replace(/[^\p{L}\p{N}._-]+/gu, '-') || 'dashboard'}.json`;
  anchor.click();
  URL.revokeObjectURL(url);
}

function stringValue(value: unknown): string {
  return typeof value === 'string' ? value : '';
}

const DATA_SOURCE_TYPES: PanelDataSourceType[] = [
  'metrics',
  'logs',
  'traces',
  'profiles',
  'sql',
];

const TRANSFORMATION_TYPES: TransformationType[] = [
  'filter_fields',
  'rename_fields',
  'organize_fields',
  'calculate_field',
  'reduce',
  'group_by',
  'sort_by',
  'limit',
  'join',
  'merge',
  'labels_to_fields',
  'rows_to_fields',
  'time_series_to_table',
];

const VARIABLE_TYPES: DashboardVariable['type'][] = [
  'query',
  'custom',
  'constant',
  'text',
  'interval',
  'data_source',
];

const ANNOTATION_SOURCES: DashboardAnnotation['source'][] = [
  'alerts',
  'deployments',
  'incidents',
  'maintenance',
  'custom',
];

const DATA_LINK_TARGETS: DataLink['target'][] = [
  'logs',
  'metrics',
  'traces',
  'profiles',
  'dashboard',
  'external',
];
