import { useCallback, useEffect, useState, type FormEvent, type ReactNode } from 'react';
import { ArrowDownLeft, ArrowRight, ArrowUpRight, Blocks, Check, Download, Eye, FileKey2, Globe2, KeyRound, LockKeyhole, Plus, ReceiptText, RefreshCw, Search, ShieldCheck, Upload, X } from 'lucide-react';
import AddressMark from './AddressMark';
import Dropdown from './Dropdown';
import { Button, CopyButton, Empty, Hash, Modal, Notice } from './App';
import { atoms, short } from './adapter';
import { t } from './i18n';
import { session, v2, type BlockView, type ChainStatus, type Draft, type State, type TxView, type Verified, type WalletView } from './v2';

const go = (path: string) => { window.location.hash = path; };
const message = (e: unknown) => e instanceof Error ? e.message : String(e);
const tabs = [['', 'Overview', ShieldCheck], ['send', 'Send', ArrowUpRight], ['history', 'History', ArrowDownLeft], ['receipts', 'Receipts', ReceiptText], ['keys', 'Phrase & keys', KeyRound], ['explorer', 'Explorer v2', Globe2]] as const;
const ACTIVE_KEY = 'orvexa.v2.active-wallet';

function Busy({ children }: { children: ReactNode }) { return <><RefreshCw className="spinning" size={17}/>{children}</>; }

function PasswordFields({ password, setPassword, repeat, setRepeat }: { password: string; setPassword: (v: string) => void; repeat?: string; setRepeat?: (v: string) => void }) {
  return <>
    <label>{t("New password")}<input required type="password" autoComplete="new-password" value={password} onChange={e => setPassword(e.target.value)} placeholder={t("At least 12 characters")}/></label>
    {setRepeat && <label>{t("Repeat password")}<input required type="password" autoComplete="new-password" value={repeat} onChange={e => setRepeat(e.target.value)}/></label>}
  </>;
}

function CreateDialog({ close, done }: { close: () => void; done: (id: string) => void }) {
  const [step, setStep] = useState<'name' | 'words' | 'check'>('name');
  const [name, setName] = useState(''); const [draft, setDraft] = useState<Draft>(); const [written, setWritten] = useState(false);
  const [answers, setAnswers] = useState<Record<number, string>>({}); const [password, setPassword] = useState(''); const [repeat, setRepeat] = useState('');
  const [busy, setBusy] = useState(false); const [error, setError] = useState('');
  const start = async (e: FormEvent) => { e.preventDefault(); setBusy(true); setError(''); try { setDraft(await v2.newWallet(name)); setStep('words'); } catch (e) { setError(message(e)); } finally { setBusy(false); } };
  const finish = async (e: FormEvent) => {
    e.preventDefault(); setError('');
    if (password !== repeat) { setError('Passwords do not match.'); return; }
    setBusy(true);
    try { const created = await v2.confirm(draft!.draft, draft!.check.map(i => [i, (answers[i] || '').trim().toLowerCase()]), password); done(created.id); }
    catch (e) { setError(message(e)); } finally { setBusy(false); setPassword(''); setRepeat(''); }
  };
  return <Modal title={t("Create a private v2 wallet")} close={close} busy={busy}>
    {step === 'name' && <form className="dialog-body" onSubmit={start}>
      <p>{t("The app creates a 24-word recovery phrase from your computer's randomness. Only the phrase can restore this wallet.")}</p>
      <label>{t("Wallet name")}<input autoFocus required maxLength={40} value={name} onChange={e => setName(e.target.value)} placeholder={t("e.g. Personal wallet")}/></label>
      {error && <Notice error>{error}</Notice>}
      <Button type="submit" disabled={busy}>{busy ? <Busy>{t("Creating phrase…")}</Busy> : <>{t("Show recovery phrase")}<ArrowRight size={17}/></>}</Button>
    </form>}
    {step === 'words' && draft && <div className="dialog-body">
      <Notice>{t("Write these 24 words on paper, in order. Anyone with them controls the wallet. ORVEXA never shows them again without your password.")}</Notice>
      <ol className="phrase-grid" aria-label={t("Recovery phrase")}>{draft.words.map((w, i) => <li key={i}><span>{i + 1}</span>{w}</li>)}</ol>
      <label className="backup-check"><input type="checkbox" checked={written} onChange={e => setWritten(e.target.checked)}/>{t("I wrote down all 24 words in order")}</label>
      <Button disabled={!written} onClick={() => setStep('check')}>{t("Check the words")}<ArrowRight size={17}/></Button>
    </div>}
    {step === 'check' && draft && <form className="dialog-body" onSubmit={finish}>
      <p>{t("Enter the words at these positions to confirm your copy.")}</p>
      {draft.check.map(i => <label key={i}>{t("Word #{v0}", { v0: i + 1 })}<input required autoComplete="off" spellCheck={false} autoCapitalize="none" value={answers[i] || ''} onChange={e => setAnswers({ ...answers, [i]: e.target.value })}/></label>)}
      <PasswordFields password={password} setPassword={setPassword} repeat={repeat} setRepeat={setRepeat}/>
      <p className="caption">{t("The password encrypts the phrase on this computer. It is not part of the recovery phrase.")}</p>
      {error && <Notice error>{error}</Notice>}
      <div className="button-row"><Button kind="ghost" type="button" disabled={busy} onClick={() => setStep('words')}>{t("Show the words again")}</Button>
        <Button type="submit" disabled={busy}>{busy ? <Busy>{t("Encrypting locally…")}</Busy> : <><Check size={17}/>{t("Create wallet")}</>}</Button></div>
    </form>}
  </Modal>;
}

