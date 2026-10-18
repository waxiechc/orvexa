// Client for the private v2 test network served by the ORVEXA app itself.
// Wallet routes need the per-launch session the native window injects; a
// normal browser tab does not have it and cannot reach private wallets.
declare global { interface Window { __ORVEXA_V2_SESSION?: string } }

export const session = () => typeof window.__ORVEXA_V2_SESSION === 'string' ? window.__ORVEXA_V2_SESSION : '';

export type Access = 'spend' | 'view_full' | 'view_incoming';
export interface NetworkView { exists: boolean; name: string | null; role: 'producer' | 'follower' | null; upstream: string | null; upstream_height: number | null; default_available: boolean; chain_id: string | null; height: number | null; remaining_supply: string | null; mempool: number }
export interface WalletSummary { id: string; name: string; address: string | null; unlocked: boolean; access: Access }
export interface State { network: NetworkView; wallets: WalletSummary[]; supply: string; default_fee: string; idle_lock_minutes: number }
export interface Activity { kind: 'received' | 'sent' | 'change'; value: string; height: number; txid: string; action: number }
export interface WalletView { id: string; name: string; access: Access; address: string; balance: string | null; pending: string; received: string; scanned_to: number; activity: Activity[] }
export interface Draft { draft: string; words: string[]; check: number[] }
export interface Verified { recipient: string; value: string; height: number; confirmations: number; txid: string }
export interface ChainStatus { chain_id: string; height: number; tip: string; burned_fees: number; remaining_supply: number; mempool: number }
export interface BlockView { height: number; id: string; prev: string; confirmations: number; txids: string[]; actions: number; fees: number }
export interface TxView { txid: string; height: number | null; confirmations: number; fee: number; expiry_height: number; actions: number; bytes: number; nullifiers: string[]; commitments: string[] }

export class V2Error extends Error { constructor(message: string, public status: number) { super(message); } }

async function call<T>(path: string, body?: unknown): Promise<T> {
  const headers: Record<string, string> = { 'X-Orvexa-Session': session() };
  if (body !== undefined) headers['Content-Type'] = 'application/json';
  const response = await fetch(path, {
    method: body === undefined ? 'GET' : 'POST', headers, body: body === undefined ? undefined : JSON.stringify(body),
    credentials: 'omit', redirect: 'error', cache: 'no-store', signal: AbortSignal.timeout(90000),
  });
  const text = await response.text();
  let data: unknown = null; try { data = JSON.parse(text); } catch { /* handled below */ }
  if (!response.ok) {
    const message = data && typeof data === 'object' && typeof (data as { error?: unknown }).error === 'string' ? (data as { error: string }).error
      : data && typeof data === 'object' && typeof (data as { code?: unknown }).code === 'string' ? (data as { code: string }).code : `Node returned HTTP ${response.status}.`;
    throw new V2Error(message, response.status);
  }
  return data as T;
}

const id = (value: string) => encodeURIComponent(value);
export const v2 = {
  state: () => call<State>('/v2/desktop/state'),
  wallet: (w: string) => call<WalletView>(`/v2/desktop/wallets/${id(w)}`),
  newWallet: (name: string) => call<Draft>('/v2/desktop/wallets/new', { name }),
  confirm: (draft: string, words: [number, string][], password: string) => call<{ id: string }>('/v2/desktop/wallets/confirm', { draft, words, password }),
  restore: (name: string, phrase: string, password: string) => call<{ id: string }>('/v2/desktop/wallets/restore', { name, phrase, password }),
  unlock: (w: string, password: string) => call<WalletView>(`/v2/desktop/wallets/${id(w)}/unlock`, { password }),
  lock: (w: string) => call<unknown>(`/v2/desktop/wallets/${id(w)}/lock`, {}),
  newAddress: (w: string) => call<{ address: string }>(`/v2/desktop/wallets/${id(w)}/address`, {}),
  send: (w: string, to: string, amount: string, fee: string, publicConsent?: { to: string; amount: string }) =>
    call<{ txid: string; receipt: string | null }>(`/v2/desktop/wallets/${id(w)}/send`, publicConsent ? { to, amount, fee, public: publicConsent } : { to, amount, fee }),
  receipt: (w: string, txid: string, action: number) => call<{ receipt: string }>(`/v2/desktop/wallets/${id(w)}/receipt`, { txid, action }),
  verify: (receipt: string) => call<Verified>('/v2/desktop/receipt/verify', { receipt }),
  phrase: (w: string, password: string) => call<{ words: string[] }>(`/v2/desktop/wallets/${id(w)}/phrase`, { password }),
  viewingKey: (w: string, full: boolean, password: string) => call<{ key: string; scope: string[] }>(`/v2/desktop/wallets/${id(w)}/viewing-key`, { full, password }),
  watch: (name: string, key: string) => call<{ id: string }>('/v2/desktop/watch', { name, key }),
  createNetwork: (wallet: string) => call<unknown>('/v2/desktop/network/create', { wallet }),
  joinNetwork: (profile?: string) => call<unknown>('/v2/desktop/network/join', profile === undefined ? {} : { profile }),
  profile: () => call<unknown>('/v2/desktop/network/profile'),
  chain: () => call<ChainStatus>('/v2/status'),
  blocks: () => call<BlockView[]>('/v2/blocks?limit=10'),
  block: (height: string) => call<BlockView>(`/v2/blocks/${id(height)}`),
  tx: (txid: string) => call<TxView>(`/v2/tx/${id(txid)}`),
};
