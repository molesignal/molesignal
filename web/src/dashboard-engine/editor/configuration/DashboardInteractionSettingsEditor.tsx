import {
  EditorField,
  EditorSectionTitle,
  EditorSelect,
} from './controls';
import {
  DEFAULT_DASHBOARD_CURSOR_SYNC_MODE,
  type DashboardCursorSyncMode,
  type DashboardInteractionSettings,
} from '../../schema';

const CURSOR_SYNC_OPTIONS = [
  ['off', 'Off'],
  ['shared_crosshair', 'Shared crosshair'],
] as const;

export function DashboardInteractionSettingsEditor({
  settings,
  onChange,
}: {
  settings: DashboardInteractionSettings | undefined;
  onChange: (settings: DashboardInteractionSettings) => void;
}) {
  return (
    <div>
      <EditorSectionTitle>Interactions</EditorSectionTitle>
      <div className="max-w-sm">
        <EditorField label="Graph tooltip">
          <EditorSelect
            value={
              settings?.cursorSync ?? DEFAULT_DASHBOARD_CURSOR_SYNC_MODE
            }
            options={CURSOR_SYNC_OPTIONS}
            onChange={(cursorSync) =>
              onChange({ cursorSync: cursorSync as DashboardCursorSyncMode })
            }
          />
        </EditorField>
      </div>
    </div>
  );
}
