import FileInput from './FileInput';
import { t, useLocale, setLocale, getLocale, localizedError } from './i18n';
import { Children, useCallback, useEffect, useRef, useState, type FormEvent, type ReactNode } from 'react';
import { Activity, ArrowDownLeft, ArrowDownToLine, ArrowLeft, ArrowRight, ArrowUpRight, Blocks, Check, CheckCheck, ChevronDown, ChevronRight, CircleAlert, Copy, Download, ExternalLink, FileKey2, Globe2, History as HistoryIcon, KeyRound, LayoutDashboard, LockKeyhole, Menu, Plus, RefreshCw, Search, Settings2, ShieldCheck, Upload, Wallet as WalletIcon, X } from 'lucide-react';
import QRCode from 'qrcode';
import NodePanel from './NodePanel';
import { atoms, availableBalance, errorText, node, NodeError, parseNodeJson, requestRaw, short, type Balance, type BlockDetails, type Genesis, type History, type Status, type TxDetails } from './adapter';
import { downloadWallet, localCrypto, readWallets, saveWallets, type Wallet } from './vault';

type Snapshot = { status: Status; genesis: Genesis; blocks: BlockDetails[]; pending: TxDetails[]; balance?: Balance; history: History[]; checked: Date; walletId?: string };
type WalletAction = 'create' | 'restore' | null;
const routes = [
  ['dashboard', 'Dashboard', LayoutDashboard], ['send', 'Send', ArrowUpRight], ['receive', 'Receive', ArrowDownLeft],
  ['transactions', 'Transactions', HistoryIcon], ['wallets', 'Wallets', WalletIcon],
  ['backup', 'Backup & Restore', FileKey2], ['settings', 'Settings', Settings2],
  ['node', 'Node', Activity],
] as const;
const verifiedNetworks = new Set<string>();
const go = (path: string) => { window.location.hash = path; };
const readRoute = () => window.location.hash.slice(2) || 'dashboard';
const isAuthority = (wallet: Wallet | undefined, genesis: Genesis | undefined) => !!wallet && wallet.address === genesis?.authority;
function Button({ children, kind = 'primary', className = '', ...props }: React.ButtonHTMLAttributes<HTMLButtonElement> & { kind?: 'primary' | 'secondary' | 'ghost'; children: ReactNode }) {
  return <button className={`button ${kind} ${className}`} {...props}>{children}</button>;
}
function Badge({ children, pending = false }: { children: ReactNode; pending?: boolean }) {
  return <span className={`badge ${pending ? 'pending' : ''}`}>{pending ? <Activity size={12}/> : <Check size={12}/>} {children}</span>;
}
function Notice({ children, error = false }: { children: ReactNode; error?: boolean }) {
  return <div className={`notice ${error ? 'error' : ''}`} role={error ? 'alert' : undefined}><CircleAlert size={17}/><div>{Children.map(children, child => typeof child === 'string' ? localizedError(child) : child)}</div></div>;
}
function Empty({ title, children, action }: { title: string; children: ReactNode; action?: ReactNode }) {
  return <div className="empty"><div className="empty-mark"><Activity size={25}/></div><h3>{title}</h3><p>{children}</p>{action}</div>;
}
function CopyButton({ value, label = 'Copy' }: { value: string; label?: string }) {
  const [copied, setCopied] = useState(false); const [failed, setFailed] = useState(false);
  return <><button className="copy-button" type="button" aria-label={t(label)} onClick={async () => {
    try { await navigator.clipboard.writeText(value); setCopied(true); setFailed(false); setTimeout(() => setCopied(false), 2000); }
    catch { setFailed(true); }
  }}>{copied ? <Check size={15}/> : <Copy size={15}/>}<span>{copied ? t("Copied") : t(label)}</span></button>{failed && <small role="alert">{t("Select and copy the text manually.")}</small>}</>;
}
function Hash({ value, label }: { value: string; label?: string }) {
  return <div className="hash-value"><code title={value}>{value}</code><CopyButton value={value} label={label || t("Copy")}/></div>;
}
function Modal({ title, children, close, busy = false }: { title: string; children: ReactNode; close: () => void; busy?: boolean }) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => { ref.current?.showModal(); return () => ref.current?.close(); }, []);
  return <dialog ref={ref} className="dialog" onCancel={e => { e.preventDefault(); if (!busy) close(); }}>
    <div className="dialog-heading"><h2>{title}</h2><button className="icon-button" aria-label={t("Close dialog")} disabled={busy} onClick={close}><X size={21}/></button></div>{children}
  </dialog>;
}
function WalletPicker({ wallets, active, select }: { wallets: Wallet[]; active?: Wallet; select: (id: string) => void }) {
  return <label className="wallet-picker"><WalletIcon size={17}/><span className="sr-only">{t("Active wallet")}</span>
    <select aria-label={t("Active wallet")} value={active?.id || ''} onChange={e => select(e.target.value)}>
      {!active && <option value="">{t("Select wallet")}</option>}{wallets.map(w => <option key={w.id} value={w.id}>{w.name}</option>)}
    </select><ChevronDown size={14}/></label>;
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
    {created ? <div className="dialog-body"><div className="success-icon"><CheckCheck/></div><h3>{created.name}</h3><p>{t("The encrypted wallet is saved in this browser. Download a backup before you continue.")}</p><Hash value={created.address}/>
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
  return <div className="activity-list"><div className="list-head"><span>{t("Transaction")}</span><span>{t("Status")}</span><span>{t("Amount")}</span></div>
    {visible.map(entry => {
      const tx = entry.transaction.transaction; const self = tx.sender === tx.recipient;
      const incoming = tx.recipient === wallet?.address && !self;
      return <button className="activity-row" key={entry.id} onClick={() => go(`/explorer/tx/${entry.id}`)}>
        <div className={`tx-direction ${incoming ? 'incoming' : ''}`}>{incoming ? <ArrowDownLeft size={19}/> : <ArrowUpRight size={19}/>}</div>
        <div className="tx-summary"><strong>{self ? t("Self transfer") : incoming ? t("Received") : wallet ? t("Sent") : t("Transfer")}</strong><code>{short(entry.id, 10)}</code></div>
        <div className="tx-status"><Badge pending={entry.status === 'pending'}>{entry.status === 'pending' ? t("Pending") : t("Confirmed")}</Badge><small>{entry.block_height ? t("Block #{v0}", { v0: entry.block_height }) : t("Awaiting authority block")}</small></div>
        <div className="tx-amount"><strong>{self ? '' : wallet ? incoming ? '+' : '−' : ''}{atoms(tx.amount)}<span>{t(" atoms")}</span></strong><small>{incoming ? t("Sender fee") : t("Fee")} {atoms(tx.fee)}{t(" atoms")}</small></div><ChevronRight className="row-chevron" size={16}/>
      </button>;
    })}</div>;
}

function Dashboard({ snapshot, wallet, entries, online, create, restore, produce }: { snapshot?: Snapshot; wallet?: Wallet; entries: TxDetails[]; online: boolean; create: () => void; restore: () => void; produce: () => void }) {
  const balance = snapshot?.balance;
  const available = balance && wallet ? availableBalance(balance.confirmed.balance, wallet.address, snapshot!.pending.map(t => t.transaction)) : undefined;
  return <>
    <div className="overview-grid"><section className="balance-panel">
      <div className="section-kicker"><span className="tiny-line"/>{wallet ? t("{v0} · Confirmed balance", { v0: wallet.name }) : t("Your local wallet")}</div>
      {wallet ? <><div className="balance-number" data-testid="confirmed-balance">{balance ? atoms(balance.confirmed.balance) : '—'}<span>{t("test atoms")}</span></div>
        <div className="balance-subline"><span>{balance ? t("{v0} available after pending", { v0: atoms(available!) }) : t("Waiting for the connected node")}</span><LockKeyhole size={13}/><span>{t("Keys stay local")}</span></div>
        <div className="balance-actions"><Button onClick={() => go('/send')} disabled={!online}><ArrowUpRight size={17}/>{t("Send")}</Button><Button kind="secondary" onClick={() => go('/receive')}><ArrowDownLeft size={17}/>{t("Receive")}</Button></div>
        <div className="wallet-address-line"><code>{short(wallet.address, 13)}</code><CopyButton value={wallet.address} label="Copy address"/></div></>
        : <><h2 className="welcome-title">{t("A clear view.")}<br/>{t("Your own wallet.")}</h2><p className="welcome-copy">{t("Create a local wallet or bring an encrypted backup. Your balances come directly from the ORVEXA node.")}</p><div className="balance-actions"><Button onClick={create}><Plus size={17}/>{t("Create wallet")}</Button><Button kind="secondary" onClick={restore}><Upload size={17}/>{t("Restore")}</Button></div></>}
      <img className="balance-watermark" src="/brand/orvexa-symbol.svg" alt="" aria-hidden="true"/>
    </section>
    <section className="network-panel"><div className="panel-heading"><h2>{t("Connected network")}</h2><span className={`connection-dot ${online ? 'online' : ''}`}/></div>
      <div className="network-name">ORVEXA <span>{t("Local devnet")}</span></div><div className="node-origin"><Globe2 size={14}/>{window.location.host}</div>
      <dl className="network-facts"><div><dt>{t("Block height")}</dt><dd>{snapshot ? `#${atoms(snapshot.status.height)}` : '—'}</dd></div><div><dt>{t("Pending transfers")}</dt><dd>{snapshot ? atoms(snapshot.status.pending) : '—'}</dd></div><div><dt>{t("Burned fees")}</dt><dd>{snapshot ? t("{v0} atoms", { v0: atoms(snapshot.status.burned_fees) }) : '—'}</dd></div></dl>
      <button className="text-link" onClick={() => go('/explorer')}>{t("Open Explorer")}<ArrowUpRight size={15}/></button>
    </section></div>
    <section className="ledger-section"><div className="section-heading"><div><h2>{t("Recent activity")}</h2><p>{t("Recorded on your connected local network.")}</p></div><button className="text-link" onClick={() => go('/transactions')}>{t("View all")}<ArrowRight size={16}/></button></div>
      {!wallet ? <Empty title={t("Choose a wallet to see activity")}>{t("Your local wallet history will appear here once a wallet is selected.")}</Empty> : <ActivityList entries={entries} wallet={wallet} limit={5}/>}
    </section>
    <div className="dashboard-bottom"><div><ShieldCheck size={20}/><div><strong>{t("Local signing. Independent validation.")}</strong><p>{t("Encrypted keys stay in your browser. The Rust node checks every transfer.")}</p></div></div>
      <Button kind="secondary" onClick={produce} disabled={!online || !snapshot?.pending.length}><Blocks size={16}/>{t("Produce block")}</Button></div>
  </>;
}

function Send({ wallet, snapshot, online, sent, openWallets }: { wallet?: Wallet; snapshot?: Snapshot; online: boolean; sent: (id: string) => void; openWallets: () => void }) {
  const [recipient, setRecipient] = useState(''); const [amount, setAmount] = useState(''); const [fee, setFee] = useState('1');
  const [review, setReview] = useState(false); const [password, setPassword] = useState(''); const [busy, setBusy] = useState(false); const [error, setError] = useState(''); const [uncertain, setUncertain] = useState('');
  if (!wallet) return <Empty title={t("A wallet is needed to send")}>{t("Create or restore an encrypted wallet first.")}<Button onClick={openWallets}>{t("Manage wallets")}</Button></Empty>;
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
  return <div className="form-layout"><section className="form-panel"><div className="panel-heading"><h2>{review ? t("Review your transfer") : t("Send test atoms")}</h2><span className="small-label">{t("V1 transfer")}</span></div>
    <div className="from-wallet"><span className="wallet-avatar">{wallet.name.slice(0,1).toUpperCase()}</span><div><strong>{wallet.name}</strong><small>{available === undefined ? t("Balance unavailable") : t("{v0} atoms available", { v0: atoms(available) })}</small></div><LockKeyhole size={17}/></div>
    {!review ? <form onSubmit={check} className="stack-form"><label>{t("Recipient address")}<input required spellCheck={false} autoComplete="off" value={recipient} onChange={e => setRecipient(e.target.value)} placeholder="void_test_…"/><small>{t("Use a receiving address on this exact local network.")}</small></label>
      <label>{t("Amount")}<div className="input-unit"><input required inputMode="numeric" value={amount} onChange={e => setAmount(e.target.value)} placeholder="0"/><span>{t("test atoms")}</span></div></label>
      <label>{t("Network fee")}<div className="input-unit"><input required inputMode="numeric" value={fee} onChange={e => setFee(e.target.value)}/><span>{t("atoms")}</span></div><small>{t("Minimum 1 atom. Fees are burned; there is no service fee.")}</small></label>
      {error && <Notice error>{error}</Notice>}<Button type="submit" disabled={!online || busy}>{busy ? t("Checking…") : t("Review transfer")}<ArrowRight size={17}/></Button></form>
      : <form onSubmit={confirm} className="stack-form"><dl className="review-list"><div><dt>{t("Recipient")}</dt><dd><code>{recipient}</code></dd></div><div><dt>{t("Amount")}</dt><dd>{atoms(amount)}{t(" atoms")}</dd></div><div><dt>{t("Network fee · burned")}</dt><dd>{atoms(fee)}{t(" atoms")}</dd></div><div className="total"><dt>{recipient === wallet.address ? t("Net debit · self transfer") : t("Total debit")}</dt><dd>{atoms(recipient === wallet.address ? BigInt(fee) : BigInt(amount) + BigInt(fee))}{t(" atoms")}</dd></div></dl>
        {recipient === wallet.address && <p className="caption">{t("This returns the amount to the same address. Only the fee is burned; amount plus fee must still fit the available balance.")}</p>}
        <p className="caption">{t("Acceptance puts this transfer in the volatile mempool. It is confirmed only when the configured authority produces a block.")}</p>
        {!uncertain && <label>{t("Wallet password")}<input type="password" required autoComplete="current-password" value={password} onChange={e => setPassword(e.target.value)}/></label>}
        {error && <Notice error>{error}</Notice>}
        {uncertain ? <Notice>{t("Submission could not be confirmed. Check this exact ID before sending again: ")}<a href={`#/explorer/tx/${uncertain}`}>{short(uncertain, 16)}</a></Notice> : <Button type="submit" disabled={!online || busy}>{busy ? <><RefreshCw className="spinning" size={17}/>{t("Signing locally…")}</> : <><ArrowUpRight size={17}/>{t("Sign & send")}</>}</Button>}
        <Button kind="ghost" type="button" disabled={busy} onClick={() => { setReview(false); setUncertain(''); setError(''); }}><ArrowLeft size={15}/>{t("Back to transfer")}</Button>
      </form>}
  </section><aside className="form-aside"><div className="aside-icon"><ArrowUpRight size={25}/></div><h2>{t("A transfer you can verify.")}</h2><p>{t("Your browser signs with the existing Rust wallet. The node receives a signed transaction, never your password or private key.")}</p><div className="aside-rule"/><strong>{t("Transparent v1")}</strong><p>{t("Addresses, amounts and sender links are public. These are test atoms with no monetary value.")}</p></aside></div>;
}

function Receive({ wallet, snapshot, openWallets }: { wallet?: Wallet; snapshot?: Snapshot; openWallets: () => void }) {
  const [qr, setQr] = useState('');
  useEffect(() => { setQr(''); let active = true; if (wallet) QRCode.toDataURL(wallet.address, { width: 256, margin: 3, errorCorrectionLevel: 'M', color: { dark: '#101A27', light: '#E6F2FF' } }).then(value => { if (active) setQr(value); }); return () => { active = false; }; }, [wallet?.address]);
  if (!wallet) return <Empty title={t("Choose a receiving wallet")}>{t("Create or restore a wallet to get its receiving address.")}<Button onClick={openWallets}>{t("Manage wallets")}</Button></Empty>;
  return <div className="form-layout"><section className="form-panel receive-panel"><div className="panel-heading"><h2>{t("Receive test atoms")}</h2><ArrowDownLeft size={21}/></div><p>{t("Send to ")}<strong>{wallet.name}</strong>{t(" on the connected local network.")}</p><div className="qr-frame">{qr ? <img src={qr} alt={t("QR code for {v0} receiving address", { v0: wallet.name })} width={256} height={256}/> : <span>{t("Generating address QR…")}</span>}</div><span className="small-label">{t("Your receiving address")}</span><Hash value={wallet.address} label="Copy address"/><p className="caption">{t("The QR contains the raw v1 address. Verify the chain ID with the sender; it is not a network-bound payment link.")}</p><div className="receive-chain"><span>{t("Network identity")}</span><code>{snapshot ? short(snapshot.status.chain_id, 16) : t("Node offline")}</code></div></section>
    <aside className="form-aside"><div className="aside-icon"><ArrowDownLeft size={25}/></div><h2>{t("One address.")}<br/>{t("Your local wallet.")}</h2><p>{t("The address can be shared. Your encrypted backup and password should stay with you.")}</p><div className="aside-rule"/><strong>{t("How to get test atoms")}</strong><p>{t("V1 allocates 1,000,000 atoms once, in genesis. There is no faucet. Ask the genesis recipient or another funded test wallet to send atoms to your address.")}</p></aside></div>;
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
  return <><div className="section-heading"><div><h2>{t("Your wallets")}</h2><p>{t("Encrypted in this browser. Compatible with ORVEXA CLI backups.")}</p></div><div className="button-row"><Button kind="secondary" onClick={restore}><Upload size={16}/>{t("Restore wallet")}</Button><Button onClick={create}><Plus size={16}/>{t("Create wallet")}</Button></div></div>
    {!wallets.length ? <Empty title={t("Start with your first wallet")}>{t("Create a wallet, or import Wallet A from the local setup to use its genesis allocation.")}</Empty> : <div className="wallet-list">{wallets.map(w => <div className={`wallet-row ${active?.id === w.id ? 'selected' : ''}`} key={w.id}>
      <div className="wallet-avatar">{w.name.slice(0, 1).toUpperCase()}</div><div className="wallet-info"><div className="wallet-name">{editing === w.id ? <form onSubmit={e => { e.preventDefault(); if (name.trim()) { rename(w.id, name.trim().slice(0, 40)); setEditing(undefined); } }}><input autoFocus aria-label={t("New wallet name")} maxLength={40} value={name} onChange={e => setName(e.target.value)}/><Button type="submit" kind="secondary">{t("Save")}</Button><Button type="button" kind="ghost" onClick={() => setEditing(undefined)}>{t("Cancel")}</Button></form> : <><strong>{w.name}</strong>{active?.id === w.id && <span className="small-label">{t("Selected")}</span>}{isAuthority(w, genesis) && <Badge>{t("Authority")}</Badge>}</>}</div><code>{short(w.address, 18)}</code></div>
      <div className="wallet-actions"><Button kind="secondary" onClick={() => select(w.id)} disabled={active?.id === w.id}>{t("Select")}</Button><Button kind="ghost" onClick={() => downloadWallet(w)} aria-label={t("Back up {v0}", { v0: w.name })}><Download size={17}/></Button><Button kind="ghost" onClick={() => { setEditing(w.id); setName(w.name); }}>{t("Rename")}</Button><Button kind="ghost" onClick={() => { setRemoving(w); setAck(false); }}>{t("Remove")}</Button></div>
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
  return <div className="form-layout"><section className="form-panel"><div className="panel-heading"><h2>{t("Keep a recoverable copy")}</h2><FileKey2 size={22}/></div><p className="panel-description">{t("A backup is the encrypted v1 wallet file. You need both this file and its password to spend again.")}</p>
    <div className="backup-action"><div><strong>{wallet?.name || t("No wallet selected")}</strong><small>Argon2id + XChaCha20-Poly1305</small></div><Button disabled={!wallet} onClick={() => wallet && downloadWallet(wallet)}><Download size={16}/>{t("Download backup")}</Button></div>
    <form className="stack-form backup-check" onSubmit={verify}><h3>{t("Verify a backup")}</h3><p>{t("Test that a saved file unlocks correctly, entirely in your browser.")}</p><label>{t("Encrypted backup file")}<FileInput aria-label={t("Encrypted backup file")} type="file" required accept=".json,application/json" onChange={e => setFile(e.target.files?.[0])}/></label><label>{t("Backup password")}<input required type="password" autoComplete="current-password" value={password} onChange={e => setPassword(e.target.value)}/></label>{error && <Notice error>{error}</Notice>}{result && <div className="verified-result" role="status"><CheckCheck size={18}/><span>{t("Backup verified locally. Address: {v0}", { v0: result })}</span></div>}<Button kind="secondary" type="submit" disabled={busy}>{busy ? t("Verifying locally…") : t("Verify backup")}<ShieldCheck size={16}/></Button></form>
  </section><aside className="form-aside"><div className="aside-icon"><KeyRound size={26}/></div><h2>{t("Recovery starts")}<br/>{t("before you need it.")}</h2><p>{t("Keep a separate copy outside this browser profile. Browser storage can be cleared or lost.")}</p><div className="aside-rule"/><strong>{t("Already have a backup?")}</strong><p>{t("Restore a wallet from the same encrypted format used by the native Rust CLI.")}</p><Button kind="secondary" onClick={restore}><Upload size={16}/>{t("Restore a wallet")}</Button><p className="caption">{t("V1 has no mnemonic seed phrase. Wallet names are local labels; the backup restores its key and address. History is read from the connected chain.")}</p></aside></div>;
}

function Explorer({ route, snapshot, online, produce }: { route: string; snapshot?: Snapshot; online: boolean; produce: () => void }) {
  const [query, setQuery] = useState(''); const [searchError, setSearchError] = useState('');
  const [tx, setTx] = useState<TxDetails>(); const [block, setBlock] = useState<BlockDetails>();
  const [addressData, setAddressData] = useState<{ balance: Balance; history: History[]; height: string }>();
  const [error, setError] = useState(''); const [loading, setLoading] = useState(false);
  const [before, setBefore] = useState<string>(); const [older, setOlder] = useState<BlockDetails[]>();
  const [, kind, value] = route.split('/');
  useEffect(() => {
    let active = true;
    setTx(undefined); setBlock(undefined); setAddressData(undefined); setError('');
    if (!kind || !value) return;
    const load = async () => {
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
      } catch (e) { if (active) setError(errorText(e)); } finally { if (active) setLoading(false); }
    };
    void load(); const timer = setInterval(load, 5000); return () => { active = false; clearInterval(timer); };
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
  return <div className="explorer"><div className="explorer-intro"><div><span className="section-kicker">{t("ORVEXA Explorer")}</span><h2>{t("Follow the chain.")}</h2><p>{t("Real blocks. Verifiable inclusion. Your local network.")}</p></div><div className="explorer-height"><span>{t("Current height")}</span><strong>{snapshot ? `#${atoms(snapshot.status.height)}` : '—'}</strong></div></div>
    <form className="explorer-search" onSubmit={search}><Search size={20}/><label className="sr-only" htmlFor="explorer-search">{t("Search transaction hash, address or block height")}</label><input id="explorer-search" placeholder={t("Search transaction hash, address or block height")} value={query} onChange={e => setQuery(e.target.value)} spellCheck={false}/><Button type="submit">{t("Search")}<ArrowRight size={15}/></Button></form>
    {searchError && <Notice error>{searchError}</Notice>}
    {kind && <div className="detail-back">{back}</div>}{error && <Notice error>{error}{kind === 'tx' && <p>{t("A pending transaction can disappear on restart. Not found does not mean confirmed.")}</p>}</Notice>}
    {loading && !tx && !block && !addressData && <p role="status">{t("Reading the connected node…")}</p>}
    {tx && <section className="form-panel detail-panel"><div className="panel-heading"><h2>{t("Transaction")}</h2><Badge pending={tx.status === 'pending'}>{tx.status === 'pending' ? t("Pending") : t("Confirmed")}</Badge></div><Hash value={tx.id}/><dl className="detail-grid"><div><dt>{t("Confirmations")}</dt><dd>{atoms(tx.confirmations)}</dd></div><div><dt>{t("Included in block")}</dt><dd>{tx.block_height ? <a href={`#/explorer/block/${tx.block_height}`}>#{tx.block_height}</a> : t("Not included")}</dd></div><div><dt>{t("Amount")}</dt><dd>{atoms(tx.transaction.transaction.amount)}{t(" atoms")}</dd></div><div><dt>{t("Burned fee")}</dt><dd>{atoms(tx.transaction.transaction.fee)}{t(" atoms")}</dd></div><div><dt>{t("Sender nonce")}</dt><dd>{tx.transaction.transaction.nonce}</dd></div><div><dt>{t("Protocol")}</dt><dd>{t("Transparent v1")}</dd></div></dl><div className="detail-addresses"><div><span>{t("From")}</span><a href={`#/explorer/address/${tx.transaction.transaction.sender}`}><code>{tx.transaction.transaction.sender}</code><ArrowUpRight size={15}/></a></div><div><span>{t("To")}</span><a href={`#/explorer/address/${tx.transaction.transaction.recipient}`}><code>{tx.transaction.transaction.recipient}</code><ArrowUpRight size={15}/></a></div></div><div className="detail-callout"><span>{t("Network")}</span><code>{tx.transaction.transaction.chain_id}</code></div><p className="caption">{t("Confirmations count accepted blocks including the containing block. The configured authority can equivocate; this is not Byzantine finality.")}</p>{tx.status === 'pending' && <Button kind="secondary" onClick={produce} disabled={!online}><Blocks size={16}/>{t("Produce authority block")}</Button>}</section>}
    {block && <section className="form-panel detail-panel"><div className="panel-heading"><h2>{t("Block #")}{block.block.header.height}</h2><Badge>{atoms(block.confirmations)}{t(" confirmations")}</Badge></div><Hash value={block.id}/><dl className="detail-grid"><div><dt>{t("Transactions")}</dt><dd>{block.transaction_ids.length}</dd></div><div><dt>{t("Logical slot")}</dt><dd>{block.block.header.timestamp}</dd></div></dl><div className="detail-callout"><span>{t("Parent hash")}</span><code>{block.block.header.parent}</code></div><div className="detail-callout"><span>{t("Ordered transaction commitment")}</span><code>{block.block.header.transactions_hash}</code></div><p className="caption">{t("The logical slot is a sequence counter, not a wall-clock timestamp.")}</p><ActivityList entries={block.block.transactions.map((transaction, i) => ({ id: block.transaction_ids[i], status: 'confirmed', block_height: block.block.header.height, block_id: block.id, confirmations: block.confirmations, transaction }))}/></section>}
    {addressData && <section className="form-panel detail-panel"><div className="panel-heading"><h2>{t("Public address")}</h2><span className="small-label">{t("Transparent v1")}</span></div><Hash value={addressData.balance.address}/><dl className="detail-grid"><div><dt>{t("Confirmed balance")}</dt><dd>{atoms(addressData.balance.confirmed.balance)}{t(" atoms")}</dd></div><div><dt>{t("Next confirmed nonce")}</dt><dd>{addressData.balance.confirmed.nonce}</dd></div></dl>{snapshot?.genesis.recipient === addressData.balance.address && <Notice>{t("This address received the original genesis allocation of ")}{atoms(snapshot.genesis.supply)}{t(" test atoms. That allocation is not a transfer.")}</Notice>}<h3>{t("Confirmed transfers · latest 256")}</h3><ActivityList entries={addressData.history.map(h => ({ id: h.id, status: 'confirmed', confirmations: (BigInt(addressData.height) - BigInt(h.height) + 1n).toString(), block_height: h.height, block_id: null, transaction: h.transaction }))}/></section>}
    {!kind && <><div className="explorer-stats"><div><span>{t("Protocol")}</span><strong>{t("Transparent v1")}</strong></div><div><span>{t("Pending")}</span><strong>{snapshot ? atoms(snapshot.status.pending) : '—'}</strong></div><div><span>{t("Burned fees")}</span><strong>{snapshot ? atoms(snapshot.status.burned_fees) : '—'} <small>{t("atoms")}</small></strong></div><div><span>{t("Genesis supply")}</span><strong>{snapshot ? atoms(snapshot.genesis.supply) : '—'} <small>{t("test atoms")}</small></strong></div></div>
      <section className="ledger-section"><div className="section-heading"><div><h2>{t("Latest blocks")}</h2><p>{t("Accepted by this node · ")}{before ? t("before #{v0}", { v0: before }) : t("newest first")}</p></div><Blocks size={20}/></div>
      {blocks.length ? <div className="block-list"><div className="block-list-head"><span>{t("Height / hash")}</span><span>{t("Transactions")}</span><span>{t("Confirmations")}</span></div>{blocks.map(b => <a className="block-row" key={b.id} href={`#/explorer/block/${b.block.header.height}`}><div className="block-cube"><Blocks size={21}/></div><div><strong>#{atoms(b.block.header.height)}</strong><code>{short(b.id, 13)}</code></div><span>{b.block.transactions.length}</span><span>{atoms(b.confirmations)}<ChevronRight size={15}/></span></a>)}</div> : <Empty title={before ? t("No earlier blocks") : t("Genesis is ready. No blocks yet.")}>{t("Submit a signed transfer and produce a block with the configured authority. Blocks shown here come from the real node.")}</Empty>}
      <div className="pagination">{before && <Button kind="ghost" onClick={() => setBefore(undefined)}><ArrowLeft size={15}/>{t("Latest blocks")}</Button>}{blocks.length === 8 && <Button kind="secondary" onClick={() => setBefore(blocks.at(-1)!.block.header.height)}>{t("Older blocks")}<ArrowRight size={15}/></Button>}</div></section>
      {!!snapshot?.pending.length && <section className="ledger-section"><div className="section-heading"><div><h2>{t("Pending transfers")}</h2><p>{t("Volatile mempool · not yet confirmed")}</p></div><Button kind="secondary" disabled={!online} onClick={produce}><Blocks size={15}/>{t("Produce block")}</Button></div><ActivityList entries={snapshot.pending}/></section>}
      {snapshot && <div className="genesis-line"><span>{t("Genesis · height 0")}</span><code>{short(snapshot.status.chain_id, 18)}</code><span>{atoms(snapshot.genesis.supply)}{t(" test atoms allocated")}</span></div>}
    </>}
  </div>;
}

function Settings({ snapshot, observed, online, interval, setIntervalValue, adopt, produce }: { snapshot?: Snapshot; observed?: Status; online: boolean; interval: string; setIntervalValue: (value: string) => void; adopt: () => void; produce: () => void }) {
  const [ack, setAck] = useState(false);
  const pinned = localStorage.getItem('orvexa.chain');
  const mismatch = !!observed && observed.chain_id !== pinned;
  return <div className="settings-layout"><section className="form-panel"><div className="panel-heading"><h2>{t("Network connection")}</h2><Globe2 size={21}/></div><dl className="settings-list"><div><dt>{t("Node origin")}</dt><dd><code>{window.location.origin}</code></dd></div><div><dt>{t("Connection")}</dt><dd>{online ? t("Connected") : t("Unavailable or network mismatch")}</dd></div><div><dt>{t("Protocol")}</dt><dd>{observed ? t("v{v0} · transparent ledger", { v0: observed.protocol }) : t("Not read yet")}</dd></div><div><dt>{t("Refresh interval")}</dt><dd><select aria-label={t("Refresh interval")} value={interval} onChange={e => setIntervalValue(e.target.value)}><option value="5000">{t("5 seconds")}</option><option value="10000">{t("10 seconds")}</option><option value="30000">{t("30 seconds")}</option></select></dd></div></dl>
    <div className="detail-callout"><span>{t("Pinned chain ID")}</span><code>{pinned || t("Not connected yet")}</code></div>
    {observed && <div className="detail-callout"><span>{t("Observed chain ID")}</span><code>{observed.chain_id}</code></div>}
    {mismatch && <div className="stack-form"><Notice>{t("The node belongs to a different network. Signing is disabled until you explicitly select that network. Existing keys and backups will not be migrated.")}</Notice><label className="checkbox-label"><input type="checkbox" checked={ack} onChange={e => setAck(e.target.checked)}/>{t("I verified this chain ID and want to use this test network.")}</label><Button disabled={!ack || observed?.protocol !== '1'} onClick={adopt}>{t("Use observed test network")}</Button></div>}
    {snapshot && !mismatch && <><div className="detail-callout"><span>{t("Authority")}</span><code>{snapshot.genesis.authority}</code></div><Button kind="secondary" onClick={produce} disabled={!online}><Blocks size={16}/>{t("Produce local block")}</Button></>}
  </section><section className="form-aside"><div className="aside-icon"><Settings2 size={24}/></div><h2>{t("Explicit by design.")}</h2><strong>{t("Local signing")}</strong><p>{t("Rust WebAssembly uses the v1 Ed25519 wallet, Argon2id and XChaCha20-Poly1305. No unlocked wallet session is retained.")}</p><strong>{t("Transparent transfers")}</strong><p>{t("Addresses, amounts and sender links are public. A future private ledger requires a separate adapter and independent review.")}</p><strong>{t("Single authority")}</strong><p>{t("The authority can censor or equivocate. Confirmations are accepted block counts, not decentralized finality.")}</p><strong>{t("Browser trust")}</strong><p>{t("Malware, extensions or compromised app code can access a password while you use it. Keep offline encrypted backups.")}</p><strong>{t("Local replication")}</strong><p>{t("The local network script pulls accepted blocks to two peers. This is not public P2P gossip.")}</p></section></div>;
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
  const [online, setOnline] = useState(false); const [refreshing, setRefreshing] = useState(false);
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
      } catch (error) { if (active) { setError(errorText(error)); setOnline(false); } }
      finally { running = false; if (active) setRefreshing(false); }
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
  const title = t(page === 'explorer' ? 'Explorer' : routes.find(r => r[0] === page)?.[1] || 'Page not found');
  const showCreate = () => setWalletAction('create'); const showRestore = () => setWalletAction('restore');
  const openProduce = () => { if (online && snapshot) setProduce(true); };
  return <div className="app-shell"><a className="skip-link" href="#main-content" onClick={e => { e.preventDefault(); document.getElementById('main-content')?.focus(); }}>{t("Skip to content")}</a>
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
    }}><button className="icon-button drawer-close" aria-label={t("Close navigation")} onClick={() => setMobileNav(false)}><X size={20}/></button><a href="#/dashboard" className="brand" aria-label={t("ORVEXA Dashboard")}><img className="brand-symbol" src="/brand/orvexa-symbol.svg" alt=""/><img className="brand-wordmark" src="/brand/orvexa-wordmark.svg" alt="ORVEXA"/></a><div className="brand-caption">{t("Wallet & Explorer")}</div>
      <div className="nav-label">{t("Workspace")}</div><nav aria-label={t("Wallet navigation")}>{routes.map(([path, label, Icon]) => <a className={`nav-link ${page === path ? 'active' : ''}`} key={path} href={`#/${path}`} aria-current={page === path ? 'page' : undefined}><Icon size={18}/><span>{t(label)}</span>{page === path && <span className="nav-dot"/>}</a>)}</nav>
      <div className="nav-divider"/><a className={`nav-link explorer-nav ${page === 'explorer' ? 'active' : ''}`} href="#/explorer" aria-current={page === 'explorer' ? 'page' : undefined}><Globe2 size={18}/><span>{t("Explorer")}</span><ArrowUpRight size={15}/></a>
      <div className="sidebar-foot"><div><span className={`connection-dot ${online ? 'online' : ''}`}/><strong>{t("V1 testnet")}</strong><span className="version-tag">v1</span></div><p>{t("Test atoms only.")}<br/>{t("No monetary value.")}</p><div className="sidebar-bottom"><LockKeyhole size={13}/><span>{t("Encrypted local wallets")}</span></div></div>
    </aside>
    <div className="main-shell" inert={mobile && mobileNav}><header className="topbar"><button ref={menuButton} className="icon-button mobile-menu" aria-label={t("Open navigation")} aria-controls="primary-navigation" aria-expanded={mobileNav} onClick={() => setMobileNav(true)}><Menu size={21}/></button><div className="breadcrumb">{t("Workspace")}<span>/</span><strong>{title}</strong></div><div className="topbar-right"><div className="connection-state"><span className={`connection-dot ${online ? 'online' : ''}`}/>{online ? t("Node connected") : t("Node unavailable")}</div><label className="locale-picker"><span className="sr-only">{t("Language")}</span><select aria-label={t("Language")} value={locale} onChange={e => setLocale(e.target.value as 'ru' | 'en')}><option value="ru">RU</option><option value="en">EN</option></select></label><WalletPicker wallets={wallets} active={wallet} select={select}/></div></header>
      <main id="main-content" tabIndex={-1}><div className="page-heading"><div><h1>{title}</h1><p>{page === 'dashboard' ? t("Your wallet, connected to the chain.") : page === 'explorer' ? t("Inspect the state your node has accepted.") : page === 'send' ? t("Sign locally. Transfer on your network.") : page === 'receive' ? t("An address for your next transfer.") : page === 'transactions' ? t("Every transfer, with its actual state.") : page === 'wallets' ? t("Your keys. Your local workspace.") : page === 'backup' ? t("Make your wallet recoverable.") : t("Know exactly what you are connected to.")}</p></div><button className="refresh-button" onClick={refresh} disabled={refreshing} aria-label={t("Refresh node data")}><RefreshCw size={15} className={refreshing ? 'spinning' : ''}/><span>{t("Refresh")}</span></button></div>
        <div className="privacy-strip"><ShieldCheck size={15}/><span><strong>{t("Transparent v1")}</strong>{t(" · Addresses and amounts are public.")}</span><span className="strip-right">{t("Local test network")}</span></div>
        {error && page !== 'node' && <Notice error>{error}{snapshot && <p>{t("Showing last known state from ")}{snapshot.checked.toLocaleTimeString(getLocale() === 'ru' ? 'ru-RU' : 'en-GB')}{t(". Signing is disabled.")}</p>}</Notice>}
        {localError && <Notice error>{localError}</Notice>}
        {receipt && <div className="receipt" role="status"><CheckCheck size={19}/><div><strong>{t("Transfer submitted")}</strong><span>{t("Track its current status in Explorer")}</span></div><a href={`#/explorer/tx/${receipt}`}>{t("View in Explorer")}<ArrowUpRight size={15}/></a><button className="icon-button" aria-label={t("Dismiss transfer notice")} onClick={() => setReceipt('')}><X size={17}/></button></div>}
        {page === 'dashboard' && <Dashboard snapshot={safeSnapshot} wallet={wallet} entries={entries} online={online} create={showCreate} restore={showRestore} produce={openProduce}/>}
        {page === 'send' && <Send key={wallet?.id || 'none'} wallet={wallet} snapshot={safeSnapshot} online={online} openWallets={() => go('/wallets')} sent={id => { setReceipt(id); refresh(); go('/dashboard'); }}/>}
        {page === 'receive' && <Receive wallet={wallet} snapshot={safeSnapshot} openWallets={() => go('/wallets')}/>}
        {page === 'transactions' && <section className="ledger-section"><div className="section-heading"><div><h2>{wallet ? t("{v0} activity", { v0: wallet.name }) : t("Wallet activity")}</h2><p>{t("Pending and latest 256 confirmed transfers. Genesis allocation is separate.")}</p></div></div><ActivityList entries={entries} wallet={wallet}/></section>}
        {page === 'wallets' && <Wallets wallets={wallets} active={wallet} select={select} create={showCreate} restore={showRestore} online={online} genesis={safeSnapshot?.genesis} rename={(id, name) => { try { update(wallets.map(w => w.id === id ? { ...w, name } : w)); } catch (e) { setLocalError(errorText(e)); } }} remove={id => { try { const next = wallets.filter(w => w.id !== id); update(next); if (selected === id) select(next[0]?.id || ''); } catch (e) { setLocalError(errorText(e)); } }}/>}
        {page === 'backup' && <Backup wallet={wallet} restore={showRestore}/>}
        {page === 'settings' && <Settings snapshot={safeSnapshot} observed={observed} online={online} interval={interval} setIntervalValue={setIntervalValue} produce={openProduce} adopt={() => { if (observed?.protocol === '1') { localStorage.setItem('orvexa.chain', observed.chain_id); setSnapshot(undefined); setReceipt(''); refresh(); } }}/>}
        {page === 'explorer' && <Explorer route={route} snapshot={safeSnapshot} online={online} produce={openProduce}/>}
        {page === 'node' && <NodePanel changed={refresh}/>}
        {!['dashboard','send','receive','transactions','wallets','backup','settings','explorer','node'].includes(page) && <Empty title={t("Page not found")}>{t("Choose a page from the navigation.")}</Empty>}
        <footer className="page-footer"><span>ORVEXA <span className="footer-separator">/</span>{t(" A local research network")}</span><span>{snapshot ? t("State checked {v0}", { v0: snapshot.checked.toLocaleTimeString(getLocale() === 'ru' ? 'ru-RU' : 'en-GB') }) : t("Waiting for node")}<span className="footer-dot">·</span>{t("No real funds")}</span></footer>
      </main>
    </div>
    {walletAction && <WalletDialog mode={walletAction} add={add} close={() => setWalletAction(null)}/>}
    {produce && snapshot && <ProduceDialog wallets={wallets} snapshot={snapshot} close={() => setProduce(false)} done={refresh}/>}
  </div>;
}
