/**
 * The investigation guide of an alert rule: what to do first when one of its
 * incidents is looked into. People read it on the incident; Mole Agent reads it
 * before it investigates. Every part may be absent.
 */
export interface RuleGuide {
  /** Markdown: what to check first and in what order. */
  steps?: string;
  /** Queries run over the incident's time range, in the order listed. */
  queries?: RuleGuideQuery[];
  /** Ids of the dashboards to open beside the incident. */
  dashboards?: string[];
}

export interface RuleGuideQuery {
  name: string;
  language: 'sql' | 'promql';
  /** The stream a SQL query reads; a PromQL query may leave it out. */
  stream?: { name: string; streamType: 'logs' | 'metrics' | 'traces' } | null;
  statement: string;
}
