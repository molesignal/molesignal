import { QuickActions } from './QuickActions';
import type { StarterSelection } from './types';

export interface AgentWorkspaceProps {
  displayName: string;
  onPrime: (selection: StarterSelection) => void;
}

export function AgentWorkspace({ displayName, onPrime }: AgentWorkspaceProps) {
  return <QuickActions displayName={displayName} onPrime={onPrime} />;
}
