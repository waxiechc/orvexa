import FileInput from './FileInput';
import AddressMark from './AddressMark';
import { t, useLocale, setLocale, getLocale, localizedError } from './i18n';
import { Children, useCallback, useEffect, useRef, useState, type FormEvent, type ReactNode } from 'react';
import { Activity, ArrowDownLeft, ArrowLeft, ArrowRight, ArrowUpRight, Blocks, Check, CheckCheck, ChevronDown, ChevronRight, CircleAlert, Copy, Download, FileKey2, Globe2, History as HistoryIcon, Home, KeyRound, LockKeyhole, Menu, Plus, RefreshCw, Search, Settings2, ShieldCheck, Upload, Wallet as WalletIcon, X } from 'lucide-react';
import QRCode from 'qrcode';
import NodePanel from './NodePanel';
import Dropdown from './Dropdown';
import PrivatePanel from './PrivatePanel';
import { UpdateToast, UpdateSettings } from './Updates';
import { atoms, availableBalance, errorText, node, NodeError, parseNodeJson, requestRaw, short, type Balance, type BlockDetails, type Genesis, type History, type Status, type TxDetails } from './adapter';
import { downloadWallet, localCrypto, readWallets, saveWallets, type Wallet } from './vault';

type Snapshot = { status: Status; genesis: Genesis; blocks: BlockDetails[]; pending: TxDetails[]; balance?: Balance; history: History[]; checked: Date; walletId?: string };
type WalletAction = 'create' | 'restore' | null;
/** Local height behind the highest authority-signed head a connected peer advertised. */
type Sync = { local: string; target: string };
type Runtime = { peers?: { height: string | null }[] };
async function readSync(local: string): Promise<Sync | undefined> {
  // Older or externally launched nodes may not expose peers; that means "unknown", not "synced".
  const runtime = parseNodeJson<Runtime>(await requestRaw('/v1/runtime'));
  const target = (runtime.peers || []).reduce((max, p) => p.height !== null && BigInt(p.height) > max ? BigInt(p.height) : max, BigInt(local));
  return target > BigInt(local) ? { local, target: target.toString() } : undefined;
}
// Grouped by what people do: move funds, watch the network, look after keys.
const navGroups = [
  [['dashboard', 'Dashboard', Home], ['send', 'Send', ArrowUpRight], ['receive', 'Receive', ArrowDownLeft], ['transactions', 'Transactions', HistoryIcon]],
  [['explorer', 'Explorer', Globe2], ['node', 'Node', Activity]],
  [['wallets', 'Wallets', WalletIcon], ['backup', 'Backup & Restore', FileKey2], ['settings', 'Settings', Settings2]],
  [['private', 'Private network v2', LockKeyhole]],
] as const;
const routes = navGroups.flat();
const verifiedNetworks = new Set<string>();
const go = (path: string) => { window.location.hash = path; };
const readRoute = () => window.location.hash.slice(2) || 'dashboard';
const isAuthority = (wallet: Wallet | undefined, genesis: Genesis | undefined) => !!wallet && wallet.address === genesis?.authority;
const timeLocale = () => getLocale() === 'ru' ? 'ru-RU' : 'en-GB';
// Counted nouns need real plural forms in Russian (1 перевод, 2 перевода, 5 переводов).
function count(value: number | string, en: [string, string], ru: [string, string, string]) {
  const n = BigInt(value); const shown = atoms(n);
  if (getLocale() !== 'ru') return `${shown} ${n === 1n ? en[0] : en[1]}`;
  const m10 = Number(n % 10n), m100 = Number(n % 100n);
  return `${shown} ${m10 === 1 && m100 !== 11 ? ru[0] : m10 >= 2 && m10 <= 4 && (m100 < 12 || m100 > 14) ? ru[1] : ru[2]}`;
}

export function Button({ children, kind = 'primary', className = '', ...props }: React.ButtonHTMLAttributes<HTMLButtonElement> & { kind?: 'primary' | 'secondary' | 'ghost'; children: ReactNode }) {
  return <button className={`button ${kind} ${className}`} {...props}>{children}</button>;
}
export function Badge({ children, pending = false }: { children: ReactNode; pending?: boolean }) {
  return <span className={`badge ${pending ? 'pending' : ''}`}>{pending ? <span className="badge-pulse" aria-hidden="true"/> : <Check size={12}/>} {children}</span>;
}
export function Notice({ children, error = false }: { children: ReactNode; error?: boolean }) {
  return <div className={`notice ${error ? 'error' : ''}`} role={error ? 'alert' : undefined}><CircleAlert size={18}/><div>{Children.map(children, child => typeof child === 'string' ? localizedError(child) : child)}</div></div>;
}
export function Empty({ title, children, action }: { title: string; children: ReactNode; action?: ReactNode }) {
  return <div className="empty"><img className="empty-mark" src="/brand/orvexa-symbol.svg" alt="" aria-hidden="true"/><h3>{title}</h3><p>{children}</p>{action}</div>;
}
export function CopyButton({ value, label = 'Copy' }: { value: string; label?: string }) {
  const [copied, setCopied] = useState(false); const [failed, setFailed] = useState(false);
  return <><button className="copy-button" type="button" aria-label={t(label)} onClick={async () => {
    try { await navigator.clipboard.writeText(value); setCopied(true); setFailed(false); setTimeout(() => setCopied(false), 2000); }
    catch { setFailed(true); }
  }}>{copied ? <Check size={15}/> : <Copy size={15}/>}<span>{copied ? t("Copied") : t(label)}</span></button>{failed && <small role="alert">{t("Select and copy the text manually.")}</small>}</>;
}
export function Hash({ value, label }: { value: string; label?: string }) {
  return <div className="hash-value"><code title={value}>{value}</code><CopyButton value={value} label={label || t("Copy")}/></div>;
}
export function Modal({ title, children, close, busy = false }: { title: string; children: ReactNode; close: () => void; busy?: boolean }) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => { ref.current?.showModal(); return () => ref.current?.close(); }, []);
  return <dialog ref={ref} className="dialog" onCancel={e => { e.preventDefault(); if (!busy) close(); }}>
    <div className="dialog-heading"><h2>{title}</h2><button className="icon-button" aria-label={t("Close dialog")} disabled={busy} onClick={close}><X size={20}/></button></div>{children}
  </dialog>;
}
function WalletPicker({ wallets, active, select }: { wallets: Wallet[]; active?: Wallet; select: (id: string) => void }) {
  return <Dropdown className="wallet-dropdown" align="end" label={t("Active wallet")} value={active?.id || ''} placeholder={t("Select wallet")} onChange={select}
    leading={active ? <AddressMark address={active.address} size={26}/> : <span className="wallet-picker-empty"><WalletIcon size={15}/></span>}
    options={wallets.map(w => ({ value: w.id, label: w.name, hint: short(w.address.replace(/^void_test_/, ''), 5), icon: <AddressMark address={w.address} size={24}/> }))}/>;
}
function WalletDialog({ mode, add, close }: { mode: 'create' | 'restore'; add: (w: Wallet) => void; close: () => void }) {
  const [name, setName] = useState(''); const [password, setPassword] = useState(''); const [repeat, setRepeat] = useState('');
  const [file, setFile] = useState<File>(); const [busy, setBusy] = useState(false); const [error, setError] = useState('');
  const [created, setCreated] = useState<Wallet>(); const [saved, setSaved] = useState(false);
  const submit = async (e: FormEvent) => {
    e.preventDefault(); setError(''); setBusy(true);
    try {
      if (!name.trim() || name.trim().length > 40) throw new Error('Enter a wallet name of up to 40 characters.');
      if (mode === 'create' && password !== repeat) throw new Error('Passwords do not match.');
      let encrypted: string;
      if (mode === 'restore') {
        if (!file || file.size > 8192) throw new Error('Choose a v1 encrypted wallet JSON file, up to 8 KiB.');
        encrypted = await file.text();
      } else encrypted = await localCrypto('create_wallet', password);
      const address = await localCrypto<string>('authenticate_wallet', encrypted, password);
      const wallet = { id: crypto.randomUUID(), name: name.trim(), address, encrypted };
      add(wallet); setCreated(wallet);
    } catch (e) { setError(errorText(e)); }
    finally { setBusy(false); setPassword(''); setRepeat(''); }
  };
  return <Modal title={created ? t("Your wallet is ready") : mode === 'create' ? t("Create a wallet") : t("Restore a wallet")} close={close} busy={busy}>
    {created ? <div className="dialog-body"><div className="created-wallet"><AddressMark address={created.address} size={52}/><h3>{created.name}</h3></div><p>{t("The encrypted wallet is saved in this browser. Download a backup before you continue.")}</p><Hash value={created.address}/>
      <Button onClick={() => { downloadWallet(created); setSaved(true); }}><Download size={17}/>{t("Download encrypted backup")}</Button>
      <p className="caption">{t("The backup uses the same v1 format as the Rust CLI. Keep your password separately. There is no seed phrase in v1.")}</p>
      <Button kind="secondary" disabled={!saved} onClick={close}>{t("Continue to wallet")}<ArrowRight size={17}/></Button></div>
      : <form className="dialog-body" onSubmit={submit}>
        <p>{mode === 'create' ? t("Keys are generated and encrypted locally by the Rust WebAssembly wallet.") : t("Import an encrypted backup from ORVEXA CLI or this browser. Nothing is uploaded to the node.")}</p>
        <label>{t("Wallet name")}<input autoFocus required maxLength={40} value={name} onChange={e => setName(e.target.value)} placeholder={t("e.g. Personal wallet")}/></label>
        {mode === 'restore' && <label>{t("Encrypted wallet file")}<FileInput aria-label={t("Encrypted wallet file")} required type="file" accept=".json,application/json" onChange={e => setFile(e.target.files?.[0])}/></label>}
        <label>{mode === 'create' ? t("New password") : t("Backup password")}<input required type="password" autoComplete={mode === 'create' ? 'new-password' : 'current-password'} value={password} onChange={e => setPassword(e.target.value)} placeholder={mode === 'create' ? t("At least 12 characters") : t("Enter your backup password")}/></label>
        {mode === 'create' && <label>{t("Repeat password")}<input required type="password" autoComplete="new-password" value={repeat} onChange={e => setRepeat(e.target.value)}/></label>}
        {error && <Notice error>{error}</Notice>}
        <Button disabled={busy} type="submit">{busy ? <><RefreshCw className="spinning" size={17}/>{t("Processing locally…")}</> : mode === 'create' ? <><Plus size={17}/>{t("Create wallet")}</> : <><Upload size={17}/>{t("Restore wallet")}</>}</Button>
        <div className="local-note"><LockKeyhole size={14}/>{t("Passwords and keys stay in this browser.")}</div>
      </form>}
  </Modal>;
}