function RestoreDialog({ close, done }: { close: () => void; done: (id: string) => void }) {
  const [name, setName] = useState(''); const [phrase, setPhrase] = useState(''); const [password, setPassword] = useState(''); const [repeat, setRepeat] = useState('');
  const [busy, setBusy] = useState(false); const [error, setError] = useState('');
  const submit = async (e: FormEvent) => {
    e.preventDefault(); setError('');
    if (password !== repeat) { setError('Passwords do not match.'); return; }
    setBusy(true);
    try { done((await v2.restore(name, phrase.toLowerCase(), password)).id); } catch (e) { setError(message(e)); } finally { setBusy(false); setPassword(''); setRepeat(''); }
  };
  return <Modal title={t("Restore from recovery phrase")} close={close} busy={busy}><form className="dialog-body" onSubmit={submit}>
    <p>{t("Enter the 24 words in order. The wallet then rescans the private chain and finds its payments, change and spent notes.")}</p>
    <label>{t("Wallet name")}<input autoFocus required maxLength={40} value={name} onChange={e => setName(e.target.value)}/></label>
    <label>{t("Recovery phrase")}<textarea required rows={4} spellCheck={false} autoComplete="off" autoCapitalize="none" value={phrase} onChange={e => setPhrase(e.target.value)} placeholder={t("24 words separated by spaces")}/></label>
    <PasswordFields password={password} setPassword={setPassword} repeat={repeat} setRepeat={setRepeat}/>
    {error && <Notice error>{error}</Notice>}
    <Button type="submit" disabled={busy}>{busy ? <Busy>{t("Restoring…")}</Busy> : <><Upload size={17}/>{t("Restore wallet")}</>}</Button>
  </form></Modal>;
}

function WatchDialog({ close, done }: { close: () => void; done: (id: string) => void }) {
  const [name, setName] = useState(''); const [key, setKey] = useState(''); const [busy, setBusy] = useState(false); const [error, setError] = useState('');
  const submit = async (e: FormEvent) => { e.preventDefault(); setBusy(true); setError(''); try { done((await v2.watch(name, key.trim())).id); } catch (e) { setError(message(e)); } finally { setBusy(false); } };
  return <Modal title={t("Add a view-only wallet")} close={close} busy={busy}><form className="dialog-body" onSubmit={submit}>
    <p>{t("Paste a viewing key someone shared with you. It can show payments but can never spend. It is kept only until the app closes.")}</p>
    <label>{t("Wallet name")}<input autoFocus required maxLength={40} value={name} onChange={e => setName(e.target.value)}/></label>
    <label>{t("Viewing key")}<textarea required rows={3} spellCheck={false} value={key} onChange={e => setKey(e.target.value)} placeholder="orvexa-fvk-v2:… / orvexa-ivk-v2:…"/></label>
    {error && <Notice error>{error}</Notice>}
    <Button type="submit" disabled={busy}>{busy ? <Busy>{t("Scanning…")}</Busy> : <><Eye size={17}/>{t("Add view-only wallet")}</>}</Button>
  </form></Modal>;
}

function Unlock({ id, name, done }: { id: string; name: string; done: () => void }) {
  const [password, setPassword] = useState(''); const [busy, setBusy] = useState(false); const [error, setError] = useState('');
  const submit = async (e: FormEvent) => { e.preventDefault(); setBusy(true); setError(''); try { await v2.unlock(id, password); done(); } catch (e) { setError(message(e)); } finally { setBusy(false); setPassword(''); } };
  return <div className="flow"><form className="flow-card stack-form" onSubmit={submit}>
    <div className="flow-heading"><h2>{t("Unlock {v0}", { v0: name })}</h2></div>
    <p>{t("The wallet is locked. Unlocking decrypts its phrase in this app only and rescans the private chain.")}</p>
    <label>{t("Wallet password")}<input autoFocus required type="password" autoComplete="current-password" value={password} onChange={e => setPassword(e.target.value)}/></label>
    {error && <Notice error>{error}</Notice>}
    <Button type="submit" className="wide" disabled={busy}>{busy ? <Busy>{t("Unlocking…")}</Busy> : <><LockKeyhole size={17}/>{t("Unlock")}</>}</Button>
  </form></div>;
}

