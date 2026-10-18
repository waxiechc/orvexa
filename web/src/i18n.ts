import { useSyncExternalStore } from 'react';
import ru from './locales/ru.json';

export type Locale = 'ru' | 'en';
const dictionary: Record<string, string> = ru;
const listeners = new Set<() => void>();
let locale: Locale = 'ru';
try { if (localStorage.getItem('orvexa.locale') === 'en') locale = 'en'; } catch { /* Language must work even when storage is unavailable. */ }
export const getLocale = () => locale;
export const numberLocale = () => locale === 'ru' ? 'ru-RU' : 'en-US';
export function setLocale(value: Locale) {
  locale = value;
  try { localStorage.setItem('orvexa.locale', value); } catch { /* Session-only preference. */ }
  document.documentElement.lang = value;
  listeners.forEach(listener => listener());
}
export function useLocale() {
  return useSyncExternalStore(listener => { listeners.add(listener); return () => { listeners.delete(listener); }; }, getLocale);
}
// Translate presentation only: protocol identifiers, paths, user names and keys
// never pass through a translation/rewrite step.
export function t(key: string, values: Record<string, string | number> = {}): string {
  const trimmed = key.trim();
  const result = locale === 'ru' ? dictionary[trimmed] ?? trimmed : trimmed;
  return (key.match(/^\s*/)?.[0] || '') + result.replace(/\{(\w+)\}/g, (match, name: string) =>
    Object.hasOwn(values, name) ? String(values[name]) : match) + (key.match(/\s*$/)?.[0] || '');
}
export function localizedError(message: string): string {
  const exact = t(message);
  if (exact !== message || locale === 'en') return exact;
  const http = message.match(/^Node returned HTTP (\d+)\.$/);
  if (http) return t('Node returned HTTP {status}.', { status: http[1] });
  const nonce = message.match(/^nonce mismatch: expected (\d+), received (\d+)$/);
  if (nonce) return t('Nonce mismatch: expected {expected}, received {actual}.', { expected: nonce[1], actual: nonce[2] });
  const duplicate = message.match(/^This wallet is already stored as (.*)\.$/);
  if (duplicate) return t('This wallet is already stored as {v0}.', { v0: duplicate[1] });
  const retry = ' Check the current height in Explorer before retrying.';
  if (message.endsWith(retry)) return t('{v0} Check the current height in Explorer before retrying.', { v0: localizedError(message.slice(0, -retry.length)) });
  if (/Failed to fetch|fetch failed|NetworkError|Load failed/i.test(message)) return t('Could not reach the local node. Keep ORVEXA open and check its status.');
  if (/timeout|timed out|aborted/i.test(message)) return t('The request timed out. Check the node status before trying again.');
  return message;
}
