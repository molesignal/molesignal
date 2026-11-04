/**
 * What is kept about a recorded session besides its events: the comments people leave on its replay and
 * the summary a model writes of it. They live in the mock server of one test. No model runs: the summary
 * of a session is made of the first two pages it opened, so that a test can tell it was asked for.
 */
import type { Express, Request, Response } from 'express';

import type { RumComment, RumRow, RumSessionSummary } from '../../src/api/rum';

const ME = { id: 'u1', name: 'Mock User' };

export function registerRumNoteRoutes(app: Express, sessionRow: (id: string) => RumRow | undefined, views: () => readonly RumRow[], now: number): void {
  const comments = new Map<string, RumComment[]>();
  const summaries = new Map<string, RumSessionSummary>();
  let next = 1;

  const of = (req: Request) => String(req.params.id);
  const known = (req: Request, res: Response) => {
    if (sessionRow(of(req))) return true;

    res.status(404).json({ error: 'not_found', message: 'RUM session not found' });

    return false;
  };

  app.get('/api/v1/rum/sessions/:id/comments', (req, res) => {
    res.json({ comments: [...(comments.get(of(req)) ?? [])].sort((a, b) => a.atTime - b.atTime) });
  });

  app.post('/api/v1/rum/sessions/:id/comments', (req, res) => {
    const body = String(req.body?.body ?? '').trim();

    if (!body) {
      res.status(400).json({ error: 'invalid', message: 'a comment must contain between 1 and 2000 characters' });

      return;
    }

    const comment: RumComment = {
      id: `comment-${next++}`,
      application: String(req.body.application),
      sessionId: of(req),
      atTime: Number(req.body.atTime),
      body,
      authorId: ME.id,
      authorName: ME.name,
      createdAt: now,
      updatedAt: now,
      editable: true,
    };

    comments.set(of(req), [...(comments.get(of(req)) ?? []), comment]);
    res.json(comment);
  });

  app.patch('/api/v1/rum/sessions/:id/comments/:comment', (req, res) => {
    const found = (comments.get(of(req)) ?? []).find((comment) => comment.id === req.params.comment);

    if (!found) {
      res.status(404).json({ error: 'not_found', message: 'RUM replay comment not found' });

      return;
    }

    found.body = String(req.body?.body ?? '').trim();
    res.json(found);
  });

  app.delete('/api/v1/rum/sessions/:id/comments/:comment', (req, res) => {
    comments.set(of(req), (comments.get(of(req)) ?? []).filter((comment) => comment.id !== req.params.comment));
    res.json({ deleted: true });
  });

  app.get('/api/v1/rum/sessions/:id/summary', (req, res) => {
    res.json({ summary: summaries.get(of(req)) ?? null });
  });

  app.post('/api/v1/rum/sessions/:id/summary', (req, res) => {
    if (!known(req, res)) return;

    const session = sessionRow(of(req)) as RumRow;
    const pages = views()
      .filter((row) => row['session.id'] === of(req))
      .sort((a, b) => a.timestamp - b.timestamp);

    const summary: RumSessionSummary = {
      sessionId: of(req),
      language: 'en-us',
      summary: `They opened ${String(session['session.landing_view.url_path'])} and went on from there.`,
      chapters: pages.slice(0, 2).map((row) => ({
        atTime: row.timestamp,
        title: `On ${String(row['view.url_path'])}`,
        summary: `They were on ${String(row['view.url_path'])}.`,
      })),
      model: 'mock-model',
      promptTokens: 120,
      completionTokens: 30,
      generatedAt: now,
    };

    summaries.set(of(req), summary);
    res.json({ summary });
  });
}