function ActivityList({ entries, wallet, limit }: { entries: TxDetails[]; wallet?: Wallet; limit?: number }) {
  const visible = limit ? entries.slice(0, limit) : entries;
  if (!visible.length) return <Empty title={t("No transfers yet")}>{t("Confirmed transfers and pending submissions for this wallet will appear here. The genesis allocation is shown separately.")}</Empty>;
  return <div className="activity-list">
    {visible.map(entry => {
      const tx = entry.transaction.transaction; const self = tx.sender === tx.recipient;
      const incoming = tx.recipient === wallet?.address && !self;
      const counterparty = wallet ? (incoming ? tx.sender : tx.recipient) : tx.sender;
      return <button className={`activity-row ${entry.status === 'pending' ? 'is-pending' : ''}`} key={entry.id} title={entry.id} onClick={() => go(`/explorer/tx/${entry.id}`)}>
        <span className="tx-mark"><AddressMark address={counterparty} size={40}/><span className={`tx-direction ${incoming ? 'incoming' : ''}`}>{incoming ? <ArrowDownLeft size={12}/> : <ArrowUpRight size={12}/>}</span></span>
        <div className="tx-summary"><strong>{self ? t("Self transfer") : incoming ? t("Received") : wallet ? t("Sent") : t("Transfer")}</strong>
          <span>{wallet ? <>{incoming ? t("From") : t("To")} <code>{short(counterparty.replace(/^void_test_/, ''), 5)}</code></> : <><code>{short(tx.sender.replace(/^void_test_/, ''), 4)}</code><ArrowRight size={11}/><code>{short(tx.recipient.replace(/^void_test_/, ''), 4)}</code></>}</span></div>
        <div className="tx-status"><Badge pending={entry.status === 'pending'}>{entry.status === 'pending' ? t("Pending") : t("Confirmed")}</Badge><small>{entry.block_height ? t("Block #{v0}", { v0: entry.block_height }) : t("Awaiting authority block")}</small></div>
        <div className={`tx-amount ${incoming ? 'incoming' : ''}`}><strong>{self ? '' : wallet ? incoming ? '+' : '−' : ''}{atoms(tx.amount)}<span>{t(" atoms")}</span></strong><small>{incoming ? t("Sender fee") : t("Fee")} {atoms(tx.fee)}</small></div><ChevronRight className="row-chevron" size={16}/>
      </button>;
    })}</div>;
}

function Dashboard({ snapshot, wallet, entries, online, sync, create, restore, produce }: { snapshot?: Snapshot; wallet?: Wallet; entries: TxDetails[]; online: boolean; sync?: Sync; create: () => void; restore: () => void; produce: () => void }) {
  const balance = snapshot?.balance;
  const available = balance && wallet ? availableBalance(balance.confirmed.balance, wallet.address, snapshot!.pending.map(t => t.transaction)) : undefined;
  const hasPending = available !== undefined && balance && available !== BigInt(balance.confirmed.balance);
  return <>
    <div className="home-grid">
      <section className="hero-card" aria-label={wallet ? t("{v0} · Confirmed balance", { v0: wallet.name }) : t("Your local wallet")}>
        <img className="hero-watermark" src="/brand/orvexa-symbol.svg" alt="" aria-hidden="true"/>
        {wallet ? <>
          <div className="hero-top"><AddressMark address={wallet.address} size={40}/><div className="hero-wallet"><strong>{wallet.name}</strong><span><code>{short(wallet.address.replace(/^void_test_/, ''), 6)}</code><CopyButton value={wallet.address} label="Copy address"/></span></div><span className="hero-tag"><ShieldCheck size={14}/>{t("Transparent v1")}</span></div>
          <div className="hero-figure"><span className="hero-label">{t("Confirmed balance")}</span>
            <div className="balance-number" data-testid="confirmed-balance">{balance ? atoms(balance.confirmed.balance) : '—'}<span>{t("atoms")}</span></div>
            <p className="balance-subline">{sync ? <span className="sync-line"><RefreshCw size={13} className="spinning"/>{t("Syncing: block #{v0} of #{v1}. The balance may be incomplete.", { v0: atoms(sync.local), v1: atoms(sync.target) })}</span> : balance ? hasPending ? t("{v0} available after pending", { v0: atoms(available!) }) : t("Test atoms. No monetary value.") : t("Waiting for the connected node")}</p></div>
          <div className="quick-actions">
            <button className="quick-action primary" onClick={() => go('/send')} disabled={!online || !!sync}><span><ArrowUpRight size={20}/></span>{t("Send")}</button>
            <button className="quick-action" onClick={() => go('/receive')}><span><ArrowDownLeft size={20}/></span>{t("Receive")}</button>
            <button className="quick-action" onClick={() => go('/backup')}><span><FileKey2 size={19}/></span>{t("Backup")}</button>
            <button className="quick-action" onClick={produce} disabled={!online || !snapshot?.pending.length}><span><Blocks size={19}/></span>{t("Produce block")}</button>
          </div></>
          : <div className="hero-welcome"><h2>{t("Create your first wallet")}</h2><p>{t("Create a local wallet or bring an encrypted backup. Your balances come directly from the ORVEXA node.")}</p><div className="button-row"><Button onClick={create}><Plus size={17}/>{t("Create wallet")}</Button><Button kind="secondary" onClick={restore}><Upload size={17}/>{t("Restore")}</Button></div></div>}
      </section>
      <section className="network-card" aria-labelledby="network-title">
        <div className="card-heading"><h2 id="network-title">{t("Network")}</h2><span className={`state-pill ${online ? 'online' : ''}`}><span className="connection-dot"/>{online ? t("Online") : t("Offline")}</span></div>
        <div className="network-height"><span>{t("Block height")}</span><strong>{snapshot ? `#${atoms(snapshot.status.height)}` : '—'}</strong></div>
        <dl className="network-facts"><div><dt>{t("Pending transfers")}</dt><dd>{snapshot ? atoms(snapshot.status.pending) : '—'}</dd></div><div><dt>{t("Burned fees")}</dt><dd>{snapshot ? t("{v0} atoms", { v0: atoms(snapshot.status.burned_fees) }) : '—'}</dd></div><div><dt>{t("Node")}</dt><dd>{window.location.host}</dd></div></dl>
        <button className="card-link" onClick={() => go('/explorer')}>{t("Open Explorer")}<ArrowUpRight size={16}/></button>
      </section>
    </div>
    <section className="surface-section"><div className="section-heading"><h2>{t("Recent activity")}</h2>{wallet && entries.length > 0 && <button className="text-link" onClick={() => go('/transactions')}>{t("View all")}<ArrowRight size={15}/></button>}</div>
      {!wallet ? <Empty title={t("Choose a wallet to see activity")}>{t("Your local wallet history will appear here once a wallet is selected.")}</Empty> : <ActivityList entries={entries} wallet={wallet} limit={5}/>}
    </section>
  </>;
}