function CreateNetwork({ wallet, state, done }: { wallet?: WalletView; state: State; done: () => void }) {
  const [busy, setBusy] = useState(''); const [error, setError] = useState('');
  const run = async (what: string, action: () => Promise<unknown>) => { setBusy(what); setError(''); try { await action(); done(); } catch (e) { setError(message(e)); } finally { setBusy(''); } };
  const importFile = async (file?: File) => {
    if (!file) return;
    if (file.size > 65536) { setError('This network file is too large.'); return; }
    await run('file', async () => v2.joinNetwork(await file.text()));
  };
  return <section className="surface-section"><div className="section-heading"><div><h2>{t("Choose a private v2 network")}</h2>
    <p>{t("Join a shared test network, or start a local one that lives only on this computer.")}</p></div></div>
    <div className="v2-choices">
      <div className="v2-choice"><h3>{t("Join a shared network")}</h3>
        <p>{t("This computer keeps its own copy of the chain and checks every block and proof itself. Blocks come from the network's public node.")}</p>
        {state.network.default_available && <Button disabled={!!busy} onClick={() => run('default', () => v2.joinNetwork())}>{busy === 'default' ? <Busy>{t("Connecting…")}</Busy> : <><Globe2 size={17}/>{t("Join the ORVEXA v2 test network")}</>}</Button>}
        <label className="file-picker-button">{busy === 'file' ? <Busy>{t("Checking the network file…")}</Busy> : <><Upload size={16}/>{t("Import a network file")}</>}<input type="file" accept=".json,application/json" className="sr-only" disabled={!!busy} onChange={e => void importFile(e.target.files?.[0])}/></label>
        <p className="caption">{t("Get the network file from the network's operator. It is public and is fully checked before use.")}</p>
      </div>
      <div className="v2-choice"><h3>{t("Create the local private test network")}</h3>
        <p>{t("A new chain only on this computer. All {v0} test atoms go to the selected wallet; send them on to your other v2 wallets.", { v0: atoms(state.supply) })}</p>
        {wallet?.access === 'spend' ? <Button kind="secondary" disabled={!!busy} onClick={() => run('local', () => v2.createNetwork(wallet.id))}>{busy === 'local' ? <Busy>{t("Proving the genesis…")}</Busy> : <><Blocks size={17}/>{t("Create network for {v0}", { v0: wallet.name })}</>}</Button>
          : <Notice>{t("Select and unlock a wallet with a recovery phrase first.")}</Notice>}
      </div>
    </div>
    {error && <Notice error>{error}</Notice>}
  </section>;
}

