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
import { advance, startTransfer, type Transfer } from './transfer';

export type View = 'inbox' | 'shares' | 'contacts' | 'settings';

/** A running download: bytes so far, total, and its speed. */
export type Download = Transfer;

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
  /** Contacts downloading our shares right now, by `pairKey(to, root)`. */
  uploads: {} as Record<string, Transfer>,
  /** Offers that will not be retried (refused, or the share expired first), by `pairKey(to, root)`. */
  // shortcut: only known for this run (the core does not keep it), so a restart shows them as delivered.
  undeliverable: {} as Record<string, true>,
  /** The last failed action, shown above the current view. */
  error: null as string | null,
  /** Problems in background work, newest last. */
  warnings: [] as string[],
  /** A newer version that can be installed, if one was found. */
  update: null as string | null,
});

/** How often a running app looks for a new version. */
const UPDATE_CHECK_MS = 6 * 60 * 60 * 1000;

export async function init(): Promise<void> {
  try {
    app.status = await api.status();
    if (app.status.peerId) {
      await onNodeEvent(applyEvent);
      await onResync(() => inBackground(reload()));
      await reload();
    }
    // Quietly: offline, or no published release yet, is not worth a banner.
    const lookQuietly = () => checkForUpdate().catch((e) => console.warn('update check failed', e));
    lookQuietly();
    setInterval(lookQuietly, UPDATE_CHECK_MS);
  } catch (e) {
    app.error = errorText(e);
  } finally {
    app.loading = false;
  }
}

/** Reads every list again from the node. */
export async function reload(): Promise<void> {
  // Known only from events, which may have been missed; running ones report again within a second.
  app.uploads = {};
  [app.contacts, app.inbox, app.shares, app.pending] = await Promise.all([
    api.contacts(),
    api.inbox(),
    api.shares(),
    api.pendingOffers(),
  ]);
}

/** Updates the lists from one node event. Exported for tests. */
export function applyEvent(event: NodeEvent): void {
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
      inBackground(refreshShares());
      break;
    case 'delivered':
      inBackground(refreshShares());
      break;
    case 'netStatus':
      if (app.status) app.status.netStatus = event.status;
      break;
    case 'upload': {
      const key = pairKey(event.to, event.root);
      if (event.finished) {
        delete app.uploads[key];
      } else {
        const now = performance.now();
        const running = app.uploads[key] ?? startTransfer(event.total, now);
        app.uploads[key] = advance(running, event.bytes, event.total, now, event.direct);
      }
      break;
    }
    case 'progress': {
      const key = pairKey(event.from, event.root);
      const running = app.downloads[key];
      if (running) app.downloads[key] = advance(running, event.bytes, event.total, performance.now(), event.direct);
      break;
    }
    case 'warning':
      app.warnings = [...app.warnings, `${event.context}: ${event.message}`];
      break;
  }
}

/** Reads our shares and what is still waiting to reach their recipients. */
async function refreshShares(): Promise<void> {
  [app.shares, app.pending] = await Promise.all([api.shares(), api.pendingOffers()]);
}

/** For work no click waits on (event-driven refreshes): a failure still shows. */
function inBackground(work: Promise<unknown>): void {
  work.catch((e) => (app.error = errorText(e)));
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
  app.downloads[key] = startTransfer(offer.totalSize, performance.now());
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

export async function forgetOffer(offer: Offer): Promise<void> {
  await attempt(async () => {
    await api.forgetOffer(offer.from, offer.root);
    app.inbox = await api.inbox();
  });
}

// Shares

export async function share(path: string, recipients: PeerId[], expiresAt: Millis | null): Promise<boolean> {
  return attempt(async () => {
    await api.share(path, recipients, expiresAt);
    await refreshShares();
  });
}

export async function revoke(root: ContentHash): Promise<void> {
  await attempt(async () => {
    await api.revoke(root);
    await refreshShares();
  });
}

export async function forgetShare(root: ContentHash): Promise<void> {
  await attempt(async () => {
    await api.forgetShare(root);
    await refreshShares();
  });
}

export async function setExpiry(root: ContentHash, expiresAt: Millis | null): Promise<void> {
  await attempt(async () => {
    await api.setExpiry(root, expiresAt);
    await refreshShares();
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
    app.contacts = await api.contacts();
    await refreshShares();
  });
}

// Updates

/** Looks for a newer version; sets `app.update` and returns it, or `null`. Throws when the check fails. */
export async function checkForUpdate(): Promise<string | null> {
  app.update = await api.checkUpdate();
  return app.update;
}

/** Installs `app.update`; on success the app restarts, so this only returns on failure. */
export const installUpdate = (): Promise<boolean> => attempt(api.installUpdate);

// Settings

export function saveSettings(settings: Settings): Promise<boolean> {
  return attempt(async () => {
    await api.saveSettings(settings);
    if (app.status) app.status.settings = settings;
  });
}

export const openLogFolder = (): Promise<boolean> => attempt(api.openLogFolder);

/** Copies `text`, calling `hint(true)` now and `hint(false)` a moment later, for a "Copied" label. */
export function copyText(text: string, hint: (copied: boolean) => void): Promise<boolean> {
  return attempt(async () => {
    await navigator.clipboard.writeText(text);
    hint(true);
    setTimeout(() => hint(false), 1500);
  });
}

/** Where an offer (or its revoke) stands for one recipient of our share. */
export type RecipientState = 'pending' | 'delivered' | 'undelivered' | 'revoking' | 'revoked';

export function recipientState(
  share: Share,
  peer: PeerId,
): { state: RecipientState; attempts: number; upload?: Transfer } {
  const upload = app.uploads[pairKey(peer, share.root)];
  if (upload) return { state: 'delivered', attempts: 0, upload };
  const mail = app.pending.find((p) => p.to === peer && p.root === share.root);
  const attempts = mail?.attempts ?? 0;
  if (share.revokedAt !== null) return { state: mail?.revoke ? 'revoking' : 'revoked', attempts };
  if (mail) return { state: 'pending', attempts };
  if (app.undeliverable[pairKey(peer, share.root)]) return { state: 'undelivered', attempts };
  return { state: 'delivered', attempts };
}