function Send({ wallet, snapshot, online, sync, sent, openWallets }: { wallet?: Wallet; snapshot?: Snapshot; online: boolean; sync?: Sync; sent: (id: string) => void; openWallets: () => void }) {
  const [recipient, setRecipient] = useState(''); const [amount, setAmount] = useState(''); const [fee, setFee] = useState('1');
  const [review, setReview] = useState(false); const [password, setPassword] = useState(''); const [busy, setBusy] = useState(false); const [error, setError] = useState(''); const [uncertain, setUncertain] = useState('');
  if (!wallet) return <Empty title={t("A wallet is needed to send")} action={<Button onClick={openWallets}>{t("Manage wallets")}</Button>}>{t("Create or restore an encrypted wallet first.")}</Empty>;
  const available = snapshot?.balance ? availableBalance(snapshot.balance.confirmed.balance, wallet.address, snapshot.pending.map(t => t.transaction)) : undefined;
  const check = async (e: FormEvent) => {
    e.preventDefault(); setError(''); setBusy(true);
    try {
      if (!/^[1-9]\d*$/.test(amount) || !/^[1-9]\d*$/.test(fee)) throw new Error('Amount and fee must be positive whole test atoms.');
      if (BigInt(amount) > 1000000n || BigInt(fee) > 1000000n) throw new Error('Amount or fee exceeds the entire test supply.');
      if (available === undefined || BigInt(amount) + BigInt(fee) > available) throw new Error('Insufficient available balance, including the fee.');
      if (!await localCrypto<boolean>('validate_address', recipient.trim())) throw new Error('Invalid v1 address or checksum. Check the entire receiving address.');
      setRecipient(recipient.trim()); setReview(true);
    } catch (e) { setError(errorText(e)); } finally { setBusy(false); }
  };
  const confirm = async (e: FormEvent) => {
    e.preventDefault(); setBusy(true); setError(''); let preparedId = '';
    try {
      const status = await node.status();
      if (!snapshot || status.chain_id !== snapshot.status.chain_id || status.chain_id !== localStorage.getItem('orvexa.chain')) throw new Error('The connected network changed. Review Settings before signing.');
      const balance = await node.balance(wallet.address);
      const result = await localCrypto<string>('sign_transfer', wallet.encrypted, password, JSON.stringify({ chain_id: status.chain_id, recipient, amount, fee, nonce: balance.pending_nonce }));
      const prepared: { id: string; signed_json: string } = JSON.parse(result); preparedId = prepared.id;
      const accepted = await node.submit(prepared.signed_json);
      if (accepted.id !== prepared.id) throw new Error('Node returned an inconsistent transaction ID.');
      sent(accepted.id);
    } catch (e) {
      if (preparedId && (!(e instanceof NodeError) || e.status === 0 || e.status >= 500)) setUncertain(preparedId);
      setError(errorText(e));
    } finally { setBusy(false); setPassword(''); }
  };
  const total = review ? recipient === wallet.address ? BigInt(fee) : BigInt(amount) + BigInt(fee) : 0n;
  return <div className="flow"><section className="flow-card">
    <div className="flow-heading"><h2>{review ? t("Review your transfer") : t("Send test atoms")}</h2><ol className="steps" aria-label={t("Step {v0} of 2", { v0: review ? 2 : 1 })}><li className="done"/><li className={review ? 'done' : ''}/></ol></div>
    <div className="from-wallet"><AddressMark address={wallet.address} size={40}/><div><strong>{wallet.name}</strong><small>{available === undefined ? t("Balance unavailable") : t("{v0} atoms available", { v0: atoms(available) })}</small></div>{!review && available !== undefined && available > 1n && <button type="button" className="chip-button" onClick={() => { const max = available - BigInt(/^[1-9]\d*$/.test(fee) ? fee : '1'); if (max > 0n) setAmount(max.toString()); }}>{t("Max")}</button>}</div>
    {!review ? <form onSubmit={check} className="stack-form">
      <label className="amount-field">{t("Amount")}<div className="amount-input"><input required inputMode="numeric" value={amount} onChange={e => setAmount(e.target.value.replace(/[\s ]/g, ''))} placeholder="0" size={Math.max(2, amount.length + 1)}/><span>{t("atoms")}</span></div></label>
      <label>{t("Recipient address")}<input required spellCheck={false} autoComplete="off" value={recipient} onChange={e => setRecipient(e.target.value)} placeholder="void_test_…"/><small>{t("Use a receiving address on this exact local network.")}</small></label>
      <label>{t("Network fee")}<div className="input-unit"><input required inputMode="numeric" value={fee} onChange={e => setFee(e.target.value)}/><span>{t("atoms")}</span></div><small>{t("Minimum 1 atom. Fees are burned; there is no service fee.")}</small></label>
      {sync && <Notice>{t("Syncing: block #{v0} of #{v1}. The balance may be incomplete.", { v0: atoms(sync.local), v1: atoms(sync.target) })}{t(" Sending is available after sync.")}</Notice>}
      {error && <Notice error>{error}</Notice>}<Button type="submit" className="wide" disabled={!online || busy || !!sync}>{busy ? t("Checking…") : t("Review transfer")}<ArrowRight size={17}/></Button></form>
      : <form onSubmit={confirm} className="stack-form">
        <div className="review-amount"><span>{recipient === wallet.address ? t("Self transfer") : t("You send")}</span><strong>{atoms(amount)}<small>{t("atoms")}</small></strong></div>
        <dl className="review-list"><div className="review-recipient"><dt>{t("Recipient")}</dt><dd><AddressMark address={recipient} size={28}/><code>{recipient}</code></dd></div><div><dt>{t("Network fee · burned")}</dt><dd>{atoms(fee)}{t(" atoms")}</dd></div><div className="total"><dt>{recipient === wallet.address ? t("Net debit · self transfer") : t("Total debit")}</dt><dd>{atoms(total)}{t(" atoms")}</dd></div></dl>
        {recipient === wallet.address && <p className="caption">{t("This returns the amount to the same address. Only the fee is burned; amount plus fee must still fit the available balance.")}</p>}
        <p className="caption">{t("Acceptance puts this transfer in the volatile mempool. It is confirmed only when the configured authority produces a block.")}</p>
        {!uncertain && <label>{t("Wallet password")}<input type="password" required autoComplete="current-password" value={password} onChange={e => setPassword(e.target.value)}/></label>}
        {error && <Notice error>{error}</Notice>}
        {uncertain ? <Notice>{t("Submission could not be confirmed. Check this exact ID before sending again: ")}<a href={`#/explorer/tx/${uncertain}`}>{short(uncertain, 16)}</a></Notice> : <Button type="submit" className="wide" disabled={!online || busy || !!sync}>{busy ? <><RefreshCw className="spinning" size={17}/>{t("Signing locally…")}</> : <><ArrowUpRight size={17}/>{t("Sign & send")}</>}</Button>}
        <Button kind="ghost" type="button" disabled={busy} onClick={() => { setReview(false); setUncertain(''); setError(''); }}><ArrowLeft size={15}/>{t("Back to transfer")}</Button>
      </form>}
    <div className="flow-foot"><LockKeyhole size={15}/><p>{t("Your browser signs with the existing Rust wallet. The node receives a signed transaction, never your password or private key.")}</p></div>
  </section><p className="flow-note">{t("Addresses, amounts and sender links are public. These are test atoms with no monetary value.")}</p></div>;
}

