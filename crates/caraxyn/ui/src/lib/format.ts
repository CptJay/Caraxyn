// Turning raw values into text for people.

import type { Millis, UiError } from './api';

const UNITS = ['B', 'KB', 'MB', 'GB', 'TB'];

export function formatBytes(bytes: number): string {
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit++;
  }
  return `${unit === 0 ? value : value.toFixed(1)} ${UNITS[unit]}`;
}

export function formatDate(ms: Millis): string {
  return new Date(ms).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' });
}

/** "expires 3 Nov, 14:00", "expired", or "no expiry". */
export function formatExpiry(expiresAt: Millis | null, now = Date.now()): string {
  if (expiresAt === null) return 'no expiry';
  return expiresAt <= now ? 'expired' : `expires ${formatDate(expiresAt)}`;
}

/** First 8 characters, like the core's `short()`. */
export const shortId = (id: string): string => id.slice(0, 8);

/** Plain words for the errors people will actually meet; the core's message otherwise. */
const FRIENDLY: Record<string, string> = {
  AlreadyExists: 'Something with that name is already in the folder. Pick another folder.',
  Stalled: 'The sender could not be reached. Retry later; the download resumes where it stopped.',
  ShareRevoked: 'The sender withdrew this share.',
  ContactRejected: 'The invite was refused. It may be used already or expired (invites last 24 h).',
  NoDisplayName: 'Set your display name in Settings first.',
};

export function errorText(error: unknown): string {
  if (typeof error === 'object' && error !== null && 'kind' in error) {
    const { kind, message } = error as UiError;
    return FRIENDLY[kind] ?? message;
  }
  return String(error);
}
