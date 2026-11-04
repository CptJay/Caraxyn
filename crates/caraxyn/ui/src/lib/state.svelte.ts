// The app's shared state and the actions that change it. Views read `app` and call these.

import { open } from '@tauri-apps/plugin-dialog';
import {
  api,
  onNodeEvent,
  onResync,
  type Contact,
  type ContentHash,
  type Millis,
  type NodeEvent,
  type Offer,
  type PeerId,
  type PendingOffer,
  type Settings,
  type Share,
  type Status,
} from './api';
import { errorText, shortId } from './format';

export type View = 'inbox' | 'shares' | 'contacts' | 'settings';

export interface Download {
  bytes: number;
  total: number | null;
}

/** Recipient + share, the key for per-pair maps. */
export const pairKey = (peer: PeerId, root: ContentHash): string => `${peer}:${root}`;

export const app = $state({
  view: 'inbox' as View,
  loading: true,
  status: null as Status | null,
  contacts: [] as Contact[],
  inbox: [] as Offer[],
  shares: [] as Share[],
  pending: [] as PendingOffer[],
  /** Running downloads by `pairKey(from, root)`. */
  downloads: {} as Record<string, Download>,
  /** Offers the recipient refused, by `pairKey(to, root)`. */
  // shortcut: only known for this run (the core does not keep it), so a restart shows them as delivered.
  undeliverable: {} as Record<string, true>,
  /** The last failed action, shown above the current view. */
  error: null as string | null,
  /** Problems in background work, newest last. */
  warnings: [] as string[],
});

export async function init(): Promise<void> {
  try {
    app.status = await api.status();
    if (app.status.peerId) {
      await onNodeEvent(applyEvent);
      await onResync(() => void reload());
      await reload();
    }
  } catch (e) {
    app.error = errorText(e);
  } finally {
    app.loading = false;
  }
}

/** Reads every list again from the node. */
export async function reload(): Promise<void> {
  [app.contacts, app.inbox, app.shares, app.pending] = await Promise.all([
    api.contacts(),
    api.inbox(),
    api.shares(),
    api.pendingOffers(),
  ]);
}

function applyEvent(event: NodeEvent): void {
  switch (event.type) {
    case 'contactAdded':
      app.contacts = [...app.contacts.filter((c) => c.peer !== event.contact.peer), event.contact];
      break;
    case 'offerReceived': {
      const { from, root } = event.offer;
      app.inbox = [...app.inbox.filter((o) => !(o.from === from && o.root === root)), event.offer];
      break;
    }
    case 'offerRevoked':
      app.inbox = app.inbox.map((o) =>
        o.from === event.from && o.root === event.root && o.state !== 'done'
          ? { ...o, state: 'revoked' }
          : o,
      );
      break;
    case 'undeliverable':
      app.undeliverable[pairKey(event.to, event.root)] = true;
      void refreshPending();
      break;
    case 'delivered':
      void refreshPending();
      break;
    case 'progress': {
      const key = pairKey(event.from, event.root);
      if (key in app.downloads) app.downloads[key] = { bytes: event.bytes, total: event.total };
      break;
    }
    case 'warning':
      app.warnings = [...app.warnings, `${event.context}: ${event.message}`];
      break;
  }
}

async function refreshPending(): Promise<void> {
  app.pending = await api.pendingOffers();
}

/** Runs `action`; on failure shows its error. Returns whether it worked. */
export async function attempt(action: () => Promise<unknown>): Promise<boolean> {
  app.error = null;
  try {
    await action();
    return true;
  } catch (e) {
    app.error = errorText(e);
    return false;
  }
}

export function contactName(peer: PeerId): string {
  return app.contacts.find((c) => c.peer === peer)?.name ?? shortId(peer);
}

/** Asks for a folder (or a file) with the native dialog; `null` if cancelled. */
export async function pickPath(directory: boolean): Promise<string | null> {
  return open({ directory, multiple: false });
}

// Inbox

/** Downloads an offer into the default folder, or `dest` / a picked one. */
export async function accept(offer: Offer, dest?: string): Promise<void> {
  const folder = dest ?? app.status?.settings.downloadDir ?? (await pickPath(true));
  if (!folder) return;
  const key = pairKey(offer.from, offer.root);
  app.downloads[key] = { bytes: 0, total: offer.totalSize };
  await attempt(async () => {
    await api.accept(offer.from, offer.root, folder);
    app.inbox = await api.inbox();
  });
  delete app.downloads[key];
}

export async function decline(offer: Offer): Promise<void> {
  await attempt(async () => {
    await api.decline(offer.from, offer.root);
    app.inbox = await api.inbox();
  });
}

// Shares

export async function share(path: string, recipients: PeerId[], expiresAt: Millis | null): Promise<boolean> {
  return attempt(async () => {
    await api.share(path, recipients, expiresAt);
    [app.shares, app.pending] = await Promise.all([api.shares(), api.pendingOffers()]);
  });
}

export async function revoke(root: ContentHash): Promise<void> {
  await attempt(async () => {
    await api.revoke(root);
    [app.shares, app.pending] = await Promise.all([api.shares(), api.pendingOffers()]);
  });
}

export async function setExpiry(root: ContentHash, expiresAt: Millis | null): Promise<void> {
  await attempt(async () => {
    await api.setExpiry(root, expiresAt);
    [app.shares, app.pending] = await Promise.all([api.shares(), api.pendingOffers()]);
  });
}

// Contacts

/** A new invite code, or `null` if it could not be made. */
export async function createInvite(): Promise<string | null> {
  let code: string | null = null;
  await attempt(async () => (code = await api.createInvite()));
  return code;
}

export function redeemInvite(code: string): Promise<boolean> {
  return attempt(async () => {
    await api.redeemInvite(code.trim());
    app.contacts = await api.contacts();
  });
}

export async function removeContact(peer: PeerId): Promise<void> {
  await attempt(async () => {
    await api.removeContact(peer);
    [app.contacts, app.shares, app.pending] = await Promise.all([
      api.contacts(),
      api.shares(),
      api.pendingOffers(),
    ]);
  });
}

// Settings

export function saveSettings(settings: Settings): Promise<boolean> {
  return attempt(async () => {
    await api.saveSettings(settings);
    if (app.status) app.status.settings = settings;
  });
}

export const openLogFolder = (): Promise<boolean> => attempt(api.openLogFolder);

/** Where an offer (or its revoke) stands for one recipient of our share. */
export type RecipientState = 'pending' | 'delivered' | 'refused' | 'revoking' | 'revoked';

export function recipientState(share: Share, peer: PeerId): { state: RecipientState; attempts: number } {
  const mail = app.pending.find((p) => p.to === peer && p.root === share.root);
  const attempts = mail?.attempts ?? 0;
  if (share.revokedAt !== null) return { state: mail?.revoke ? 'revoking' : 'revoked', attempts };
  if (mail) return { state: 'pending', attempts };
  if (app.undeliverable[pairKey(peer, share.root)]) return { state: 'refused', attempts };
  return { state: 'delivered', attempts };
}
