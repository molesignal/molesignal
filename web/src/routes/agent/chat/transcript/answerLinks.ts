import * as React from 'react';
import { matchRoutes, UNSAFE_DataRouterContext } from 'react-router-dom';

/** Where a link in an answer leads: a page of the product, or a web address in a new tab. */
export interface AnswerLink {
  to: string;
  external: boolean;
}

const KNOWLEDGE_WORDS = /^(知识库|知识|knowledge|kb)$/i;

/**
 * A knowledge file named the way a model may name it: `MOLE.md`,
 * `知识库 / runbooks/api.md`, `knowledge/services.md`. Null when the text is
 * not a file path.
 */
export function knowledgeFileOf(raw: string): string | null {
  const parts = raw
    .split('/')
    .map((part) => part.trim())
    .filter(Boolean);
  while (parts.length > 1 && KNOWLEDGE_WORDS.test(parts[0] ?? '')) parts.shift();
  const path = parts.join('/');
  return /^[^\s?#]+\.(md|markdown|txt)$/i.test(path) ? path : null;
}

/**
 * Reads the routes a model writes into an answer. A path of the product is
 * followed only when a page answers it, a web address opens in a new tab, and a
 * knowledge file opens on the Knowledge tab; anything else is left as text, so
 * an invented route never leads to a missing page.
 */
export function useAnswerLinks(): (raw: string | undefined) => AnswerLink | null {
  const dataRouter = React.useContext(UNSAFE_DataRouterContext);
  return React.useCallback(
    (raw: string | undefined) => {
      const value = raw?.trim();
      if (!value) return null;
      if (/^https?:\/\//i.test(value)) return { to: value, external: true };

      if (value.startsWith('/') && !value.startsWith('//')) {
        const routes = dataRouter?.router.routes;
        // Outside the app's router (a test) the path is taken as it is.
        if (!routes) return { to: value, external: false };
        const pathname = value.split(/[?#]/)[0] ?? value;
        const matches = matchRoutes(routes, pathname);
        // The catch-all routes answer every path: a match there is a missing page.
        const page = matches?.[matches.length - 1]?.route.path;
        if (matches && page !== '*') return { to: value, external: false };
      }

      const file = knowledgeFileOf(value.replace(/^\/+/, ''));
      return file
        ? { to: `/agent/knowledge?path=${encodeURIComponent(file)}`, external: false }
        : null;
    },
    [dataRouter],
  );
}