function Receive({ wallet, snapshot, openWallets }: { wallet?: Wallet; snapshot?: Snapshot; openWallets: () => void }) {
  const [qr, setQr] = useState('');
  useEffect(() => { setQr(''); let active = true; if (wallet) QRCode.toDataURL(wallet.address, { width: 512, margin: 2, errorCorrectionLevel: 'M', color: { dark: '#050608', light: '#E6F2FF' } }).then(value => { if (active) setQr(value); }); return () => { active = false; }; }, [wallet?.address]);
  if (!wallet) return <Empty title={t("Choose a receiving wallet")} action={<Button onClick={openWallets}>{t("Manage wallets")}</Button>}>{t("Create or restore a wallet to get its receiving address.")}</Empty>;
  return <div className="flow"><section className="flow-card receive-card">
    <div className="receive-wallet"><AddressMark address={wallet.address} size={40}/><div><h2>{t("Receive test atoms")}</h2><p>{t("Send to ")}<strong>{wallet.name}</strong>{t(" on the connected local network.")}</p></div></div>
    <div className="qr-frame">{qr ? <img src={qr} alt={t("QR code for {v0} receiving address", { v0: wallet.name })} width={256} height={256}/> : <span>{t("Generating address QR…")}</span>}</div>
    <span className="field-label">{t("Your receiving address")}</span><Hash value={wallet.address} label="Copy address"/>
    <p className="caption">{t("The QR contains the raw v1 address. Verify the chain ID with the sender; it is not a network-bound payment link.")}</p>
    <dl className="plain-rows"><div><dt>{t("Network identity")}</dt><dd><code>{snapshot ? short(snapshot.status.chain_id, 12) : t("Node offline")}</code></dd></div></dl>
  </section>
  <section className="flow-card subtle"><h3>{t("How to get test atoms")}</h3><p>{t("V1 allocates 1,000,000 atoms once, in genesis. There is no faucet. Ask the genesis recipient or another funded test wallet to send atoms to your address.")}</p></section></div>;
}

function ProduceDialog({ wallets, snapshot, close, done }: { wallets: Wallet[]; snapshot: Snapshot; close: () => void; done: () => void }) {
  const authority = wallets.find(w => isAuthority(w, snapshot.genesis));
  const [password, setPassword] = useState(''); const [busy, setBusy] = useState(false); const [error, setError] = useState('');
  const produce = async (e: FormEvent) => {
    e.preventDefault(); if (!authority) return; setBusy(true); setError('');
    try {
      const [status, genesis, template] = await Promise.all([node.status(), requestRaw('/v1/genesis'), requestRaw('/v1/block-template')]);
      if (status.chain_id !== snapshot.status.chain_id || status.chain_id !== localStorage.getItem('orvexa.chain')) throw new Error('Network identity changed. Review Settings.');
      const signed = await localCrypto<string>('authorize_block', authority.encrypted, password, genesis, template, status.chain_id);
      await node.commit(signed); done(); close();
    } catch (e) { setError(`${errorText(e)} Check the current height in Explorer before retrying.`); }
    finally { setBusy(false); setPassword(''); }
  };
  return <Modal title={t("Produce a local test block")} close={close} busy={busy}><form className="dialog-body" onSubmit={produce}>
    <p>{t("This signs the current block template with the authority configured in genesis. It is not mining or decentralized finality.")}</p>
    <div className="detail-callout"><span>{t("Configured authority")}</span><code>{snapshot.genesis.authority}</code></div>
    {authority ? <><label>{t("Authority wallet")}<input value={authority.name} readOnly/></label><label>{t("Authority wallet password")}<input required autoFocus type="password" autoComplete="current-password" value={password} onChange={e => setPassword(e.target.value)}/></label><p className="caption">{t("Only the signed block is sent to the node. Pending transfers are checked again before inclusion.")}</p>
      {error && <Notice error>{error}</Notice>}<Button type="submit" disabled={busy}>{busy ? t("Signing block locally…") : t("Sign & produce block")}<Blocks size={17}/></Button></>
      : <Notice>{t("Import the configured authority's encrypted wallet in Wallets to produce blocks here, or use the native CLI. No server-side authority key is stored.")}</Notice>}
  </form></Modal>;
}

function Wallets({ wallets, active, select, create, restore, rename, remove, online, genesis }: { wallets: Wallet[]; active?: Wallet; select: (id: string) => void; create: () => void; restore: () => void; rename: (id: string, name: string) => void; remove: (id: string) => void; online: boolean; genesis?: Genesis }) {
  const [editing, setEditing] = useState<string>(); const [name, setName] = useState(''); const [removing, setRemoving] = useState<Wallet>(); const [ack, setAck] = useState(false);
  return <><div className="section-heading"><p>{t("Encrypted in this browser. Compatible with ORVEXA CLI backups.")}</p><div className="button-row"><Button kind="secondary" onClick={restore}><Upload size={16}/>{t("Restore wallet")}</Button><Button onClick={create}><Plus size={16}/>{t("Create wallet")}</Button></div></div>
    {!wallets.length ? <Empty title={t("Start with your first wallet")}>{t("Create a wallet, or import Wallet A from the local setup to use its genesis allocation.")}</Empty> : <div className="wallet-list">{wallets.map(w => <div className={`wallet-row ${active?.id === w.id ? 'selected' : ''}`} key={w.id}>
      <AddressMark address={w.address} size={44}/><div className="wallet-info"><div className="wallet-name">{editing === w.id ? <form onSubmit={e => { e.preventDefault(); if (name.trim()) { rename(w.id, name.trim().slice(0, 40)); setEditing(undefined); } }}><input autoFocus aria-label={t("New wallet name")} maxLength={40} value={name} onChange={e => setName(e.target.value)}/><Button type="submit" kind="secondary">{t("Save")}</Button><Button type="button" kind="ghost" onClick={() => setEditing(undefined)}>{t("Cancel")}</Button></form> : <><strong>{w.name}</strong>{active?.id === w.id && <span className="selected-tag">{t("Selected")}</span>}{isAuthority(w, genesis) && <Badge>{t("Authority")}</Badge>}</>}</div><code>{short(w.address, 16)}</code></div>
      <div className="wallet-actions">{active?.id !== w.id && <Button kind="secondary" onClick={() => select(w.id)}>{t("Select")}</Button>}<Button kind="ghost" onClick={() => downloadWallet(w)} aria-label={t("Back up {v0}", { v0: w.name })} title={t("Back up {v0}", { v0: w.name })}><Download size={17}/></Button><Button kind="ghost" onClick={() => { setEditing(w.id); setName(w.name); }}>{t("Rename")}</Button><Button kind="ghost" onClick={() => { setRemoving(w); setAck(false); }}>{t("Remove")}</Button></div>
    </div>)}</div>}
    <Notice>{t("Creating a wallet does not create coins. V1 has a single genesis allocation and no faucet. ")}{online ? t("Balances are read from the connected node.") : t("Connect the local node to read balances.")}</Notice>
    {removing && <Modal title={t("Remove {v0}?", { v0: removing.name })} close={() => setRemoving(undefined)}><div className="dialog-body"><p>{t("This removes only the encrypted browser copy. It cannot delete the address or its on-chain history. Recovery requires the encrypted backup and its password.")}</p><Button kind="secondary" onClick={() => downloadWallet(removing)}><Download size={17}/>{t("Download backup first")}</Button><label className="checkbox-label"><input type="checkbox" checked={ack} onChange={e => setAck(e.target.checked)}/>{t("I have a verified backup and know its password.")}</label><Button disabled={!ack} onClick={() => { remove(removing.id); setRemoving(undefined); }}>{t("Remove browser copy")}</Button></div></Modal>}
  </>;
}

