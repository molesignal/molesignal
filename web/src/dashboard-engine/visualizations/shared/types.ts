import type { DashboardPanel, PanelData } from '../../schema';

export interface VisualizationProps<
  TOptions = Record<string, unknown>,
> {
  panel: DashboardPanel;
  data: PanelData;
  options: TOptions;
  height: number;
  cursorScopeId?: string | null | undefined;
}

export interface VisualizationEditorProps<
  TOptions = Record<string, unknown>,
> {
  options: TOptions;
  onChange: (options: TOptions) => void;
}
