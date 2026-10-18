import { parseNodeJson } from './lossless';
export { parseNodeJson };
export type UInt = string;
export interface Status { protocol: UInt; network: string; privacy: string; consensus: string; chain_id: string; height: UInt; tip: string; burned_fees: UInt; pending: UInt }
export interface Genesis { version: UInt; authority: string; recipient: string; supply: UInt; network_salt: string }
export interface Transfer { version: UInt; chain_id: string; sender: string; recipient: string; amount: UInt; fee: UInt; nonce: UInt }
export interface SignedTransfer { transaction: Transfer; signature: string }
export interface Balance { address: string; confirmed: { balance: UInt; nonce: UInt }; pending_nonce: UInt }
export interface History { height: UInt; id: string; transaction: SignedTransfer }
export interface Block { header: { version: UInt; chain_id: string; height: UInt; parent: string; timestamp: UInt; transactions_hash: string }; transactions: SignedTransfer[]; signature: string }
export interface BlockDetails { id: string; transaction_ids: string[]; confirmations: UInt; block: Block }
export interface TxDetails { id: string; status: 'pending' | 'confirmed'; confirmations: UInt; block_height: UInt | null; block_id: string | null; transaction: SignedTransfer }
export interface UiState { status: Status; genesis: Genesis; blocks: BlockDetails[]; pending: TxDetails[]; balance: Balance | null; history: History[] }

export class NodeError extends Error { constructor(message: string, public status = 0) { super(message); } }
export async function requestRaw(path: string, body?: string): Promise<string> {
  const response = await fetch(path, { method: body === undefined ? 'GET' : 'POST', body,
    headers: body === undefined ? undefined : { 'Content-Type': 'application/json' },
    credentials: 'omit', redirect: 'error', cache: 'no-store', signal: AbortSignal.timeout(10000) });
  const reader = response.body?.getReader();
  const chunks: Uint8Array[] = []; let size = 0;
  if (!reader) throw new NodeError('Empty node response.');
  while (true) {
    const { value, done } = await reader.read(); if (done) break;
    size += value.byteLength;
    if (size > 2 * 1024 * 1024) { await reader.cancel(); throw new NodeError('Node response exceeds the size limit.'); }
    chunks.push(value);
  }
  const bytes = new Uint8Array(size); let offset = 0;
  for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.length; }
  const text = new TextDecoder().decode(bytes);
  if (!response.ok) {
    let message = `Node returned HTTP ${response.status}.`;
    try { const data = JSON.parse(text); if (typeof data.error === 'string') message = data.error; } catch { /* No request material is logged. */ }
    throw new NodeError(message, response.status);
  }
  return text;
}
async function get<T>(path: string) { return parseNodeJson<T>(await requestRaw(path)); }

/** Replace this entire adapter for a future private ledger. Address lookup is a
 * transparent-v1 capability; it must not leak into a shielded backend contract. */
export interface LedgerReader {
  readonly privacy: 'transparent';
  status(): Promise<Status>;
  recentBlocks(before?: string): Promise<BlockDetails[]>;
  transaction(id: string): Promise<TxDetails>;
}
export class TransparentV1Adapter implements LedgerReader {
  readonly privacy = 'transparent' as const;
  status = () => get<Status>('/v1/status');
  snapshot = async (address?: string) => {
    const raw = await requestRaw(`/v1/ui-state${address ? `?address=${encodeURIComponent(address)}` : ''}`);
    return { raw, data: parseNodeJson<UiState>(raw) };
  };
  genesis = () => get<Genesis>('/v1/genesis');
  balance = (address: string) => get<Balance>(`/v1/balance/${encodeURIComponent(address)}`);
  history = (address: string) => get<History[]>(`/v1/history/${encodeURIComponent(address)}`);
  mempool = () => get<SignedTransfer[]>('/v1/mempool');
  pending = () => get<TxDetails[]>('/v1/explorer/mempool');
  recentBlocks = (before?: string) => get<BlockDetails[]>(`/v1/explorer/blocks${before ? `?before=${before}` : ''}`);
  block = (height: string) => get<BlockDetails>(`/v1/blocks/${height}`);
  transaction = (id: string) => get<TxDetails>(`/v1/transactions/${id}`);
  submit = async (signed: string) => parseNodeJson<{ id: string }>(await requestRaw('/v1/transactions', signed));
  commit = async (signed: string) => parseNodeJson<{ id: string }>(await requestRaw('/v1/blocks', signed));
}
export const node = new TransparentV1Adapter();
export const atoms = (value: string | bigint) => BigInt(value).toLocaleString('en-US');
export const short = (value: string, length = 7) => value.length > length * 2 + 3 ? `${value.slice(0, length)}…${value.slice(-length)}` : value;
export const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);
export function availableBalance(confirmed: string, address: string, pool: SignedTransfer[]): bigint {
  return pool.reduce((balance, signed) => {
    const t = signed.transaction;
    return balance - (t.sender === address ? BigInt(t.amount) + BigInt(t.fee) : 0n) + (t.recipient === address ? BigInt(t.amount) : 0n);
  }, BigInt(confirmed));
}
