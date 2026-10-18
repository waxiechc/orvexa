---
name: ORVEXA Wallet, Explorer & Node
description: Russian-first desktop wallet for ORVEXA v1 with an English option. Obsidian/navy brand, chamfer geometry from the wordmark.
colors:
  obsidian: "#050608"        # canvas
  midnight-navy: "#101A27"   # raised surface, selected nav, dialogs
  deep-blue: "#1B2B3D"       # block tiles, mark backgrounds
  steel-blue: "#334B65"      # selected wallet border, hover borders
  slate-blue: "#526D89"      # focus ring, offline dot
  ice-white: "#E6F2FF"       # text, primary actions
  secondary-text: "#9AA9BA"
  borders: "#253243"
derived:
  surface: "#0B121B"         # large panels (obsidian → midnight)
  surface-hover: "#15212F"
  line: "#1A2533"            # row separators
  text-strong: "#F3F8FF"
  text-faint: "#6F8196"
  incoming: "#A9CFF5"        # received amounts, online dot — the only "positive" tone
typography:
  ui: "Inter 4.0 (bundled, Latin + Cyrillic subset), 400/500/600"
  display: "Inter Display 500/600 — balance, page titles, large figures"
  mono: "SF Mono / Cascadia Code — addresses and hashes only"
  scale: "12 · 13 · 14 (body) · 15 · 18 · 20 · 24 · 32 (h1) · 40–64 (figures)"
rounded:
  pill: "999px — buttons, chips, badges, topbar controls"
  sm: "9px"
  md: "12px — inputs, nav items"
  lg: "18px — rows, inner panels"
  xl: "24px — surfaces, dialogs"
  cut: "22px chamfer — signature surfaces only"
---

# Design System: ORVEXA Wallet, Explorer & Node

## Direction

A modern self-custody wallet, not an operations console. The balance and the
next action come first; protocol facts stay available but quiet. Russian is the
default language, English is explicit. Brand commitments from PRODUCT.md hold:
the white eye-flower mark and ORVEXA wordmark, the obsidian/navy palette, no
purple, red, neon or glow.

## The one bold idea: the wordmark's chamfer

The ORVEXA letters have cut corners. That cut — top-left and bottom-right — is
used only on things that carry identity:

- the balance card on the dashboard (`.hero-card`, 22px cut, 1px gradient edge
  drawn by an outer layer showing through `::before`);
- the receive QR frame (`.qr-frame`, 18px);
- block tiles in Explorer (`.block-cube`, 9px; the newest block is ice-white);
- the node status mark (`.node-operating-mark`, 12px).

Everything else uses ordinary rounded surfaces, so the cut stays recognisable.

## Address marks

`AddressMark.tsx` draws a six-petal picture from the first bytes of an address,
in the logo's geometry and only palette tones. It appears in the wallet picker,
wallet cards, activity rows, send review and transaction details. It helps
people recognise an address at a glance; full addresses are still shown wherever
a decision is made (send review, receive, explorer).

## Layout

- Sidebar (244px): mark + wordmark, three unlabeled groups — money (overview,
  send, receive, operations), network (explorer, node), keys (wallets, backup,
  settings) — and a network chip at the bottom. Below 760px it becomes a drawer.
- Topbar: connection pill (connecting / node connected / node unavailable),
  refresh, language, wallet picker with the active address mark. Sticky, blurred.
- Page title only (no slogans or subtitles). Content max 1180px.
- Dashboard: chamfered balance card with four round quick actions (send,
  receive, backup, produce block) + network card; recent activity below.
- Send / receive / backup: centered flow cards (max 540px), two-step send with
  a large amount field and a review that leads with the amount.
- Explorer: large search, one stats strip, pending transfers, block list.
- Settings: connection list + "how ORVEXA v1 works" trust model.

## Components

- Buttons are pills: primary ice-white on obsidian; secondary translucent ice;
  ghost text. `.wide` = 52px full-width confirm.
- Inputs 48px, radius 12, dark fill, slate focus ring. Checkbox labels are row
  flex; ordinary labels stack.
- Badges: confirmed (light steel tint, check), pending (muted, pulsing dot).
- Activity rows: address mark with a direction dot, title + counterparty,
  status, signed amount (received amounts in `--incoming`).
- Notices: one style; errors use a stronger border and `role="alert"`.

## Motion

One orchestrated moment: the balance figure rises in on load (520ms, desktop
only). Dialogs scale in. Everything else is direct feedback — hover tints and a
small press scale. `prefers-reduced-motion` turns animation off.

## Copy

Sentence case, no all-caps labels, no marketing lines. Russian plurals for
counted nouns (`count()` in App.tsx). Protocol data, addresses and user labels
are never translated. Transparency of v1 is stated once per context: the chip on
the balance card, the note under the send flow, the trust model in settings.