function Backup({ wallet, restore }: { wallet?: Wallet; restore: () => void }) {
  const [file, setFile] = useState<File>(); const [password, setPassword] = useState(''); const [result, setResult] = useState(''); const [error, setError] = useState(''); const [busy, setBusy] = useState(false);
  const verify = async (e: FormEvent) => {
    e.preventDefault(); setBusy(true); setResult(''); setError('');
    try {
      if (!file || file.size > 8192) throw new Error('Choose an encrypted v1 backup up to 8 KiB.');
      const address = await localCrypto<string>('authenticate_wallet', await file.text(), password);
      if (wallet && address !== wallet.address) throw new Error('The backup is valid but belongs to a different wallet. Select that wallet or restore this file separately.');
      setResult(address);
    } catch (e) { setError(errorText(e)); } finally { setBusy(false); setPassword(''); }
  };
  return <div className="flow wide-flow"><section className="flow-card">
    <div className="flow-heading"><h2>{t("Keep a recoverable copy")}</h2></div>
    <p className="panel-description">{t("A backup is the encrypted v1 wallet file. You need both this file and its password to spend again.")}</p>
    <div className="backup-action">{wallet ? <AddressMark address={wallet.address} size={40}/> : <span className="wallet-picker-empty large"><WalletIcon size={18}/></span>}<div><strong>{wallet?.name || t("No wallet selected")}</strong><small>Argon2id + XChaCha20-Poly1305</small></div><Button disabled={!wallet} onClick={() => wallet && downloadWallet(wallet)}><Download size={16}/>{t("Download backup")}</Button></div>
    <form className="stack-form backup-check" onSubmit={verify}><h3>{t("Verify a backup")}</h3><p>{t("Test that a saved file unlocks correctly, entirely in your browser.")}</p><label>{t("Encrypted backup file")}<FileInput aria-label={t("Encrypted backup file")} type="file" required accept=".json,application/json" onChange={e => setFile(e.target.files?.[0])}/></label><label>{t("Backup password")}<input required type="password" autoComplete="current-password" value={password} onChange={e => setPassword(e.target.value)}/></label>{error && <Notice error>{error}</Notice>}{result && <div className="verified-result" role="status"><CheckCheck size={18}/><span>{t("Backup verified locally. Address: {v0}", { v0: result })}</span></div>}<Button kind="secondary" type="submit" disabled={busy}>{busy ? t("Verifying locally…") : t("Verify backup")}<ShieldCheck size={16}/></Button></form>
  </section>
  <section className="flow-card subtle"><div className="subtle-heading"><KeyRound size={18}/><h3>{t("Already have a backup?")}</h3></div><p>{t("Restore a wallet from the same encrypted format used by the native Rust CLI.")}</p><p>{t("Keep a separate copy outside this browser profile. Browser storage can be cleared or lost.")}</p><Button kind="secondary" onClick={restore}><Upload size={16}/>{t("Restore a wallet")}</Button><p className="caption">{t("V1 has no mnemonic seed phrase. Wallet names are local labels; the backup restores its key and address. History is read from the connected chain.")}</p></section></div>;
}

