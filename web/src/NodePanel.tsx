import { useEffect, useRef, useState, type FormEvent } from 'react';
import { Activity, Check, CircleAlert, Cpu, FileCheck2, Globe2, LockKeyhole, Play, RefreshCw, Square, Upload } from 'lucide-react';
import { atoms, errorText, parseNodeJson, requestRaw, short } from './adapter';

type Runtime = { enabled: boolean; transport: string; role: string; halted: boolean; peer_id: string; listen_addresses: string[]; peers: { url: string; state: string; height: string | null }[] };
type Desktop = { phase: string; configured: boolean; chain_id: string | null; seeds: string[]; peer_port: string; allow_private_peers: boolean; resume_on_launch: boolean; restarts: string; problem: string | null; runtime: Runtime | null; ledger: { height: string; tip: string } | null; validator_active: boolean; rewards_supported: boolean };
const phaseLabel: Record<string, string> = { running: 'Node running', stopped: 'Node stopped', restarting: 'Reconnecting', failed: 'Needs attention', quarantined: 'Node quarantined' };

export default function NodePanel({ changed }: { changed: () => void }) {
  const [status, setStatus] = useState<Desktop>(); const [external, setExternal] = useState<Runtime>();
  const [error, setError] = useState(''); const [busy, setBusy] = useState(''); const [editing, setEditing] = useState(false);
  const [genesis, setGenesis] = useState<File>(); const [manifest, setManifest] = useState<File>();
  const [chain, setChain] = useState(''); const [ack, setAck] = useState(false); const [lan, setLan] = useState(false);
  const [resume, setResume] = useState(false); const [port, setPort] = useState('4243');
  const [readError, setReadError] = useState('');
  const reading = useRef(false); const mutating = useRef(false); const sequence = useRef(0);
  const load = async () => {
    if (reading.current || mutating.current) return;
    reading.current = true; const request = ++sequence.current;
    try {
      const response = await fetch('/v1/desktop/status', { cache: 'no-store', redirect: 'error', signal: AbortSignal.timeout(4000) });
      if (response.status === 404) {
        const runtime = parseNodeJson<Runtime>(await requestRaw('/v1/runtime'));
        if (request === sequence.current) { setExternal(runtime); setReadError(''); } return;
      }
      if (!response.ok) throw new Error('Node controls are unavailable. Keep your data and reopen ORVEXA.');
      const next = parseNodeJson<Desktop>(await response.text());
      if (request === sequence.current) { setStatus(next); setReadError(''); }
    } catch (error) { if (request === sequence.current) setReadError(errorText(error)); }
    finally { reading.current = false; }
  };
  useEffect(() => { void load(); const timer = window.setInterval(load, 2000); return () => window.clearInterval(timer); }, []);
  useEffect(() => {
    if (status) { setLan(status.allow_private_peers); setResume(status.resume_on_launch); setPort(status.peer_port); }
  }, [status?.chain_id, status?.configured]);
  useEffect(() => {
    let active = true; setChain(''); setAck(false);
    if (manifest && manifest.size <= 8192) manifest.text().then(text => { if (active) { try { const value = JSON.parse(text).chain_id; if (typeof value === 'string' && /^[0-9a-f]{64}$/.test(value)) setChain(value); } catch { /* Server validates the entire public configuration on import. */ } } });
    return () => { active = false; };
  }, [manifest]);
  const action = async (name: 'start' | 'stop') => {
    mutating.current = true; sequence.current++; setBusy(name); setError('');
    try { setStatus(parseNodeJson<Desktop>(await requestRaw(`/v1/desktop/${name}`, '{}'))); setReadError(''); changed(); }
    catch (error) { setError(errorText(error)); }
    finally { mutating.current = false; setBusy(''); }
  };
  const configure = async (event: FormEvent) => {
    event.preventDefault(); mutating.current = true; sequence.current++; setError(''); setBusy('configure');
    try {
      if (!genesis || !manifest || genesis.size > 8192 || manifest.size > 8192 || !ack) throw new Error('Choose both public files and verify the chain ID. Maximum 8 KiB per file.');
      if (!/^\d{4,5}$/.test(port) || Number(port) < 1024 || Number(port) > 65535) throw new Error('Use a peer port from 1024 to 65535.');
      const profile = { genesis: JSON.parse(await genesis.text()), network: JSON.parse(await manifest.text()), allow_private_peers: lan, resume_on_launch: resume, peer_port: Number(port) };
      setStatus(parseNodeJson<Desktop>(await requestRaw('/v1/desktop/configure', JSON.stringify(profile))));
      setReadError(''); setEditing(false); setGenesis(undefined); setManifest(undefined); changed();
    } catch (error) { setError(errorText(error)); }
    finally { mutating.current = false; setBusy(''); }
  };
  const fresh = !readError && !!(status || external);
  const runtime = fresh ? status?.runtime || external : undefined;
  const running = fresh && status?.phase === 'running';
  const needsSetup = fresh && status && (!status.configured || editing);
  const peers = runtime?.peers || [];
  return <div className="node-workspace">
    <section className="node-operation" aria-labelledby="node-operation-title">
      <div className="node-operation-heading"><div className={`node-operating-mark ${running ? 'is-running' : ''}`}><Cpu size={30}/></div><div><h2 id="node-operation-title">{readError ? 'Node status unavailable' : status ? !status.configured ? 'Connect your node' : phaseLabel[status.phase] || 'Checking node' : external ? 'Your connected node' : 'Checking node controls'}</h2><p>{readError ? 'Waiting for a fresh observation. State-dependent controls are paused.' : status ? 'One computer. A verified copy of your network.' : external ? 'The node is managed by its external launcher.' : 'Reading the local application status.'}</p></div>
        <span className="node-role">Verifying full node</span></div>
      {readError && <div className="notice error" role="alert"><CircleAlert size={17}/><div>{readError}</div></div>}
      {error && <div className="notice error" role="alert"><CircleAlert size={17}/><div>{error}</div></div>}
      {status?.problem && <div className="notice error" role="alert"><CircleAlert size={17}/><div>{status.problem}</div></div>}
      {status?.phase === 'quarantined' && <div className="notice" role="alert"><LockKeyhole size={17}/><div>Conflicting authority signatures were observed. Preserve the evidence in your app data. Restarting will not clear quarantine.</div></div>}
      {needsSetup ? <form className="node-setup" onSubmit={configure}>
        <div className="node-setup-intro"><FileCheck2 size={21}/><p>Join the same chain as other participants. Ask your network operator for these two public files. No wallet keys or passwords are needed.</p></div>
        <div className="node-file-fields"><label>Genesis file<input required type="file" accept=".json,application/json" onChange={e => setGenesis(e.target.files?.[0])}/><small>genesis.json</small></label><label>Network file<input required type="file" accept=".json,application/json" onChange={e => setManifest(e.target.files?.[0])}/><small>network.json · actual seed addresses</small></label></div>
        <label>Peer port<input required inputMode="numeric" value={port} onChange={e => setPort(e.target.value)} maxLength={5}/><small>TCP for node connections. Your wallet API stays local.</small></label>
        {chain && <div className="detail-callout"><span>Chain ID in the selected manifest</span><code>{chain}</code></div>}
        <label className="checkbox-label"><input type="checkbox" checked={ack} onChange={e => setAck(e.target.checked)} required/>I checked this chain ID with the network operator.</label>
        <label className="checkbox-label"><input type="checkbox" checked={resume} onChange={e => setResume(e.target.checked)}/>Resume this node when I open ORVEXA.</label>
        <label className="checkbox-label"><input type="checkbox" checked={lan} onChange={e => setLan(e.target.checked)}/>Allow private peer addresses for a LAN or local test.</label>
        <div className="button-row"><button className="button primary" disabled={!!busy || !chain || !ack || !genesis} type="submit"><Upload size={16}/>{busy === 'configure' ? 'Verifying files…' : 'Verify & save network'}</button>{editing && <button type="button" className="button ghost" disabled={!!busy} onClick={() => setEditing(false)}>Cancel</button>}</div>
        <p className="caption">The app never creates a separate chain when joining. Changing networks does not migrate your wallets or coins.</p>
      </form> : <>
        <div className="node-live-facts"><div><span>Accepted height</span><strong>{fresh && status?.ledger ? `#${atoms(status.ledger.height)}` : '—'}</strong></div><div><span>Connected peers</span><strong>{runtime ? peers.length : '—'}</strong></div><div><span>Transport</span><strong>{runtime?.transport === 'libp2p-tcp-noise' ? 'TCP / Noise' : runtime?.transport || (fresh && status?.phase === 'stopped' ? 'Stopped' : '—')}</strong></div></div>
        {status && <div className="node-control-row">{status.phase === 'stopped' ? <button className="button primary" disabled={!!busy || !status.configured || !fresh} onClick={() => action('start')}>{busy === 'start' ? <RefreshCw className="spinning" size={16}/> : <Play size={16}/>}Start node</button> : <button className="button secondary" disabled={!!busy || !fresh} onClick={() => action('stop')}><Square size={15}/>{busy === 'stop' ? 'Stopping…' : 'Stop node'}</button>}
          <span>{!fresh ? 'Last reported settings are retained. Live state is unavailable.' : running ? peers.length ? 'Verifying and exchanging real chain data.' : 'Listening. Waiting for a reachable peer.' : status.resume_on_launch ? 'Resume on app launch is enabled.' : 'Your saved chain stays on this computer.'}</span>
          {fresh && status.phase === 'stopped' && <button className="text-link" onClick={() => setEditing(true)}>Network settings</button>}</div>}
        {external && <p className="caption">Start and stop controls are available in ORVEXA Desktop. This browser is connected to an independently managed node.</p>}
        {status?.chain_id && <div className="node-identity"><span>Network identity</span><code>{status.chain_id}</code></div>}
        {runtime?.peer_id && <div className="node-identity"><span>Your peer ID</span><code>{runtime.peer_id}</code></div>}
      </>}
    </section>
    <section className="node-boundary"><div><Globe2 size={19}/><h2>What your node contributes</h2></div><p>It checks signatures and ledger rules, stores accepted blocks, and exchanges transactions with peers. Keep ORVEXA open to participate.</p><p><strong>Block production and rewards are not enabled for this role.</strong> V1 still uses one configured authority. Multiple validators, automatic proposer rotation and rewards belong to a later network upgrade.</p></section>
    {!needsSetup && <section className="ledger-section"><div className="section-heading"><div><h2>Peer connections</h2><p>Actual connections reported by the local node.</p></div><Activity size={19}/></div>
      {peers.length ? <div className="node-peer-list">{peers.map(peer => <div key={peer.url}><span className="connection-dot online"/><code title={peer.url}>{short(peer.url, 18)}</code><span>{peer.state}</span><span>{peer.height === null ? 'Height not received' : `#${atoms(peer.height)}`}</span></div>)}</div>
        : <div className="node-peer-empty"><Globe2 size={22}/><div><h3>{!fresh ? 'Peer state not available' : runtime ? 'No peers connected' : 'Node is stopped'}</h3><p>{!fresh ? 'Peer counts will appear after a successful status check.' : running ? 'Check that a seed is online and reachable. This app does not supply a hosted bootstrap server.' : external ? 'Check the external launcher for connection settings.' : 'Start the node to connect to the configured network.'}</p></div></div>}
    </section>}
    <div className="node-trust-line"><Check size={15}/><span>No cloud account or VPN. Spending keys stay in your wallet.</span><a href="/open-source-notices.txt" download="ORVEXA-open-source-notices.txt">Licenses</a></div>
  </div>;
}