## Files

`web/src/tokens.css` (palette, derived tones, fonts), `web/src/styles.css`,
`web/src/App.tsx`, `web/src/NodePanel.tsx`, `web/src/AddressMark.tsx`,
`web/src/fonts/` (Inter, OFL — see `LICENSE-Inter.txt`).

The download site (`docs/index.html`, `docs/site.css`) still uses the previous
expression of the same brand and should be brought to this system next.

## Rules carried over
### Language and Presentation

Russian is the initial application locale; the explicit RU/EN selection updates
the document language and persists when browser storage is available. Translate
route names, states, forms, help, errors and accessible labels at presentation.
Keep canonical errors and backup addresses in state so switching language does
not require retrying an operation or repeat translation. Wallet names, filenames,
addresses, hashes, keys and serialized transactions remain unchanged.

Whole-atom amounts are formatted from BigInt with the selected locale; do not
pass them through floating-point conversion. Localization changes display
grouping, not the signed value or API representation. The site provides Russian
at the root and English under `en/`, with language links and correct document
language. It keeps the same release-journal structure in both languages.

### Transaction and Network States

Pending/Ожидает and Confirmed/Подтверждено badges use words and distinct marks
(pulsing dot, check) in muted blue tones.
Confirmed means accepted block inclusion by the connected authority chain;
confirmation counts do not imply decentralized finality. Genesis allocation is
shown separately from transfers. Missing node data uses an em dash; empty
history explains the next available action. An unreachable node keeps any last
known state explicitly labeled and disables signing. A changed chain identity
requires an explicit Settings decision before signing resumes.

**The Observed State Rule.** Do not fill a disconnected or empty interface with
invented balances, blocks, transactions, confirmations or chart activity.

### Node Controls

The operation panel communicates setup, stopped, running, reconnecting, failed,
quarantined and unknown observations with text, icons and the existing palette.
It imports public genesis/network files, asks for independent chain-ID checking,
and exposes explicit start/stop controls through the same-origin desktop
controller. A browser connected to an externally launched node explains that
its launcher owns these controls.

Live height, peers and transport appear only after a successful status read.
Failed reads show unavailable state and em dashes, pause state-dependent actions,
and distinguish retained settings from a fresh observation. The localized
Verifying full node role label and nearby explanation state that this role does not
produce blocks or earn rewards. Quarantine copy preserves the conflict boundary
instead of suggesting a restart can clear it.

### Release Downloads and Planned Features

The site uses a real native-window capture as evidence, paired with a Windows
preview download, release notes and a nearby transparent-testnet notice. Lower
download rows link the EXE, ZIP and SHA-256 checksums. Links and factual copy work
without JavaScript; same-origin release metadata only enhances the file-size
label. The site is not a wallet, network bootstrap service or node dashboard.

The Mac link is a text link to source build instructions, in Ice White, rather
than a download button. Native Tao/WKWebView support and a universal build script
target macOS (14+) in source; a verified Mac application download is not offered.
Do not turn source support into a claim of a tested or signed Mac release.

Future privacy, disclosure, seed recovery, validator quorum and rewards each use
an explicit Planned/В планах badge. The network note says a public network is not live
yet and participants need a reachable seed and common configuration. Keep
present capabilities, download availability and roadmap goals separately
checkable; never use the preview image as proof of a deployed public network.

## Do's and Don'ts

### Do:

- **Do** preserve the approved eye/flower paths and ORVEXA wordmark proportions.
- **Do** default to Russian, retain an English option and show real whole test atoms.
- **Do** keep Transparent v1, public addresses and amounts explicit.
- **Do** distinguish pending submission, block inclusion and last known state.
- **Do** retain keyboard focus, mobile drawer behavior and reduced-motion support.
- **Do** show encrypted-file backup and restore; v1 has no seed phrase.
- **Do** distinguish a verifying full node from block authority and reward roles.
- **Do** pair release downloads with actual app evidence and current preview limits.
- **Do** translate presentation and accessible labels while preserving user and protocol data.

### Don't:

- **Don't** replace the approved logo with another eye, star or emblem.
- **Don't** introduce bright red, purple, neon accents or decorative glow.
- **Don't** describe v1 transfers as private, anonymous or ready for real funds.
- **Don't** display passwords, private keys or invented recovery phrases in UI examples.
- **Don't** imply automatic mining, a faucet or decentralized finality.
- **Don't** use promotional graphics or illustrative data as real ledger state.
- **Don't** present stale node observations as live or a preview as a public network.
- **Don't** advertise a verified Mac download or measured efficiency without runtime evidence.