function Explorer({ route, snapshot, online, produce }: { route: string; snapshot?: Snapshot; online: boolean; produce: () => void }) {
  const [query, setQuery] = useState(''); const [searchError, setSearchError] = useState('');
  const [tx, setTx] = useState<TxDetails>(); const [block, setBlock] = useState<BlockDetails>();
  const [addressData, setAddressData] = useState<{ balance: Balance; history: History[]; height: string }>();
  const [error, setError] = useState(''); const [loading, setLoading] = useState(false);
  const [before, setBefore] = useState<string>(); const [older, setOlder] = useState<BlockDetails[]>();
  const [, kind, value] = route.split('/');
  useEffect(() => {
    let active = true; let running = false;
    setTx(undefined); setBlock(undefined); setAddressData(undefined); setError('');
    if (!kind || !value) return;
    const load = async () => {
      if (!active || running || document.visibilityState === 'hidden') return;
      running = true;
      setLoading(true);
      try {
        if (kind === 'tx' && /^[a-f0-9]{64}$/.test(value)) { const result = await node.transaction(value); if (active) setTx(result); }
        else if (kind === 'block' && /^\d{1,20}$/.test(value)) { const result = await node.block(value); if (active) setBlock(result); }
        else if (kind === 'address' && value.startsWith('void_test_')) {
          const { data } = await node.snapshot(value);
          if (!data.balance) throw new Error('Address balance is missing from node state.');
          if (active) setAddressData({ balance: data.balance, history: data.history, height: data.status.height });
        } else throw new Error('Invalid Explorer identifier.');
        if (active) setError('');
      } catch (e) { if (active) setError(errorText(e)); } finally { running = false; if (active) setLoading(false); }
    };
    void load(); const timer = setInterval(load, 5000);
    const visible = () => { if (document.visibilityState === 'visible') void load(); };
    document.addEventListener('visibilitychange', visible);
    return () => { active = false; clearInterval(timer); document.removeEventListener('visibilitychange', visible); };
  }, [kind, value]);
  useEffect(() => {
    if (!before) { setOlder(undefined); return; } let active = true;
    node.recentBlocks(before).then(result => { if (active) { setOlder(result); setError(''); } }).catch(e => { if (active) setError(errorText(e)); });
    return () => { active = false; };
  }, [before]);
  const search = (e: FormEvent) => {
    e.preventDefault(); const q = query.trim(); setSearchError('');
    if (/^[a-fA-F0-9]{64}$/.test(q)) go(`/explorer/tx/${q.toLowerCase()}`);
    else if (/^void_test_[a-f0-9]{72}$/.test(q)) go(`/explorer/address/${q}`);
    else if (/^\d{1,20}$/.test(q) && BigInt(q) > 0n) go(`/explorer/block/${BigInt(q)}`);
    else setSearchError('Enter a 64-character transaction hash, a v1 address, or a positive block height.');
  };
  const blocks = before ? older || [] : snapshot?.blocks || [];
  const back = <button className="text-link" onClick={() => go('/explorer')}><ArrowLeft size={15}/>{t("Explorer overview")}</button>;
  return <div className="explorer">
    <form className="explorer-search" onSubmit={search}><Search size={20}/><label className="sr-only" htmlFor="explorer-search">{t("Search transaction hash, address or block height")}</label><input id="explorer-search" placeholder={t("Search transaction hash, address or block height")} value={query} onChange={e => setQuery(e.target.value)} spellCheck={false}/><Button type="submit">{t("Search")}</Button></form>
    {searchError && <Notice error>{searchError}</Notice>}
    {kind && <div className="detail-back">{back}</div>}{error && <Notice error>{error}{kind === 'tx' && <p>{t("A pending transaction can disappear on restart. Not found does not mean confirmed.")}</p>}</Notice>}
    {loading && !tx && !block && !addressData && <p className="loading-line" role="status">{t("Reading the connected node…")}</p>}
    {tx && <section className="surface detail-panel"><div className="panel-heading"><h2>{t("Transaction")}</h2><Badge pending={tx.status === 'pending'}>{tx.status === 'pending' ? t("Pending") : t("Confirmed")}</Badge></div>
      <div className="detail-amount"><strong>{atoms(tx.transaction.transaction.amount)}</strong><span>{t("atoms")}</span></div>
      <div className="detail-route"><a href={`#/explorer/address/${tx.transaction.transaction.sender}`}><AddressMark address={tx.transaction.transaction.sender} size={36}/><span><small>{t("From")}</small><code>{tx.transaction.transaction.sender}</code></span><ArrowUpRight size={15}/></a><a href={`#/explorer/address/${tx.transaction.transaction.recipient}`}><AddressMark address={tx.transaction.transaction.recipient} size={36}/><span><small>{t("To")}</small><code>{tx.transaction.transaction.recipient}</code></span><ArrowUpRight size={15}/></a></div>
      <dl className="detail-grid"><div><dt>{t("Confirmations")}</dt><dd>{atoms(tx.confirmations)}</dd></div><div><dt>{t("Included in block")}</dt><dd>{tx.block_height ? <a href={`#/explorer/block/${tx.block_height}`}>#{tx.block_height}</a> : t("Not included")}</dd></div><div><dt>{t("Burned fee")}</dt><dd>{atoms(tx.transaction.transaction.fee)}{t(" atoms")}</dd></div><div><dt>{t("Sender nonce")}</dt><dd>{tx.transaction.transaction.nonce}</dd></div><div><dt>{t("Protocol")}</dt><dd>{t("Transparent v1")}</dd></div></dl>
      <div className="detail-callout"><span>{t("Transaction hash")}</span><Hash value={tx.id}/></div>
      <div className="detail-callout"><span>{t("Network")}</span><code>{tx.transaction.transaction.chain_id}</code></div><p className="caption">{t("Confirmations count accepted blocks including the containing block. The configured authority can equivocate; this is not Byzantine finality.")}</p>{tx.status === 'pending' && <Button kind="secondary" onClick={produce} disabled={!online}><Blocks size={16}/>{t("Produce authority block")}</Button>}</section>}
    {block && <section className="surface detail-panel"><div className="panel-heading"><h2>{t("Block #")}{block.block.header.height}</h2><Badge>{atoms(block.confirmations)}{t(" confirmations")}</Badge></div><Hash value={block.id}/><dl className="detail-grid"><div><dt>{t("Transactions")}</dt><dd>{block.transaction_ids.length}</dd></div><div><dt>{t("Logical slot")}</dt><dd>{block.block.header.timestamp}</dd></div></dl><div className="detail-callout"><span>{t("Parent hash")}</span><code>{block.block.header.parent}</code></div><div className="detail-callout"><span>{t("Ordered transaction commitment")}</span><code>{block.block.header.transactions_hash}</code></div><p className="caption">{t("The logical slot is a sequence counter, not a wall-clock timestamp.")}</p><ActivityList entries={block.block.transactions.map((transaction, i) => ({ id: block.transaction_ids[i], status: 'confirmed', block_height: block.block.header.height, block_id: block.id, confirmations: block.confirmations, transaction }))}/></section>}
    {addressData && <section className="surface detail-panel"><div className="panel-heading"><div className="address-heading"><AddressMark address={addressData.balance.address} size={40}/><h2>{t("Public address")}</h2></div><span className="selected-tag">{t("Transparent v1")}</span></div><Hash value={addressData.balance.address}/><dl className="detail-grid"><div><dt>{t("Confirmed balance")}</dt><dd>{atoms(addressData.balance.confirmed.balance)}{t(" atoms")}</dd></div><div><dt>{t("Next confirmed nonce")}</dt><dd>{addressData.balance.confirmed.nonce}</dd></div></dl>{snapshot?.genesis.recipient === addressData.balance.address && <Notice>{t("This address received the original genesis allocation of ")}{atoms(snapshot.genesis.supply)}{t(" test atoms. That allocation is not a transfer.")}</Notice>}<h3>{t("Confirmed transfers · latest 256")}</h3><ActivityList entries={addressData.history.map(h => ({ id: h.id, status: 'confirmed', confirmations: (BigInt(addressData.height) - BigInt(h.height) + 1n).toString(), block_height: h.height, block_id: null, transaction: h.transaction }))}/></section>}
    {!kind && <><dl className="explorer-stats"><div><dt>{t("Current height")}</dt><dd>{snapshot ? `#${atoms(snapshot.status.height)}` : '—'}</dd></div><div><dt>{t("Pending")}</dt><dd>{snapshot ? atoms(snapshot.status.pending) : '—'}</dd></div><div><dt>{t("Burned fees")}</dt><dd>{snapshot ? atoms(snapshot.status.burned_fees) : '—'} <small>{t("atoms")}</small></dd></div><div><dt>{t("Genesis supply")}</dt><dd>{snapshot ? atoms(snapshot.genesis.supply) : '—'} <small>{t("atoms")}</small></dd></div></dl>
      {!!snapshot?.pending.length && <section className="surface-section"><div className="section-heading"><div><h2>{t("Pending transfers")}</h2><p>{t("Volatile mempool · not yet confirmed")}</p></div><Button kind="secondary" disabled={!online} onClick={produce}><Blocks size={15}/>{t("Produce block")}</Button></div><ActivityList entries={snapshot.pending}/></section>}
      <section className="surface-section"><div className="section-heading"><div><h2>{t("Latest blocks")}</h2><p>{t("Accepted by this node · ")}{before ? t("before #{v0}", { v0: before }) : t("newest first")}</p></div></div>
      {blocks.length ? <div className="block-list">{blocks.map(b => <a className="block-row" key={b.id} href={`#/explorer/block/${b.block.header.height}`}><span className="block-cube">#{atoms(b.block.header.height)}</span><div><strong>{count(b.block.transactions.length, ['transfer', 'transfers'], ['перевод', 'перевода', 'переводов'])}</strong><code>{short(b.id, 10)}</code></div><span className="block-conf">{count(b.confirmations, ['confirmation', 'confirmations'], ['подтверждение', 'подтверждения', 'подтверждений'])}<ChevronRight size={15}/></span></a>)}</div> : <Empty title={before ? t("No earlier blocks") : t("Genesis is ready. No blocks yet.")}>{t("Submit a signed transfer and produce a block with the configured authority. Blocks shown here come from the real node.")}</Empty>}
      <div className="pagination">{before && <Button kind="ghost" onClick={() => setBefore(undefined)}><ArrowLeft size={15}/>{t("Latest blocks")}</Button>}{blocks.length === 8 && <Button kind="secondary" onClick={() => setBefore(blocks.at(-1)!.block.header.height)}>{t("Older blocks")}<ArrowRight size={15}/></Button>}</div></section>
      {snapshot && <div className="genesis-line"><span>{t("Genesis · height 0")}</span><code>{short(snapshot.status.chain_id, 18)}</code><span>{atoms(snapshot.genesis.supply)}{t(" test atoms allocated")}</span></div>}
    </>}
  </div>;
}

function Settings({ snapshot, observed, online, interval, setIntervalValue, adopt, produce }: { snapshot?: Snapshot; observed?: Status; online: boolean; interval: string; setIntervalValue: (value: string) => void; adopt: () => void; produce: () => void }) {
  const [ack, setAck] = useState(false);
  const pinned = localStorage.getItem('orvexa.chain');
  const mismatch = !!observed && observed.chain_id !== pinned;
  return <div className="settings-layout"><section className="surface"><div className="panel-heading"><h2>{t("Network connection")}</h2><span className={`state-pill ${online ? 'online' : ''}`}><span className="connection-dot"/>{online ? t("Connected") : t("Offline")}</span></div><dl className="settings-list"><div><dt>{t("Node origin")}</dt><dd><code>{window.location.origin}</code></dd></div><div><dt>{t("Connection")}</dt><dd>{online ? t("Connected") : t("Unavailable or network mismatch")}</dd></div><div><dt>{t("Protocol")}</dt><dd>{observed ? t("v{v0} · transparent ledger", { v0: observed.protocol }) : t("Not read yet")}</dd></div><div><dt>{t("Refresh interval")}</dt><dd><Dropdown className="inline-dropdown" align="end" label={t("Refresh interval")} value={interval} onChange={setIntervalValue} options={[{ value: '5000', label: t("5 seconds") }, { value: '10000', label: t("10 seconds") }, { value: '30000', label: t("30 seconds") }]}/></dd></div></dl>
    <div className="detail-callout"><span>{t("Pinned chain ID")}</span><code>{pinned || t("Not connected yet")}</code></div>
    {observed && <div className="detail-callout"><span>{t("Observed chain ID")}</span><code>{observed.chain_id}</code></div>}
    {mismatch && <div className="stack-form"><Notice>{t("The node belongs to a different network. Signing is disabled until you explicitly select that network. Existing keys and backups will not be migrated.")}</Notice><label className="checkbox-label"><input type="checkbox" checked={ack} onChange={e => setAck(e.target.checked)}/>{t("I verified this chain ID and want to use this test network.")}</label><Button disabled={!ack || observed?.protocol !== '1'} onClick={adopt}>{t("Use observed test network")}</Button></div>}
    {snapshot && !mismatch && <><div className="detail-callout"><span>{t("Authority")}</span><code>{snapshot.genesis.authority}</code></div><Button kind="secondary" onClick={produce} disabled={!online}><Blocks size={16}/>{t("Produce local block")}</Button></>}
  </section><section className="surface trust-model"><h2>{t("How ORVEXA v1 works")}</h2><dl>
    <div><dt><LockKeyhole size={16}/>{t("Local signing")}</dt><dd>{t("Rust WebAssembly uses the v1 Ed25519 wallet, Argon2id and XChaCha20-Poly1305. No unlocked wallet session is retained.")}</dd></div>
    <div><dt><Globe2 size={16}/>{t("Transparent transfers")}</dt><dd>{t("Addresses, amounts and sender links are public. A future private ledger requires a separate adapter and independent review.")}</dd></div>
    <div><dt><Blocks size={16}/>{t("Single authority")}</dt><dd>{t("The authority can censor or equivocate. Confirmations are accepted block counts, not decentralized finality.")}</dd></div>
    <div><dt><ShieldCheck size={16}/>{t("Browser trust")}</dt><dd>{t("Malware, extensions or compromised app code can access a password while you use it. Keep offline encrypted backups.")}</dd></div>
    <div><dt><Activity size={16}/>{t("Local replication")}</dt><dd>{t("The local network script pulls accepted blocks to two peers. This is not public P2P gossip.")}</dd></div>
  </dl></section></div>;
}

function loadLocal() {
  try { return { wallets: readWallets(), error: '' }; }
  catch (e) { return { wallets: [] as Wallet[], error: errorText(e) }; }
}

export default function App() {
  const locale = useLocale();
  useEffect(() => { document.documentElement.lang = locale; document.title = locale === 'ru' ? 'ORVEXA — Кошелёк и обозреватель' : 'ORVEXA — Wallet & Explorer'; }, [locale]);
  const [initial] = useState(loadLocal);
  const [wallets, setWallets] = useState(initial.wallets);
  const [selected, setSelected] = useState(() => localStorage.getItem('orvexa.active-wallet') || initial.wallets[0]?.id || '');
  const wallet = wallets.find(w => w.id === selected);
  const [route, setRoute] = useState(readRoute); const [mobileNav, setMobileNav] = useState(false);
  const [mobile, setMobile] = useState(() => matchMedia('(max-width: 760px)').matches);
  const menuButton = useRef<HTMLButtonElement>(null); const drawer = useRef<HTMLElement>(null); const wasOpen = useRef(false);
  const [snapshot, setSnapshot] = useState<Snapshot>(); const [observed, setObserved] = useState<Status>();
  const [error, setError] = useState(''); const [localError, setLocalError] = useState(initial.error);
  const [online, setOnline] = useState(false); const [refreshing, setRefreshing] = useState(false); const [attempted, setAttempted] = useState(false); const [sync, setSync] = useState<Sync>();
  const [tick, setTick] = useState(0); const [interval, setIntervalValue] = useState('5000');
  const [walletAction, setWalletAction] = useState<WalletAction>(null); const [produce, setProduce] = useState(false); const [receipt, setReceipt] = useState('');
  useEffect(() => {
    const media = matchMedia('(max-width: 760px)');
    const changed = () => { setMobile(media.matches); if (!media.matches) setMobileNav(false); };
    media.addEventListener('change', changed); return () => media.removeEventListener('change', changed);
  }, []);
  useEffect(() => {
    if (mobile && mobileNav) { drawer.current?.querySelector<HTMLButtonElement>('.drawer-close')?.focus(); wasOpen.current = true; }
    else if (wasOpen.current) { menuButton.current?.focus(); wasOpen.current = false; }
  }, [mobile, mobileNav]);
  useEffect(() => { const changed = () => { setRoute(readRoute()); setMobileNav(false); }; window.addEventListener('hashchange', changed); return () => window.removeEventListener('hashchange', changed); }, []);
  useEffect(() => {
    const changed = (e: StorageEvent) => {
      if (e.key === 'orvexa.encrypted-wallets.v1' || e.key === null) { try { setWallets(readWallets()); } catch (error) { setLocalError(errorText(error)); } }
      if (e.key === 'orvexa.chain' || e.key === null) { setOnline(false); setTick(value => value + 1); }
    };
    window.addEventListener('storage', changed); return () => window.removeEventListener('storage', changed);
  }, []);
  useEffect(() => {
    let active = true; let running = false;
    const load = async () => {
      if (running || document.visibilityState === 'hidden') return; running = true; if (active) setRefreshing(true);
      try {
        const { raw, data } = await node.snapshot(wallet?.address);
        const status = data.status; if (active) setObserved(status);
        if (status.protocol !== '1' || !status.privacy.startsWith('TRANSPARENT')) throw new Error('Unsupported ledger. This app only supports transparent v1; no private adapter is enabled.');
        const pin = localStorage.getItem('orvexa.chain');
        if (pin && pin !== status.chain_id) throw new Error('Network identity changed. Open Settings to review the observed chain ID. Signing is disabled.');
        if (!verifiedNetworks.has(status.chain_id)) { await localCrypto('verify_network', raw, status.chain_id); verifiedNetworks.add(status.chain_id); }
        if (active) {
          // Verification awaits a worker. Another tab/user may pin a different
          // chain during that time; never overwrite the newer choice.
          const currentPin = localStorage.getItem('orvexa.chain');
          if (currentPin && currentPin !== status.chain_id) throw new Error('Network identity changed. Open Settings to review the observed chain ID. Signing is disabled.');
          if (!currentPin) localStorage.setItem('orvexa.chain', status.chain_id);
          setSnapshot({ ...data, balance: data.balance || undefined, checked: new Date(), walletId: wallet?.id });
          setError(''); setOnline(true);
        }
        const next = await readSync(status.height).catch(() => undefined);
        if (active) setSync(next);
      } catch (error) { if (active) { setError(errorText(error)); setOnline(false); setSync(undefined); } }
      finally { running = false; if (active) { setRefreshing(false); setAttempted(true); } }
    };
    void load(); const timer = window.setInterval(load, Number(interval));
    const visible = () => { if (document.visibilityState === 'visible') void load(); };
    document.addEventListener('visibilitychange', visible);
    return () => { active = false; window.clearInterval(timer); document.removeEventListener('visibilitychange', visible); };
  }, [wallet?.id, wallet?.address, tick, interval]);
  const refresh = useCallback(() => setTick(value => value + 1), []);
  const select = (id: string) => { setSelected(id); localStorage.setItem('orvexa.active-wallet', id); setReceipt(''); };
  const update = (next: Wallet[]) => {
    if (initial.error) throw new Error(initial.error);
    saveWallets(next); setWallets(next);
  };
  const add = (next: Wallet) => {
    const existing = wallets.find(w => w.address === next.address);
    if (existing) throw new Error(`This wallet is already stored as ${existing.name}.`);
    update([...wallets, next]); select(next.id);
  };
  const safeSnapshot = snapshot && snapshot.walletId === wallet?.id ? snapshot : snapshot ? { ...snapshot, balance: undefined, history: [] } : undefined;
  const entries: TxDetails[] = wallet && safeSnapshot ? [
    ...safeSnapshot.pending.filter(t => t.transaction.transaction.sender === wallet.address || t.transaction.transaction.recipient === wallet.address),
    ...safeSnapshot.history.map(h => ({ id: h.id, status: 'confirmed' as const, block_height: h.height, block_id: null,
      confirmations: (BigInt(safeSnapshot.status.height) - BigInt(h.height) + 1n).toString(), transaction: h.transaction })),
  ] : [];
  const page = route.split('/')[0];
  const title = t(routes.find(r => r[0] === page)?.[1] || 'Page not found');
  const showCreate = () => setWalletAction('create'); const showRestore = () => setWalletAction('restore');
  const openProduce = () => { if (online && snapshot) setProduce(true); };
  // One plain state for the topbar: connecting → syncing → ready, or why not.
  const mismatch = /^(Network identity changed|Unsupported ledger)/.test(error);
  const connection = online ? sync ? t("Syncing") : wallet ? t("Ready to send") : t("Node connected")
    : !attempted ? t("Connecting…") : mismatch ? t("Different network") : t("Node unavailable");
  const connectionState = online ? sync ? 'syncing' : 'online' : attempted ? 'offline' : '';
  return <div className={`app-shell page-${page}`}><a className="skip-link" href="#main-content" onClick={e => { e.preventDefault(); document.getElementById('main-content')?.focus(); }}>{t("Skip to content")}</a>
    {mobileNav && <button className="nav-scrim" onClick={() => setMobileNav(false)} aria-label={t("Close navigation")}/>}
    <aside id="primary-navigation" ref={drawer} className={`sidebar ${mobileNav ? 'open' : ''}`} inert={mobile && !mobileNav} onClick={e => { if ((e.target as Element).closest('a')) setMobileNav(false); }} onKeyDown={e => {
      if (!mobile || !mobileNav) return;
      if (e.key === 'Escape') { e.preventDefault(); setMobileNav(false); }
      if (e.key === 'Tab') {
        const items = drawer.current?.querySelectorAll<HTMLElement>('a[href],button:not([disabled])');
        if (!items?.length) return;
        const first = items[0]; const last = items[items.length - 1];
        if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last.focus(); }
        else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
      }
    }}><button className="icon-button drawer-close" aria-label={t("Close navigation")} onClick={() => setMobileNav(false)}><X size={20}/></button>
      <a href="#/dashboard" className="brand" aria-label={t("ORVEXA Dashboard")}><img className="brand-symbol" src="/brand/orvexa-symbol.svg" alt=""/><img className="brand-wordmark" src="/brand/orvexa-wordmark.svg" alt="ORVEXA"/></a>
      <nav aria-label={t("Wallet navigation")}>{navGroups.map((group, i) => <div className="nav-group" key={i}>{group.map(([path, label, Icon]) => <a className={`nav-link ${page === path ? 'active' : ''}`} key={path} href={`#/${path}`} aria-current={page === path ? 'page' : undefined}><Icon size={18}/><span>{t(label)}</span></a>)}</div>)}</nav>
      <div className="sidebar-foot"><div className="network-chip"><span className={`connection-dot ${online ? 'online' : ''}`}/><div><strong>{t("V1 testnet")}</strong><span>{t("Test atoms. No monetary value.")}</span></div></div></div>
    </aside>
    <div className="main-shell" inert={mobile && mobileNav}><header className="topbar"><button ref={menuButton} className="icon-button mobile-menu" aria-label={t("Open navigation")} aria-controls="primary-navigation" aria-expanded={mobileNav} onClick={() => setMobileNav(true)}><Menu size={21}/></button><img className="topbar-symbol" src="/brand/orvexa-symbol.svg" alt="" aria-hidden="true"/>
      <div className="topbar-right">{page !== 'private' && <><div className={`connection-state ${connectionState}`} title={sync ? t("Syncing: block #{v0} of #{v1}. The balance may be incomplete.", { v0: atoms(sync.local), v1: atoms(sync.target) }) : undefined}><span className="connection-dot"/><span className="connection-text">{connection}</span></div><button className="icon-button refresh-button" onClick={refresh} disabled={refreshing} aria-label={t("Refresh node data")} title={t("Refresh node data")}><RefreshCw size={16} className={refreshing ? 'spinning' : ''}/></button></>}<Dropdown className="locale-dropdown" align="end" label={t("Language")} value={locale} onChange={v => setLocale(v as 'ru' | 'en')} options={[{ value: 'ru', label: 'RU', hint: 'Русский' }, { value: 'en', label: 'EN', hint: 'English' }]}/>{page !== 'private' && <WalletPicker wallets={wallets} active={wallet} select={select}/>}</div></header>
      <main id="main-content" tabIndex={-1}><div className="page-heading"><h1>{title}</h1></div>
        {error && page !== 'node' && page !== 'private' && <Notice error>{error}{snapshot && <p>{t("Showing last known state from ")}{snapshot.checked.toLocaleTimeString(timeLocale())}{t(". Signing is disabled.")}</p>}</Notice>}
        {localError && <Notice error>{localError}</Notice>}
        {receipt && <div className="receipt" role="status"><span className="receipt-icon"><CheckCheck size={18}/></span><div><strong>{t("Transfer submitted")}</strong><span>{t("Track its current status in Explorer")}</span></div><a href={`#/explorer/tx/${receipt}`}>{t("View in Explorer")}<ArrowUpRight size={15}/></a><button className="icon-button" aria-label={t("Dismiss transfer notice")} onClick={() => setReceipt('')}><X size={17}/></button></div>}
        {page === 'dashboard' && <Dashboard snapshot={safeSnapshot} wallet={wallet} entries={entries} online={online} sync={sync} create={showCreate} restore={showRestore} produce={openProduce}/>}
        {page === 'send' && <Send key={wallet?.id || 'none'} wallet={wallet} snapshot={safeSnapshot} online={online} sync={sync} openWallets={() => go('/wallets')} sent={id => { setReceipt(id); refresh(); go('/dashboard'); }}/>}
        {page === 'receive' && <Receive wallet={wallet} snapshot={safeSnapshot} openWallets={() => go('/wallets')}/>}
        {page === 'transactions' && <section className="surface-section"><div className="section-heading"><div><h2>{wallet ? t("{v0} activity", { v0: wallet.name }) : t("Wallet activity")}</h2><p>{t("Pending and latest 256 confirmed transfers. Genesis allocation is separate.")}</p></div></div><ActivityList entries={entries} wallet={wallet}/></section>}
        {page === 'wallets' && <Wallets wallets={wallets} active={wallet} select={select} create={showCreate} restore={showRestore} online={online} genesis={safeSnapshot?.genesis} rename={(id, name) => { try { update(wallets.map(w => w.id === id ? { ...w, name } : w)); } catch (e) { setLocalError(errorText(e)); } }} remove={id => { try { const next = wallets.filter(w => w.id !== id); update(next); if (selected === id) select(next[0]?.id || ''); } catch (e) { setLocalError(errorText(e)); } }}/>}
        {page === 'backup' && <Backup wallet={wallet} restore={showRestore}/>}
        {page === 'settings' && <Settings snapshot={safeSnapshot} observed={observed} online={online} interval={interval} setIntervalValue={setIntervalValue} produce={openProduce} adopt={() => { if (observed?.protocol === '1') { localStorage.setItem('orvexa.chain', observed.chain_id); setSnapshot(undefined); setReceipt(''); refresh(); } }}/>}
        {page === 'settings' && <UpdateSettings/>}
        {page === 'explorer' && <Explorer route={route} snapshot={safeSnapshot} online={online} produce={openProduce}/>}
        {page === 'node' && <NodePanel changed={refresh}/>}
        {page === 'private' && <PrivatePanel route={route}/>}
        {!routes.some(r => r[0] === page) && <Empty title={t("Page not found")}>{t("Choose a page from the navigation.")}</Empty>}
        <footer className="page-footer"><span>{snapshot ? t("State checked {v0}", { v0: snapshot.checked.toLocaleTimeString(timeLocale()) }) : t("Waiting for node")}</span><span>{t("No real funds")}</span></footer>
      </main>
    </div>
    <UpdateToast/>
    {walletAction && <WalletDialog mode={walletAction} add={add} close={() => setWalletAction(null)}/>}
    {produce && snapshot && <ProduceDialog wallets={wallets} snapshot={snapshot} close={() => setProduce(false)} done={refresh}/>}
  </div>;
}
