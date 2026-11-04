/**
 * The playlists of RUM sessions, as the server keeps them: lists of sessions of one application,
 * each session with the moment worth opening it at, in the order they were added. The rules are the
 * server's: a name is needed and is the application's alone, a playlist holds at most 500 sessions,
 * a session put in again takes the new moment and keeps its place, and a playlist is of an
 * application that exists. The playlists live in the mock server of one test.
 */
import type { Express, Request, Response } from 'express';

const MAX_SESSIONS = 500;
const MAX_NAME = 128;

interface Entry {
  sessionId: string;
  atTime: number | null;
  addedBy: string | null;
  addedAt: number;
}

interface Playlist {
  id: string;
  application: string;
  name: string;
  description: string;
  createdBy: string | null;
  createdAt: number;
  updatedAt: number;
  sessions: Entry[];
}

/** What a rule of the server refuses with. */
export class Refusal extends Error {
  constructor(
    message: string,
    readonly status = 400,
  ) {
    super(message);
  }
}

const present = (value: unknown) => typeof value === 'string' && value.trim() !== '';

const view = (playlist: Playlist) => {
  const { sessions, ...rest } = playlist;

  return { ...rest, sessionCount: sessions.length };
};

const detail = (playlist: Playlist) => ({ ...view(playlist), sessions: playlist.sessions });

/** What the server keeps from the sessions of a request: each once, the last moment given counting. */
function entriesOf(input: unknown, now: number): Entry[] {
  const sessions = Array.isArray(input) ? (input as Array<{ sessionId?: unknown; atTime?: unknown }>) : [];
  const found = new Map<string, number | null>();

  for (const session of sessions) {
    if (!present(session.sessionId) || String(session.sessionId).trim().length > 128) throw new Refusal('session id must contain between 1 and 128 characters');

    if (session.atTime !== undefined && session.atTime !== null && !(Number(session.atTime) > 0)) {
      throw new Refusal('atTime must be a moment after the epoch');
    }

    found.set(String(session.sessionId).trim(), session.atTime === undefined || session.atTime === null ? null : Number(session.atTime));
  }

  if (found.size > MAX_SESSIONS) throw new Refusal(`a playlist holds at most ${MAX_SESSIONS} sessions`);

  return [...found]
    .sort((a, b) => a[0].localeCompare(b[0]))
    .map(([sessionId, atTime]) => ({ sessionId: sessionId, atTime: atTime, addedBy: 'u1', addedAt: now }));
}

export function registerPlaylistRoutes(
  app: Express,
  options: {
    now: number;
    /** The ids of the applications that exist. */
    applications: () => string[];
    guarded: (handler: (req: Request, res: Response) => void) => (req: Request, res: Response) => void;
    /** The playlists the organization starts with. */
    seed: Array<{ name: string; description?: string; application: string; sessions: Array<{ id: string; at?: number }>; ago: number }>;
  },
): void {
  const { now, guarded } = options;
  const playlists = new Map<string, Playlist>();
  let counter = 0;

  const sorted = () => [...playlists.values()].sort((a, b) => b.updatedAt - a.updatedAt || a.id.localeCompare(b.id));

  for (const seeded of options.seed) {
    counter += 1;

    const id = `pl-${counter}`;
    const at = now - seeded.ago;

    playlists.set(id, {
      id,
      application: seeded.application,
      name: seeded.name,
      description: seeded.description ?? '',
      createdBy: 'u1',
      createdAt: at,
      updatedAt: at,
      sessions: seeded.sessions.map((session, index) => ({
        sessionId: session.id,
        atTime: session.at ?? null,
        addedBy: 'u1',
        addedAt: at + index,
      })),
    });
  }

  const found = (req: Request): Playlist => {
    const playlist = playlists.get(String(req.params.id));

    if (!playlist) throw new Refusal('RUM playlist not found', 404);

    return playlist;
  };

  const checkName = (name: unknown, application: string, except?: string) => {
    if (!present(name) || String(name).trim().length > MAX_NAME) throw new Refusal(`playlist name must contain between 1 and ${MAX_NAME} characters`);

    const taken = [...playlists.values()].some(
      (other) => other.id !== except && other.application === application && other.name.toLowerCase() === String(name).trim().toLowerCase(),
    );

    if (taken) throw new Refusal('a playlist of this application already has this name', 409);
  };

  const reply = (res: Response, playlist: Playlist) => res.json(detail(playlist));

  app.get(
    '/api/v1/rum/playlists',
    guarded((req, res) => {
      const application = String(req.query.application ?? '').trim();

      res.json(sorted().filter((playlist) => application === '' || playlist.application === application).map(view));
    }),
  );

  app.post(
    '/api/v1/rum/playlists',
    guarded((req, res) => {
      const body = req.body as { application?: string; name?: string; description?: string; sessions?: unknown };
      const application = String(body.application ?? '').trim();

      if (!options.applications().includes(application)) throw new Refusal(`RUM application \`${application}\` does not exist`, 404);

      checkName(body.name, application);

      counter += 1;

      const playlist: Playlist = {
        id: `pl-${counter}`,
        application,
        name: String(body.name).trim(),
        description: body.description ?? '',
        createdBy: 'u1',
        createdAt: now,
        updatedAt: now,
        sessions: entriesOf(body.sessions, now),
      };

      playlists.set(playlist.id, playlist);
      reply(res, playlist);
    }),
  );

  app.get('/api/v1/rum/playlists/:id', guarded((req, res) => reply(res, found(req))));

  app.patch(
    '/api/v1/rum/playlists/:id',
    guarded((req, res) => {
      const playlist = found(req);
      const body = req.body as { name?: string; description?: string };

      if (body.name !== undefined) {
        checkName(body.name, playlist.application, playlist.id);
        playlist.name = body.name.trim();
      }

      if (body.description !== undefined) playlist.description = body.description;

      playlist.updatedAt = now;
      reply(res, playlist);
    }),
  );

  app.delete(
    '/api/v1/rum/playlists/:id',
    guarded((req, res) => {
      playlists.delete(found(req).id);
      res.json({ deleted: true });
    }),
  );

  app.post(
    '/api/v1/rum/playlists/:id/sessions',
    guarded((req, res) => {
      const playlist = found(req);
      const added = entriesOf((req.body as { sessions?: unknown }).sessions, now);
      const total = new Set([...playlist.sessions.map((entry) => entry.sessionId), ...added.map((entry) => entry.sessionId)]);

      if (total.size > MAX_SESSIONS) throw new Refusal(`a playlist holds at most ${MAX_SESSIONS} sessions`);

      for (const entry of added) {
        const held = playlist.sessions.find((candidate) => candidate.sessionId === entry.sessionId);

        if (held) held.atTime = entry.atTime;
        else playlist.sessions.push(entry);
      }

      playlist.updatedAt = now;
      reply(res, playlist);
    }),
  );

  app.delete(
    '/api/v1/rum/playlists/:id/sessions/:sessionId',
    guarded((req, res) => {
      const playlist = found(req);

      playlist.sessions = playlist.sessions.filter((entry) => entry.sessionId !== req.params.sessionId);
      playlist.updatedAt = now;
      reply(res, playlist);
    }),
  );

  app.get('/api/v1/rum/sessions/:id/playlists', (req, res) => {
    res.json(sorted().filter((playlist) => playlist.sessions.some((entry) => entry.sessionId === req.params.id)).map(view));
  });
}