async function shareProfile() {
  const profile = await v2.profile();
  const url = URL.createObjectURL(new Blob([JSON.stringify(profile, null, 2) + '\n'], { type: 'application/json' }));
  const a = document.createElement('a'); a.href = url; a.download = 'orvexa-v2-network.json'; a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

function Overview({ wallet, state, changed }: { wallet: WalletView; state: State; changed: () => void }) {
  const [address, setAddress] = useState(wallet.address);
  useEffect(() => setAddress(wallet.address), [wallet.address]);
  return <div className="home-grid">
    <section className="hero-card" aria-label={t("{v0} · Confirmed balance", { v0: wallet.name })}>
      <img className="hero-watermark" src="/brand/orvexa-symbol.svg" alt="" aria-hidden="true"/>
      <div className="hero-top"><AddressMark address={wallet.address} size={40}/><div className="hero-wallet"><strong>{wallet.name}</strong><span><code>{short(wallet.address, 8)}</code><CopyButton value={wallet.address} label="Copy address"/></span></div><span className="hero-tag"><ShieldCheck size={14}/>{wallet.access === 'spend' ? t("Private v2") : t("View only")}</span></div>
      <div className="hero-figure"><span className="hero-label">{wallet.balance === null ? t("Received") : t("Confirmed balance")}</span>
        <div className="balance-number" data-testid="v2-balance">{atoms(wallet.balance ?? wallet.received)}<span>{t("atoms")}</span></div>
        <p className="balance-subline">{wallet.balance === null ? t("This key shows incoming payments only, so a balance cannot be known.") : BigInt(wallet.pending) > 0n ? t("{v0} atoms reserved by a transfer that is not confirmed yet.", { v0: atoms(wallet.pending) }) : t("Amounts are hidden on the chain; only this wallet can read them.")}</p></div>
      <div className="quick-actions">
        <button className="quick-action primary" disabled={wallet.access !== 'spend' || !state.network.exists} onClick={() => go('/private/send')}><span><ArrowUpRight size={20}/></span>{t("Send")}</button>
        <button className="quick-action" onClick={() => go('/private/history')}><span><ArrowDownLeft size={20}/></span>{t("History")}</button>
        <button className="quick-action" onClick={() => go('/private/keys')}><span><KeyRound size={19}/></span>{t("Keys")}</button>
      </div>
    </section>
    <section className="network-card" aria-labelledby="v2-network-title">
      <div className="card-heading"><h2 id="v2-network-title">{t("Private network v2")}</h2><span className={`state-pill ${state.network.exists ? 'online' : ''}`}><span className="connection-dot"/>{state.network.exists ? t("Online") : t("Not created")}</span></div>
      <div className="network-height"><span>{t("Block height")}</span><strong>{state.network.height === null ? '—' : `#${atoms(String(state.network.height))}`}</strong></div>
      {state.network.role && <p className="caption">{state.network.role === 'producer' ? t("This computer produces the blocks of this network.") : state.network.upstream_height !== null && state.network.height !== null && state.network.upstream_height > state.network.height ? t("Syncing: block #{v0} of #{v1}.", { v0: state.network.height, v1: state.network.upstream_height }) : t("Up to date with {v0}. Every block is checked here.", { v0: state.network.upstream || '' })}</p>}
      <dl className="network-facts"><div><dt>{t("Network")}</dt><dd>{state.network.name || '—'}</dd></div><div><dt>{t("Pending transfers")}</dt><dd>{state.network.mempool}</dd></div><div><dt>{t("Remaining supply")}</dt><dd>{state.network.remaining_supply ? atoms(state.network.remaining_supply) : '—'}</dd></div><div><dt>{t("Chain ID")}</dt><dd><code>{state.network.chain_id ? short(state.network.chain_id, 6) : '—'}</code></dd></div></dl>
      <div className="dialog-body">
        <span className="field-label">{t("Receiving address")}</span><Hash value={address} label="Copy address"/>
        {wallet.access !== 'view_full' && <Button kind="secondary" onClick={async () => { setAddress((await v2.newAddress(wallet.id)).address); changed(); }}><Plus size={16}/>{t("New address")}</Button>}
        <p className="caption">{t("Every new address is unlinkable to the others, and all of them reach this wallet.")}</p>
        {state.network.exists && <Button kind="ghost" onClick={() => void shareProfile()}><Download size={16}/>{t("Share the network file")}</Button>}
      </div>
    </section>
  </div>;
}

function Send({ wallet, state, sent }: { wallet: WalletView; state: State; sent: () => void }) {
  const [to, setTo] = useState(''); const [amount, setAmount] = useState(''); const [fee, setFee] = useState(state.default_fee);
  const [publicMode, setPublicMode] = useState(false); const [agree, setAgree] = useState(false); const [review, setReview] = useState(false);
  const [busy, setBusy] = useState(false); const [error, setError] = useState(''); const [result, setResult] = useState<{ txid: string; receipt: string | null }>();
  if (wallet.access !== 'spend') return <Empty title={t("A view-only wallet cannot send")}>{t("Choose a wallet with a recovery phrase.")}</Empty>;
  if (result) return <div className="flow"><section className="flow-card stack-form">
    <div className="flow-heading"><h2>{t("Private transfer submitted")}</h2></div>
    <span className="field-label">{t("Transaction ID")}</span><Hash value={result.txid}/>
    {result.receipt && <><span className="field-label">{t("Public receipt")}</span><Hash value={result.receipt} label="Copy receipt"/><p className="caption">{t("Publish or send this receipt; anyone can check it against the chain.")}</p></>}
    <Button onClick={() => { setResult(undefined); setReview(false); setAmount(''); setTo(''); setAgree(false); setPublicMode(false); sent(); }}>{t("Done")}</Button>
  </section></div>;
  const check = (e: FormEvent) => {
    e.preventDefault(); setError('');
    if (!/^[1-9]\d*$/.test(amount) || !/^[1-9]\d*$/.test(fee)) { setError('Amount and fee must be positive whole test atoms.'); return; }
    if (!/^ovxt1[0-9a-z]+$/i.test(to.trim())) { setError('This is not a valid ORVEXA v2 test address (ovxt1…).'); return; }
    setTo(to.trim()); setReview(true);
  };
  const confirm = async (e: FormEvent) => {
    e.preventDefault(); setBusy(true); setError('');
    try { setResult(await v2.send(wallet.id, to, amount, fee, publicMode ? { to, amount } : undefined)); } catch (e) { setError(message(e)); } finally { setBusy(false); }
  };
  return <div className="flow"><section className="flow-card">
    <div className="flow-heading"><h2>{review ? t("Review your transfer") : t("Send private test atoms")}</h2><ol className="steps" aria-label={t("Step {v0} of 2", { v0: review ? 2 : 1 })}><li className="done"/><li className={review ? 'done' : ''}/></ol></div>
    <div className="from-wallet"><AddressMark address={wallet.address} size={40}/><div><strong>{wallet.name}</strong><small>{t("{v0} atoms available", { v0: atoms(wallet.balance || '0') })}</small></div></div>
    {!review ? <form onSubmit={check} className="stack-form">
      <label className="amount-field">{t("Amount")}<div className="amount-input"><input required inputMode="numeric" value={amount} onChange={e => setAmount(e.target.value.replace(/[\s ]/g, ''))} placeholder="0" size={Math.max(2, amount.length + 1)}/><span>{t("atoms")}</span></div></label>
      <label>{t("Recipient address")}<input required spellCheck={false} autoComplete="off" value={to} onChange={e => setTo(e.target.value)} placeholder="ovxt1…"/><small>{t("An address of the private v2 test network.")}</small></label>
      <label>{t("Network fee")}<div className="input-unit"><input required inputMode="numeric" value={fee} onChange={e => setFee(e.target.value)}/><span>{t("atoms")}</span></div><small>{t("Minimum 1 atom. The fee is public and burned.")}</small></label>
      <label className="backup-check"><input type="checkbox" checked={publicMode} onChange={e => { setPublicMode(e.target.checked); setAgree(false); }}/>{t("Make this payment public")}</label>
      {error && <Notice error>{error}</Notice>}
      <Button type="submit" className="wide" disabled={!state.network.exists}>{t("Review transfer")}<ArrowRight size={17}/></Button>
    </form> : <form onSubmit={confirm} className="stack-form">
      <div className="review-amount"><span>{t("You send")}</span><strong>{atoms(amount)}<small>{t("atoms")}</small></strong></div>
      <dl className="review-list"><div className="review-recipient"><dt>{t("Recipient")}</dt><dd><AddressMark address={to} size={28}/><code>{to}</code></dd></div><div><dt>{t("Network fee · burned")}</dt><dd>{atoms(fee)}{t(" atoms")}</dd></div><div><dt>{t("Mode")}</dt><dd>{publicMode ? t("Public payment") : t("Private")}</dd></div></dl>
      {publicMode ? <div className="scope-box"><strong>{t("Becomes public with the receipt")}</strong><ul>{[t("recipient address"), t("amount"), t("transaction ID")].map(x => <li key={x}>{x}</li>)}</ul>
        <strong>{t("Stays private")}</strong><ul>{[t("who sent it"), t("your change and balance"), t("your other payments")].map(x => <li key={x}>{x}</li>)}</ul>
        <label className="backup-check"><input type="checkbox" checked={agree} onChange={e => setAgree(e.target.checked)}/>{t("I understand what becomes public. It cannot be made private later.")}</label></div>
        : <p className="caption">{t("The chain shows neither the amount, the recipient nor which of your notes was spent.")}</p>}
      {error && <Notice error>{error}</Notice>}
      <Button type="submit" className="wide" disabled={busy || (publicMode && !agree)}>{busy ? <Busy>{t("Creating the proof…")}</Busy> : <><ArrowUpRight size={17}/>{t("Prove & send")}</>}</Button>
      <Button kind="ghost" type="button" disabled={busy} onClick={() => setReview(false)}>{t("Back to transfer")}</Button>
    </form>}
    <div className="flow-foot"><LockKeyhole size={15}/><p>{t("The proof is created on this computer. The node receives only the encrypted transaction.")}</p></div>
  </section></div>;
}

function History({ wallet }: { wallet: WalletView }) {
  const [shown, setShown] = useState<{ key: string; receipt: string }>(); const [error, setError] = useState('');
  if (!wallet.activity.length) return <Empty title={t("No private payments yet")}>{t("Payments received and sent by this wallet will appear here after they are confirmed.")}</Empty>;
  const label = { received: t("Received"), sent: t("Sent"), change: t("Change") };
  return <section className="surface-section"><div className="section-heading"><div><h2>{t("Private history")}</h2><p>{t("Read from the chain by this wallet's keys. Nobody else sees these amounts.")}</p></div></div>
    {error && <Notice error>{error}</Notice>}
    {shown && <div className="stack-form"><span className="field-label">{t("Payment receipt")}</span><Hash value={shown.receipt} label="Copy receipt"/></div>}
    <div className="activity-list">{wallet.activity.map(a => { const key = `${a.txid}:${a.action}`; return <div className="activity-row" key={key}>
      <span className="tx-mark"><AddressMark address={a.txid} size={40}/><span className={`tx-direction ${a.kind !== 'sent' ? 'incoming' : ''}`}>{a.kind === 'sent' ? <ArrowUpRight size={12}/> : <ArrowDownLeft size={12}/>}</span></span>
      <div className="tx-summary"><strong>{label[a.kind]}</strong><span><code>{short(a.txid, 8)}</code></span></div>
      <div className="tx-status"><small>{t("Block #{v0}", { v0: a.height })}</small>{a.kind !== 'change' && <button className="chip-button" aria-pressed={shown?.key === key} onClick={async () => { try { const r = await v2.receipt(wallet.id, a.txid, a.action); setShown({ key, receipt: r.receipt }); } catch (e) { setError(message(e)); } }}>{t("Receipt")}</button>}</div>
      <div className={`tx-amount ${a.kind !== 'sent' ? 'incoming' : ''}`}><strong>{a.kind === 'sent' ? '−' : '+'}{atoms(a.value)}<span>{t(" atoms")}</span></strong></div>
    </div>; })}</div>
    <p className="caption">{t("A receipt proves one payment: recipient address and amount. It does not reveal the sender, change or other payments, and cannot be taken back once shared.")}</p>
  </section>;
}

function Receipts() {
  const [text, setText] = useState(''); const [busy, setBusy] = useState(false); const [error, setError] = useState(''); const [ok, setOk] = useState<Verified>();
  const submit = async (e: FormEvent) => { e.preventDefault(); setBusy(true); setError(''); setOk(undefined); try { setOk(await v2.verify(text.trim())); } catch (e) { setError(message(e)); } finally { setBusy(false); } };
  return <div className="flow"><form className="flow-card stack-form" onSubmit={submit}>
    <div className="flow-heading"><h2>{t("Check a payment receipt")}</h2></div>
    <p>{t("The receipt is checked against this computer's copy of the private chain. No key is needed.")}</p>
    <label>{t("Receipt")}<textarea required rows={4} spellCheck={false} value={text} onChange={e => setText(e.target.value)} placeholder="orvexa-receipt-v2:…"/></label>
    {error && <Notice error>{error}</Notice>}
    {ok && <div className="verified-result"><Check size={18}/><dl className="review-list"><div><dt>{t("Amount")}</dt><dd>{atoms(ok.value)}{t(" atoms")}</dd></div><div className="review-recipient"><dt>{t("Recipient")}</dt><dd><code>{ok.recipient}</code></dd></div><div><dt>{t("Block")}</dt><dd>#{ok.height} · {t("{v0} confirmations", { v0: ok.confirmations })}</dd></div><div><dt>{t("Transaction ID")}</dt><dd><code>{short(ok.txid, 10)}</code></dd></div></dl></div>}
    <Button type="submit" className="wide" disabled={busy}>{busy ? <Busy>{t("Checking…")}</Busy> : <><ShieldCheck size={17}/>{t("Check receipt")}</>}</Button>
  </form></div>;
}

function Keys({ wallet }: { wallet: WalletView }) {
  const [password, setPassword] = useState(''); const [words, setWords] = useState<string[]>(); const [full, setFull] = useState(false);
  const [key, setKey] = useState<{ key: string; scope: string[] }>(); const [busy, setBusy] = useState(''); const [error, setError] = useState('');
  if (wallet.access !== 'spend') return <Empty title={t("No phrase in a view-only wallet")}>{t("This wallet was added from a viewing key.")}</Empty>;
  const scope = full ? [t("all incoming payments to any address of this wallet"), t("all outgoing payments: recipients and amounts"), t("change, spent funds and the exact balance"), t("all past and future history; access cannot be revoked")]
    : [t("incoming payments to this wallet's addresses: amounts and time"), t("not outgoing payments, change or the balance"), t("all past and future incoming history; access cannot be revoked")];
  const run = async (what: 'phrase' | 'key') => {
    setBusy(what); setError('');
    try { if (what === 'phrase') setWords((await v2.phrase(wallet.id, password)).words); else setKey(await v2.viewingKey(wallet.id, full, password)); }
    catch (e) { setError(message(e)); } finally { setBusy(''); setPassword(''); }
  };
  return <div className="settings-layout">
    <section className="surface-section"><div className="section-heading"><div><h2>{t("Recovery phrase")}</h2><p>{t("Show the 24 words only where nobody can see your screen.")}</p></div></div>
      {words ? <><ol className="phrase-grid" aria-label={t("Recovery phrase")}>{words.map((w, i) => <li key={i}><span>{i + 1}</span>{w}</li>)}</ol><Button kind="secondary" onClick={() => setWords(undefined)}><X size={16}/>{t("Hide phrase")}</Button></>
        : <p className="caption">{t("Enter the wallet password below and choose Show phrase.")}</p>}
    </section>
    <section className="surface-section"><div className="section-heading"><div><h2>{t("Viewing key")}</h2><p>{t("Lets someone watch this wallet without being able to spend.")}</p></div></div>
      <div className="stack-form">
        <label className="backup-check"><input type="radio" name="vk" checked={!full} onChange={() => { setFull(false); setKey(undefined); }}/>{t("Incoming only")}</label>
        <label className="backup-check"><input type="radio" name="vk" checked={full} onChange={() => { setFull(true); setKey(undefined); }}/>{t("Full view")}</label>
        <div className="scope-box"><strong>{t("The holder will see")}</strong><ul>{scope.map(x => <li key={x}>{x}</li>)}</ul></div>
        {key && <><span className="field-label">{full ? t("Full viewing key") : t("Incoming viewing key")}</span><Hash value={key.key} label="Copy key"/></>}
      </div>
    </section>
    <section className="surface-section"><form className="stack-form" onSubmit={e => e.preventDefault()}>
      <label>{t("Wallet password")}<input type="password" autoComplete="current-password" value={password} onChange={e => setPassword(e.target.value)}/></label>
      {error && <Notice error>{error}</Notice>}
      <div className="button-row"><Button kind="secondary" disabled={!password || !!busy} onClick={() => run('phrase')}>{busy === 'phrase' ? <Busy>{t("Checking…")}</Busy> : <><FileKey2 size={16}/>{t("Show phrase")}</>}</Button>
        <Button disabled={!password || !!busy} onClick={() => run('key')}>{busy === 'key' ? <Busy>{t("Checking…")}</Busy> : <><KeyRound size={16}/>{t("Export viewing key")}</>}</Button></div>
    </form></section>
  </div>;
}

function Explorer() {
  const [status, setStatus] = useState<ChainStatus>(); const [blocks, setBlocks] = useState<BlockView[]>([]);
  const [query, setQuery] = useState(''); const [found, setFound] = useState<BlockView | TxView>(); const [error, setError] = useState('');
  useEffect(() => { let on = true; const load = async () => { try { const [s, b] = await Promise.all([v2.chain(), v2.blocks()]); if (on) { setStatus(s); setBlocks(b); } } catch { /* shown as empty */ } }; void load(); const id = setInterval(load, 3000); return () => { on = false; clearInterval(id); }; }, []);
  const search = async (e: FormEvent) => { e.preventDefault(); setError(''); setFound(undefined); const q = query.trim(); try { setFound(/^\d+$/.test(q) ? await v2.block(q) : await v2.tx(q)); } catch (e) { setError(message(e) === 'not_found' ? 'Not found on this node.' : message(e)); } };
  return <section className="surface-section">
    <div className="section-heading"><div><h2>{t("Explorer v2")}</h2><p>{t("Blocks, hashes, fees and confirmations of the private chain. Amounts and addresses are not on the chain in readable form.")}</p></div></div>
    <dl className="network-facts"><div><dt>{t("Block height")}</dt><dd>{status ? `#${status.height}` : '—'}</dd></div><div><dt>{t("Pending transfers")}</dt><dd>{status?.mempool ?? '—'}</dd></div><div><dt>{t("Burned fees")}</dt><dd>{status ? atoms(String(status.burned_fees)) : '—'}</dd></div></dl>
    <form className="explorer-search" onSubmit={search}><Search size={18}/><input value={query} onChange={e => setQuery(e.target.value)} aria-label={t("Transaction hash or block height")} placeholder={t("Transaction hash or block height")}/><Button type="submit">{t("Search")}</Button></form>
    {error && <Notice error>{error}</Notice>}
    {found && ('txid' in found ? <dl className="review-list"><div><dt>{t("Transaction ID")}</dt><dd><code>{found.txid}</code></dd></div><div><dt>{t("Status")}</dt><dd>{found.height === null ? t("Waiting for a block") : t("Block #{v0}", { v0: found.height })}</dd></div><div><dt>{t("Confirmations")}</dt><dd>{found.confirmations}</dd></div><div><dt>{t("Fee")}</dt><dd>{found.fee}</dd></div><div><dt>{t("Actions")}</dt><dd>{found.actions}</dd></div><div><dt>{t("Size")}</dt><dd>{t("{v0} bytes", { v0: found.bytes })}</dd></div></dl>
      : <dl className="review-list"><div><dt>{t("Block")}</dt><dd>#{found.height}</dd></div><div><dt>{t("Hash")}</dt><dd><code>{found.id}</code></dd></div><div><dt>{t("Confirmations")}</dt><dd>{found.confirmations}</dd></div><div><dt>{t("Transactions")}</dt><dd>{found.txids.length}</dd></div><div><dt>{t("Fees")}</dt><dd>{found.fees}</dd></div></dl>)}
    <div className="block-list">{blocks.map(b => <button className="block-row" key={b.id} onClick={() => { setQuery(String(b.height)); setFound(b); }}><span className="block-cube">#{b.height}</span><div><strong>{t("{v0} tx", { v0: b.txids.length })}</strong><code>{short(b.id, 10)}</code></div><span className="block-conf">{t("{v0} confirmations", { v0: b.confirmations })}</span></button>)}</div>
  </section>;
}

export default function PrivatePanel({ route }: { route: string }) {
  const tab = route.split('/')[1] || '';
  const [state, setState] = useState<State>(); const [wallet, setWallet] = useState<WalletView>(); const [error, setError] = useState('');
  const [active, setActive] = useState(() => { try { return localStorage.getItem(ACTIVE_KEY) || ''; } catch { return ''; } });
  const [dialog, setDialog] = useState<'create' | 'restore' | 'watch' | null>(null); const [tick, setTick] = useState(0);
  const refresh = useCallback(() => setTick(v => v + 1), []);
  const choose = (id: string) => { setActive(id); setWallet(undefined); try { localStorage.setItem(ACTIVE_KEY, id); } catch { /* session only */ } };
  useEffect(() => {
    if (!session()) return; let on = true;
    const load = async () => {
      if (document.visibilityState === 'hidden') return;
      try {
        const s = await v2.state(); if (!on) return; setState(s); setError('');
        const current = s.wallets.find(w => w.id === active) || s.wallets[0];
        if (current && current.id !== active) setActive(current.id);
        if (current?.unlocked) { const view = await v2.wallet(current.id); if (on) setWallet(view); } else if (on) setWallet(undefined);
      } catch (e) { if (on) setError(message(e)); }
    };
    void load(); const id = setInterval(load, 3000); return () => { on = false; clearInterval(id); };
  }, [active, tick]);
  if (!session()) return <Empty title={t("Open the ORVEXA app")}>{t("The private v2 wallet works only inside the ORVEXA desktop window. A browser tab cannot reach it.")}</Empty>;
  const summary = state?.wallets.find(w => w.id === active);
  const added = (id: string) => { setDialog(null); choose(id); refresh(); };
  return <div className="private-v2">
    <div className="notice"><ShieldCheck size={18}/><div>{t("Separate private test network v2. Amounts, addresses and payment links are hidden on the chain. v1 test atoms do not move here and have no value.")}</div></div>
    {error && <Notice error>{error}</Notice>}
    <div className="v2-toolbar">
      {!!state?.wallets.length && <Dropdown className="wallet-dropdown" label={t("Private wallet")} value={active} onChange={choose}
        leading={summary?.address ? <AddressMark address={summary.address} size={26}/> : undefined}
        options={state.wallets.map(w => ({ value: w.id, label: w.name, hint: w.access !== 'spend' ? t("view only") : w.unlocked ? short(w.address || '', 6) : t("locked"), icon: <AddressMark address={w.address || w.id} size={24}/> }))}/>}
      <Button onClick={() => setDialog('create')}><Plus size={16}/>{t("New v2 wallet")}</Button>
      <Button kind="secondary" onClick={() => setDialog('restore')}><Upload size={16}/>{t("Restore from phrase")}</Button>
      <Button kind="secondary" onClick={() => setDialog('watch')}><Eye size={16}/>{t("Add view-only")}</Button>
      {summary?.unlocked && summary.access === 'spend' && <Button kind="ghost" onClick={async () => { await v2.lock(summary.id); refresh(); }}><LockKeyhole size={16}/>{t("Lock")}</Button>}
    </div>
    {!state ? <p className="loading-line">{t("Loading the private network…")}</p>
      : !state.wallets.length ? <Empty title={t("Create your first private wallet")}>{t("A v2 wallet is restored from 24 words. Create one, then create the local private test network.")}</Empty>
      : summary && !summary.unlocked ? <Unlock id={summary.id} name={summary.name} done={refresh}/>
      : !wallet ? <p className="loading-line">{t("Scanning the private chain…")}</p>
      : <>
        {!state.network.exists && <CreateNetwork wallet={wallet} state={state} done={refresh}/>}
        <nav className="tabbar" aria-label={t("Private wallet sections")}>{tabs.map(([path, label, Icon]) => <a key={path} className={tab === path ? 'active' : ''} href={`#/private${path ? `/${path}` : ''}`} aria-current={tab === path ? 'page' : undefined}><Icon size={16}/>{t(label)}</a>)}</nav>
        {tab === '' && <Overview wallet={wallet} state={state} changed={refresh}/>}
        {tab === 'send' && <Send key={wallet.id} wallet={wallet} state={state} sent={refresh}/>}
        {tab === 'history' && <History wallet={wallet}/>}
        {tab === 'receipts' && <Receipts/>}
        {tab === 'keys' && <Keys key={wallet.id} wallet={wallet}/>}
        {tab === 'explorer' && <Explorer/>}
      </>}
    {dialog === 'create' && <CreateDialog close={() => setDialog(null)} done={added}/>}
    {dialog === 'restore' && <RestoreDialog close={() => setDialog(null)} done={added}/>}
    {dialog === 'watch' && <WatchDialog close={() => setDialog(null)} done={added}/>}
  </div>;
}
