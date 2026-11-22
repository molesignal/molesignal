/** The shape of the datasource catalogue and the slots its commands carry. */

export type Category =
  | 'recommended'
  | 'otel'
  | 'otel-collector'
  | 'custom'
  | 'servers'
  | 'databases'
  | 'security'
  | 'devops'
  | 'networking'
  | 'queues'
  | 'languages'
  | 'ai';

export type Signal = 'logs' | 'metrics' | 'traces' | 'profiles';

export interface GuideStep {
  /** A command or file to copy; `{{ENDPOINT}}`-style slots are filled in when it is shown. */
  code?: { lang: string; content: string };
  /** A check the reader runs where the collector is deployed, shown with the verification. */
  verification?: boolean;
}

export interface Source {
  id: string;
  category: Category;
  signals: Signal[];
  docsUrl?: string;
  rumPlatform?: 'browser' | 'flutter' | 'android' | 'ios';
  steps: GuideStep[];
}

// Rendered placeholders. The guide substitutes the live access URL,
// host / port / tls flag, and the user's default intake token at render
// time (see `substitute`). They are intentionally NOT real values here so the
// catalogue stays a pure static module. Org never appears in a path.
export const ENDPOINT = '{{ENDPOINT}}'; // full origin, e.g. https://obs.example.com
export const ENDPOINT_HOST = '{{ENDPOINT_HOST}}'; // bare hostname, no scheme/port
export const ENDPOINT_PORT = '{{ENDPOINT_PORT}}'; // numeric port
export const ENDPOINT_TLS = '{{ENDPOINT_TLS}}'; // Fluent Bit tls flag: On / Off
export const TOKEN = '{{TOKEN}}';
