import { useParams } from 'react-router-dom';

import { KnowledgeView } from './KnowledgeView';
import { MemoryView } from './MemoryView';

/** The two views of the Knowledge tab, chosen in the navigation: `/agent/knowledge[/memory]`. */
export const KNOWLEDGE_VIEWS = [
  { value: 'knowledge', path: '/agent/knowledge', key: 'knowledge.views.knowledge' },
  { value: 'memory', path: '/agent/knowledge/memory', key: 'knowledge.views.memory' },
] as const;

/** Which view of the Knowledge tab a path is, or null outside it. */
export function knowledgeViewOf(pathname: string): 'knowledge' | 'memory' | null {
  if (pathname === '/agent/knowledge/memory' || pathname.startsWith('/agent/knowledge/memory/')) {
    return 'memory';
  }
  return pathname === '/agent/knowledge' || pathname.startsWith('/agent/knowledge/')
    ? 'knowledge'
    : null;
}

/**
 * The Knowledge tab: what Mole Agent knows about this organization and its
 * teams (files every chat of the scope reads), and what it remembers from
 * chats (the person's own memories and their teams').
 */
export function AgentKnowledgePage() {
  const { view } = useParams<{ view?: string }>();
  return view === 'memory' ? <MemoryView /> : <KnowledgeView />;
}
