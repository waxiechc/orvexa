import { useEffect, useState } from 'react';
import { Download, RefreshCw, RotateCw } from 'lucide-react';
import { Button, Notice } from './App';
import { getLocale, t } from './i18n';
import { session } from './v2';

interface Available { release: string; notes_ru: string; notes_en: string; bytes: number }
interface UpdateState { current: string | null; platform: string; automatic: boolean; checking: boolean; checked_at: number | null; error: string | null; available: Available | null; ready: boolean; can_install: boolean }

async function call<T>(path: string, body?: unknown): Promise<T> {
  const response = await fetch(path, { method: body === undefined ? 'GET' : 'POST', headers: { 'X-Orvexa-Session': session(), ...(body === undefined ? {} : { 'Content-Type': 'application/json' }) },
    body: body === undefined ? undefined : JSON.stringify(body), credentials: 'omit', cache: 'no-store', signal: AbortSignal.timeout(120000) });
  const data = await response.json().catch(() => null);
  if (!response.ok) throw new Error(data?.error || `HTTP ${response.status}`);
  return data as T;
}

function useUpdates() {
  const [state, setState] = useState<UpdateState>();
  useEffect(() => {
    if (!session()) return; let on = true;
    const load = () => call<UpdateState>('/desktop/update/state').then(s => { if (on) setState(s); }).catch(() => undefined);
    void load(); const id = setInterval(load, 60000); return () => { on = false; clearInterval(id); };
  }, []);
  return [state, setState] as const;
}

const notes = (a: Available) => getLocale() === 'ru' ? a.notes_ru : a.notes_en;

async function install(setError: (e: string) => void, setBusy: (b: boolean) => void) {
  setBusy(true); setError('');
  try { await call('/desktop/update/install', {}); } catch (e) { setError(e instanceof Error ? e.message : String(e)); setBusy(false); }
}

const DISMISSED = 'orvexa.update.dismissed';
const dismissed = () => { try { return localStorage.getItem(DISMISSED) || ''; } catch { return ''; } };

/** Notification in the corner of every page once a verified update is ready. */
export function UpdateToast() {
  const [state] = useUpdates(); const [busy, setBusy] = useState(false); const [error, setError] = useState('');
  const [hidden, setHidden] = useState(dismissed);
  if (!state?.available || !state.ready || hidden === state.available.release) return null;
  const later = () => { const r = state.available!.release; try { localStorage.setItem(DISMISSED, r); } catch { /* session only */ } setHidden(r); };
  return <aside className="update-toast" role="status" aria-live="polite">
    <span className="receipt-icon"><Download size={18}/></span>
    <div className="update-toast-body"><strong>{t("New version {v0}", { v0: state.available.release })}</strong>
      <span>{notes(state.available)}</span>
      {!state.can_install && <small>{t("Install it from the ORVEXA site: this copy cannot replace itself.")}</small>}
      {error && <span className="update-error">{error}</span>}
      <div className="button-row">
        {state.can_install && <Button disabled={busy} onClick={() => void install(setError, setBusy)}>{busy ? <><RefreshCw className="spinning" size={16}/>{t("Restarting…")}</> : <><RotateCw size={16}/>{t("Restart and update")}</>}</Button>}
        <Button kind="ghost" disabled={busy} onClick={later}>{t("Later")}</Button>
      </div>
    </div>
  </aside>;
}

/** Settings section: version, automatic download, manual check. */
export function UpdateSettings() {
  const [state, setState] = useUpdates(); const [busy, setBusy] = useState(false); const [error, setError] = useState('');
  if (!session()) return null;
  const check = async () => { setBusy(true); setError(''); try { setState(await call<UpdateState>('/desktop/update/check', {})); } catch (e) { setError(e instanceof Error ? e.message : String(e)); } finally { setBusy(false); } };
  const toggle = async (automatic: boolean) => { try { setState(await call<UpdateState>('/desktop/update/settings', { automatic })); } catch (e) { setError(e instanceof Error ? e.message : String(e)); } };
  return <section className="surface-section"><div className="section-heading"><div><h2>{t("Updates")}</h2>
    <p>{t("ORVEXA checks for new versions every few hours. Only builds signed with the ORVEXA release key are accepted; the app restarts only when you press the button.")}</p></div></div>
    <dl className="network-facts">
      <div><dt>{t("This version")}</dt><dd>{state?.current || t("development build")}</dd></div>
      <div><dt>{t("Latest available")}</dt><dd>{state?.available ? state.available.release : state?.checked_at ? t("You have the latest version") : '—'}</dd></div>
      <div><dt>{t("Last check")}</dt><dd>{state?.checked_at ? new Date(state.checked_at * 1000).toLocaleString(getLocale() === 'ru' ? 'ru-RU' : 'en-GB') : '—'}</dd></div>
    </dl>
    <label className="backup-check switch-row"><input type="checkbox" role="switch" checked={!!state?.automatic} onChange={e => void toggle(e.target.checked)}/><span><strong>{t("Automatic updates")}</strong><small>{state?.automatic ? t("ORVEXA checks and downloads new versions itself and tells you when one is ready.") : t("ORVEXA does not contact the update server on its own. Use Check now.")}</small></span></label>
    {(error || state?.error) && <Notice error>{error || state!.error!}</Notice>}
    {!state?.current && <p className="caption">{t("Development builds are not updated automatically. Install releases from the ORVEXA site.")}</p>}
    <div className="button-row">
      <Button kind="secondary" disabled={busy || state?.checking} onClick={() => void check()}>{busy || state?.checking ? <><RefreshCw className="spinning" size={16}/>{t("Checking…")}</> : <><RefreshCw size={16}/>{t("Check now")}</>}</Button>
      {state?.can_install && <Button disabled={busy} onClick={() => void install(setError, setBusy)}><RotateCw size={16}/>{t("Restart and update")}</Button>}
    </div>
  </section>;
}
