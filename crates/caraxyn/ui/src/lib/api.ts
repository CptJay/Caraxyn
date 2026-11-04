// The Rust side, typed. The only file that calls `invoke` or `listen`.
// Mirrors crates/caraxyn/src/dto.rs and commands.rs: change both together.

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

/** 64 hex characters. */
export type PeerId = string;
/** 64 hex characters. */
export type ContentHash = string;
/** Unix milliseconds. */
export type Millis = number;

export interface Settings {
  displayName: string;
  downloadDir: string | null;
}

/** Whether contacts can reach us over the internet (through our relay server). */
export type NetStatus = 'connecting' | 'online' | 'offline';

export interface Status {
  /** `null` when the node failed to start. */
  peerId: PeerId | null;
  netStatus: NetStatus | null;
  startupError: string | null;
  dataDir: string;
  settings: Settings;
  version: string;
}

export interface Contact {
  peer: PeerId;
  name: string;
  addedAt: Millis;
}

export type OfferState = 'new' | 'declined' | 'done' | 'revoked';

export interface Offer {
  from: PeerId;
  root: ContentHash;
  title: string;
  fileCount: number;
  totalSize: number;
  /** The first files: path in the share and size. */
  preview: [string, number][];
  expiresAt: Millis | null;
  receivedAt: Millis;
  state: OfferState;
}

export interface Share {
  root: ContentHash;
  title: string;
  recipients: PeerId[];
  fileCount: number;
  totalSize: number;
  revokedAt: Millis | null;
  expiresAt: Millis | null;
}

export interface PendingOffer {
  to: PeerId;
  root: ContentHash;
  /** `true`: news of a revoke; `false`: the offer itself. */
  revoke: boolean;
  attempts: number;
}

export type NodeEvent =
  | { type: 'contactAdded'; contact: Contact }
  | { type: 'offerReceived'; offer: Offer }
  | { type: 'offerRevoked'; from: PeerId; root: ContentHash }
  | { type: 'delivered'; to: PeerId; root: ContentHash; revoke: boolean }
  | { type: 'undeliverable'; to: PeerId; root: ContentHash; revoke: boolean }
  | { type: 'netStatus'; status: NetStatus }
  | { type: 'progress'; from: PeerId; root: ContentHash; bytes: number; total: number | null }
  | { type: 'warning'; context: string; message: string };

/** What a failed command rejects with. `kind` is the core error's name, e.g. `AlreadyExists`. */
export interface UiError {
  kind: string;
  message: string;
}

export const api = {
  status: () => invoke<Status>('status'),
  saveSettings: (settings: Settings) => invoke<void>('save_settings', { settings }),
  openLogFolder: () => invoke<void>('open_log_folder'),

  contacts: () => invoke<Contact[]>('contacts'),
  createInvite: () => invoke<string>('create_invite'),
  redeemInvite: (code: string) => invoke<Contact>('redeem_invite', { code }),
  removeContact: (peer: PeerId) => invoke<void>('remove_contact', { peer }),

  inbox: () => invoke<Offer[]>('inbox'),
  accept: (from: PeerId, root: ContentHash, dest: string) =>
    invoke<string[]>('accept', { from, root, dest }),
  decline: (from: PeerId, root: ContentHash) => invoke<void>('decline', { from, root }),
  /** Removes a finished offer from the inbox, and frees its unsaved data. */
  forgetOffer: (from: PeerId, root: ContentHash) => invoke<void>('forget_offer', { from, root }),

  shares: () => invoke<Share[]>('shares'),
  pendingOffers: () => invoke<PendingOffer[]>('pending_offers'),
  share: (path: string, recipients: PeerId[], expiresAt: Millis | null) =>
    invoke<ContentHash>('share', { path, recipients, expiresAt }),
  revoke: (root: ContentHash) => invoke<void>('revoke', { root }),
  /** Removes a share from the list (withdrawing it first if needed). */
  forgetShare: (root: ContentHash) => invoke<void>('forget_share', { root }),
  setExpiry: (root: ContentHash, expiresAt: Millis | null) =>
    invoke<void>('set_expiry', { root, expiresAt }),
};

export const onNodeEvent = (handler: (event: NodeEvent) => void): Promise<UnlistenFn> =>
  listen<NodeEvent>('node-event', (e) => handler(e.payload));

/** Fired when the app fell behind on events; reload everything. */
export const onResync = (handler: () => void): Promise<UnlistenFn> => listen('resync', handler);
