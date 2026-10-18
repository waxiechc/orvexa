export interface Wallet { id: string; name: string; address: string; encrypted: string }
const KEY = 'orvexa.encrypted-wallets.v1';
export function readWallets(): Wallet[] {
  try {
    const raw = localStorage.getItem(KEY); if (!raw) return [];
    if (raw.length > 300000) throw new Error('Wallet storage exceeds its limit.');
    const entries: unknown = JSON.parse(raw);
    if (!Array.isArray(entries) || entries.length > 20) throw new Error('Invalid local wallet index.');
    return entries.map(w => {
      if (!w || typeof w.id !== 'string' || typeof w.name !== 'string' || w.name.length > 40 || typeof w.address !== 'string' || typeof w.encrypted !== 'string' || w.encrypted.length > 8192) throw new Error('Invalid wallet record.');
      return w as Wallet;
    });
  } catch {
    throw new Error('Local wallet storage could not be read. Keep this browser profile and restore an encrypted backup in another profile; do not clear storage.');
  }
}
export function saveWallets(wallets: Wallet[]) {
  if (wallets.length > 20) throw new Error('This local workspace supports up to 20 wallets.');
  try { localStorage.setItem(KEY, JSON.stringify(wallets)); }
  catch { throw new Error('Could not save the encrypted wallet. Free browser storage and try again.'); }
}
export function localCrypto<T = string>(operation: string, ...args: string[]): Promise<T> {
  return new Promise((resolve, reject) => {
    // A fresh isolated worker per operation: no unlocked key/session is retained.
    const worker = new Worker(new URL('./signer.worker.ts', import.meta.url), { type: 'module' });
    const timer = window.setTimeout(() => { worker.terminate(); reject(new Error('Local signing timed out. No wallet was sent to the node.')); }, 120000);
    const close = () => { window.clearTimeout(timer); worker.terminate(); };
    worker.onmessage = event => { close(); event.data.error ? reject(new Error(event.data.error)) : resolve(event.data.result as T); };
    worker.onerror = () => { close(); reject(new Error('The local Rust signer could not load. Rebuild the web app and reload this page.')); };
    worker.postMessage({ operation, args });
  });
}
export function downloadWallet(wallet: Wallet) {
  const blob = new Blob([wallet.encrypted], { type: 'application/json' });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement('a');
  anchor.href = url; anchor.download = `orvexa-${wallet.name.replace(/[^\p{L}\p{N}-]/gu, '-').slice(0, 30)}.wallet.json`;
  document.body.append(anchor); anchor.click(); anchor.remove();
  setTimeout(() => URL.revokeObjectURL(url), 10000);
}
