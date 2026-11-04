// The rules the inbox and "My shares" show, checked without the Rust side.

import { beforeEach, describe, expect, it, vi } from 'vitest';

// The real calls need the Tauri window; event handlers only refresh lists through them.
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => []) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));

import type { Offer, Share } from './api';
import { app, applyEvent, pairKey, recipientState } from './state.svelte';

const A = 'a'.repeat(64);
const B = 'b'.repeat(64);
const ROOT = 'f'.repeat(64);

const share = (revokedAt: number | null = null): Share => ({
  root: ROOT,
  title: 'trip',
  recipients: [A, B],
  fileCount: 1,
  totalSize: 1,
  revokedAt,
  expiresAt: null,
});

const offer = (state: Offer['state']): Offer => ({
  from: A,
  root: ROOT,
  title: 'trip',
  fileCount: 1,
  totalSize: 1,
  preview: [],
  expiresAt: null,
  receivedAt: 1,
  state,
});

beforeEach(() => {
  app.pending = [];
  app.undeliverable = {};
  app.inbox = [];
});

describe('recipientState', () => {
  it('is pending while an offer waits, delivered once it is gone', () => {
    app.pending = [{ to: A, root: ROOT, revoke: false, attempts: 2 }];
    expect(recipientState(share(), A)).toEqual({ state: 'pending', attempts: 2 });
    expect(recipientState(share(), B)).toEqual({ state: 'delivered', attempts: 0 });
  });

  it('is undelivered when the offer will not be retried', () => {
    app.undeliverable[pairKey(A, ROOT)] = true;
    expect(recipientState(share(), A).state).toBe('undelivered');
  });

  it('after a revoke: revoking while the news waits, then revoked', () => {
    app.pending = [{ to: A, root: ROOT, revoke: true, attempts: 0 }];
    expect(recipientState(share(5), A).state).toBe('revoking');
    expect(recipientState(share(5), B).state).toBe('revoked');
  });
});

describe('applyEvent', () => {
  it('a revoke withdraws an unsaved offer but leaves a saved one saved', () => {
    for (const [before, after] of [
      ['new', 'revoked'],
      ['declined', 'revoked'],
      ['done', 'done'],
    ] as const) {
      app.inbox = [offer(before)];
      applyEvent({ type: 'offerRevoked', from: A, root: ROOT });
      expect(app.inbox[0].state).toBe(after);
    }
  });

  it('an offer arriving again replaces the old one instead of adding a second', () => {
    app.inbox = [offer('revoked')];
    applyEvent({ type: 'offerReceived', offer: offer('new') });
    expect(app.inbox).toHaveLength(1);
    expect(app.inbox[0].state).toBe('new');
  });

  it('an upload shows on its recipient while it runs and goes when it ends', () => {
    app.uploads = {};
    applyEvent({ type: 'upload', to: A, root: ROOT, bytes: 0, total: 10, finished: false });
    applyEvent({ type: 'upload', to: A, root: ROOT, bytes: 4, total: 10, finished: false });
    expect(recipientState(share(), A).upload?.bytes).toBe(4);
    expect(recipientState(share(), B).upload).toBeUndefined();

    applyEvent({ type: 'upload', to: A, root: ROOT, bytes: 10, total: 10, finished: true });
    expect(app.uploads).toEqual({});
    expect(recipientState(share(), A).state).toBe('delivered');
  });

  it('progress only counts for downloads this window started', () => {
    app.downloads = { [pairKey(A, ROOT)]: { bytes: 0, total: 10, speed: null, at: 0 } };
    applyEvent({ type: 'progress', from: A, root: ROOT, bytes: 4, total: 10 });
    applyEvent({ type: 'progress', from: B, root: ROOT, bytes: 9, total: 10 });
    expect(Object.keys(app.downloads)).toEqual([pairKey(A, ROOT)]);
    expect(app.downloads[pairKey(A, ROOT)].bytes).toBe(4);
  });
});
