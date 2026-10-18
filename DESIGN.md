---
name: ORVEXA Wallet, Explorer & Node
description: English operating and release-download surfaces for the ORVEXA v1 preview.
colors:
  obsidian: "#050608"
  midnight-navy: "#101A27"
  deep-blue: "#1B2B3D"
  steel-blue: "#334B65"
  slate-blue: "#526D89"
  ice-white: "#E6F2FF"
  secondary-text: "#9AA9BA"
  borders: "#253243"
typography:
  display:
    fontFamily: '"Segoe UI Variable", "Segoe UI", -apple-system, BlinkMacSystemFont, Arial, sans-serif'
    fontSize: "clamp(36px, 4.1vw, 60px)"
    fontWeight: 450
    lineHeight: 1.15
    letterSpacing: "-0.035em"
  headline:
    fontFamily: '"Segoe UI Variable", "Segoe UI", -apple-system, BlinkMacSystemFont, Arial, sans-serif'
    fontSize: "30px"
    fontWeight: 550
    lineHeight: 1.25
    letterSpacing: "-0.03em"
  title:
    fontFamily: '"Segoe UI Variable", "Segoe UI", -apple-system, BlinkMacSystemFont, Arial, sans-serif'
    fontSize: "17px"
    fontWeight: 550
    lineHeight: 1.25
    letterSpacing: "-0.015em"
  body:
    fontFamily: '"Segoe UI Variable", "Segoe UI", -apple-system, BlinkMacSystemFont, Arial, sans-serif'
    fontSize: "14px"
    fontWeight: 400
    lineHeight: 1.5
  button:
    fontFamily: '"Segoe UI Variable", "Segoe UI", -apple-system, BlinkMacSystemFont, Arial, sans-serif'
    fontSize: "12px"
    fontWeight: 550
    lineHeight: 1.5
  mono:
    fontFamily: '"Cascadia Code", "SFMono-Regular", Consolas, monospace'
    fontSize: "12px"
    fontWeight: 400
  site-headline:
    fontFamily: '"Segoe UI Variable", "Segoe UI", -apple-system, BlinkMacSystemFont, Arial, sans-serif'
    fontSize: "clamp(46px, 5.4vw, 76px)"
    fontWeight: 500
    lineHeight: 1.04
    letterSpacing: "-0.038em"
  site-title:
    fontFamily: '"Segoe UI Variable", "Segoe UI", -apple-system, BlinkMacSystemFont, Arial, sans-serif'
    fontSize: "clamp(30px, 3vw, 42px)"
    fontWeight: 500
    lineHeight: 1.14
    letterSpacing: "-0.03em"
  site-body:
    fontFamily: '"Segoe UI Variable", "Segoe UI", -apple-system, BlinkMacSystemFont, Arial, sans-serif'
    fontSize: "15px"
    fontWeight: 400
    lineHeight: 1.65
  site-button:
    fontFamily: '"Segoe UI Variable", "Segoe UI", -apple-system, BlinkMacSystemFont, Arial, sans-serif'
    fontSize: "13px"
    fontWeight: 600
rounded:
  badge: "4px"
  field: "5px"
  control: "6px"
  navigation: "7px"
  notice: "8px"
  preview: "9px"
  panel: "10px"
  dialog: "12px"
spacing:
  compact: "10px"
  label: "12px"
  inline: "16px"
  mobile-gutter: "20px"
  panel-compact: "24px"
  tablet-gutter: "30px"
  site-gutter: "60px"
  site-mobile-gutter: "22px"
components:
  button-primary:
    backgroundColor: "{colors.ice-white}"
    textColor: "{colors.midnight-navy}"
    typography: "{typography.button}"
    rounded: "{rounded.control}"
    padding: "10px 17px"
  button-primary-hover:
    backgroundColor: "#FFFFFF"
  button-secondary:
    backgroundColor: "rgba(27,43,61,0.38)"
    textColor: "{colors.ice-white}"
    typography: "{typography.button}"
    rounded: "{rounded.control}"
    padding: "10px 17px"
  button-ghost:
    backgroundColor: "transparent"
    textColor: "{colors.secondary-text}"
    typography: "{typography.button}"
    rounded: "{rounded.control}"
    padding: "10px"
  input:
    backgroundColor: "#0E1925"
    textColor: "{colors.ice-white}"
    rounded: "{rounded.field}"
    padding: "11px 13px"
  navigation-active:
    backgroundColor: "#162231"
    textColor: "{colors.ice-white}"
    rounded: "{rounded.navigation}"
    padding: "12px 13px"
  network-panel:
    backgroundColor: "rgba(16,26,39,0.42)"
    textColor: "{colors.ice-white}"
    rounded: "{rounded.panel}"
    padding: "25px 25px 18px"
  badge-confirmed:
    backgroundColor: "rgba(51,75,101,0.25)"
    textColor: "#D2E5F9"
    rounded: "{rounded.badge}"
    padding: "3px 7px"
  badge-pending:
    backgroundColor: "rgba(82,109,137,0.12)"
    textColor: "#C2CFDF"
    rounded: "{rounded.badge}"
    padding: "3px 7px"
  node-operation:
    backgroundColor: "rgba(16,26,39,0.42)"
    textColor: "{colors.ice-white}"
    rounded: "{rounded.panel}"
    padding: "28px"
  site-download-button:
    backgroundColor: "{colors.ice-white}"
    textColor: "{colors.midnight-navy}"
    typography: "{typography.site-button}"
    rounded: "{rounded.control}"
    padding: "13px 19px"
  site-planned-badge:
    backgroundColor: "transparent"
    textColor: "{colors.secondary-text}"
    rounded: "{rounded.badge}"
    padding: "1px 7px"
---

# Design System: ORVEXA Wallet, Explorer & Node

## Overview

**Creative North Star: "ORVEXA local wallet workspace"**

An English operating interface for testers creating wallets, transferring
test atoms and running verifying nodes. A release-download site introduces the
working desktop preview and its current network boundaries. The confirmed brief is premium,
restrained, technological and minimal: a cold navy field, clear system type,
geometric identity, thin borders and space around important actions.

The approved eye/flower symbol and ORVEXA wordmark anchor every route. Current
SVGs in `web/public/brand/` are path-based reconstructions traced from
`ORVEXA.png`; they contain no embedded bitmap and are not an original vector
master. Preserve these geometries and proportions when extending the interface.

**Key Characteristics:**

- Obsidian-to-navy background with Ice White actions and identity.
- Open transaction rows, exact integers and compact network facts.
- Visible connection, pending, confirmed, unavailable and empty states.
- One visual language across Wallet, Explorer, Node controls and release downloads.
- Actual application captures and explicit preview/planned labels on the site.

This records the built system in `web/src/tokens.css`, `web/src/styles.css` and
`web/src/App.tsx`, including the `web/src/NodePanel.tsx` extension. The download
site in `docs/index.html` and `docs/site.css` uses the same identity as a release
journal: download action and preview boundary beside an actual native-app
capture, followed by setup, network limitations and staged goals. Its five-block
contract records the chosen structure and seed `215b7181`; this is a surface
expression, not a replacement brand.

## Colors

Use the user's exact palette names. The frontmatter is the palette reference;
CSS tokens remain the runtime source. Additional muted blues in component
definitions describe existing surface and state variants. The site's shorter
CSS names (`--navy`, `--ice`, `--text` and peers) map to this same eight-color
palette; they do not establish another color system. Sidecar tonal ramps are
preview metadata, not extra runtime colors.

### Primary

- **Ice White:** logo paths, primary button fills, principal text and keyboard focus.
- **Slate Blue:** visible field and Explorer-search borders, secondary hover borders.

### Neutral

- **Obsidian:** darkest page and navigation foundation.
- **Midnight Navy:** far end of the background gradient and primary-button text.
- **Deep Blue:** translucent secondary surfaces and information strips.
- **Steel Blue:** selection, quiet separators and confirmed-badge surface tint.
- **Secondary Text:** supporting copy, labels and inactive navigation.
- **Borders:** structural dividers and resting panel outlines.

**The Restrained State Rule.** Use wording, icons and contrast to distinguish
states within the blue palette. Do not introduce red, purple, neon or glow.

## Typography

The UI uses the local Segoe UI Variable/system stack, with Cascadia Code/system
monospace for hashes and addresses. No remote font is required. Headings are
compact; the balance is the principal display numeral. Amounts and network facts
use tabular numerals so updates do not shift alignment.

The frontmatter records the base hierarchy. Mobile page headings reduce to
(26px); the balance has explicit responsive sizes in the stylesheet. Transaction
rows use compact labels (10–13px), while long hashes wrap in detail views.
Keep the balance display role for operating numerals. The download site uses
its separate, larger headline and more open body roles; feature descriptions
and notices remain compact. Do not convert ordinary labels to tracked
uppercase decoration.

## Layout

The desktop shell has fixed navigation (236px), a top bar (92px) and a main
container capped at (1450px) with horizontal gutters (46px). Dashboard places the
balance and actions beside network facts, then uses full-width ledger rows.
Forms pair a working panel with explanatory side copy; detail pages keep hashes
and addresses available without truncating the underlying value.

At (1250px) navigation and gutters narrow; at (1000px) Dashboard and forms stack.
At (760px) navigation becomes an off-canvas drawer, the top bar reduces to
(70px), and main gutters become (20px). The drawer is inert while closed;
opening it focuses its close control, traps Tab, accepts Escape and restores
focus to the menu control. Transaction rows reflow rather than requiring a
desktop-width table. The minimum supported page width is (320px), with a
further compact adjustment at (370px).

Node controls extend this shell with a bordered operation panel, two public-file
fields, wrapping live facts and an open peer list. At the mobile breakpoint the
file fields and identity labels stack. Navigation scrolls vertically when its
full route list exceeds the available window height.

The release site uses a centered container (1400px), desktop gutters (60px),
and a split opening with copy beside a larger actual-app capture. The following
sections use open columns and fine dividers. At (1100px) gutters and gaps
contract; at (760px) columns stack and gutters become (22px); at (390px)
gutters become (18px). The download action and transparent-preview explanation
precede the image on mobile. This reading order belongs to the site, not to
every operating screen.

## Elevation & Depth

Depth comes from restrained tonal layers, translucent navy panels and one-pixel
borders. The page gradient runs diagonally from Obsidian through a muted dark
blue into Midnight Navy. The balance panel has its own quiet blue gradient and
a faint approved-symbol watermark. Neither is a glow treatment.

Within the app, only modal dialogs use a deep ambient shadow, behind a dark
backdrop; resting panels and buttons are flat. The site adds an ambient shadow
under its actual-app capture, framed by a quiet window caption. It does not
apply that treatment to ordinary content or imply live telemetry. Motion consists of short state-color transitions,
the mobile drawer slide, a desktop balance reveal and the refresh spinner.
`prefers-reduced-motion` disables transitions and animations. Exact shadow,
gradient and motion values are preserved in the sidecar.

## Shapes

Moderately rounded panels, compact controls and smaller badges share the radius
roles in the frontmatter. Circle direction markers distinguish incoming and
outgoing transfers. Use thin structural borders; field borders deliberately use
the brighter Slate Blue rather than the quieter panel-border color. Keep the
eye/flower identity separate from functional line icons. The site repeats compact
rounded download actions, a softly rounded preview frame and an outlined network
notice; numbered setup steps use small circles.

## Components

### Buttons

Ice White primary actions, outlined blue secondary actions and quiet ghost
actions share a minimum height (41px). Hover changes the surface or text tone;
keyboard focus uses an Ice White outline. Disabled controls reduce opacity and
retain a clear unavailable cursor. No decorative lift or glow is used.
The site download link keeps the same palette and radius with a taller hit
area (minimum 49px); secondary release/source links remain text links.

### Inputs / Fields

Dark filled fields have a Slate Blue stroke, visible labels and a brighter
focus stroke. Amounts are whole test atoms; password fields remain password
inputs. Search uses one outlined enclosing surface and visible focus-within.
Validation failures appear as icon-and-text notices, without relying on color.

### Navigation

Wallet routes and Explorer share the fixed sidebar and original brand lockup.
Active routes use a navy fill, thin outline and small white dot; `aria-current`
exposes selection. Mobile uses the keyboard-managed drawer described above.
A skip link leads directly to the main content.

### Cards / Containers

The balance panel is the visual anchor; the network panel is quieter. Use
bordered form panels for input and open divided rows for activity and blocks.
Reserve the modal shadow and backdrop for actual dialogs. QR codes sit on an
Ice White square and encode the selected wallet's real address.

### Transaction and Network States

Pending and Confirmed badges use words and distinct icons in muted blue tones.
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
and distinguish retained settings from a fresh observation. The role label says
Verifying full node; the nearby explanation states that this role does not
produce blocks or earn rewards. Quarantine copy preserves the conflict boundary
instead of suggesting a restart can clear it.

### Release Downloads and Planned Features

The site uses a real native-window capture as evidence, paired with a Windows
preview download, release notes and a nearby transparent-testnet notice. Lower
download rows link the EXE, ZIP and SHA-256 checksums. Links and factual copy work
without JavaScript; same-origin release metadata only enhances the file-size
label. The site is not a wallet, network bootstrap service or node dashboard.

Future privacy, disclosure, seed recovery, validator quorum and rewards each use
an explicit Planned badge. The network note says a public network is not live
yet and participants need a reachable seed and common configuration. Keep
present capabilities, download availability and roadmap goals separately
checkable; never use the preview image as proof of a deployed public network.

## Do's and Don'ts

### Do:

- **Do** preserve the approved eye/flower paths and ORVEXA wordmark proportions.
- **Do** use English UI copy, whole test atoms and real node-backed values.
- **Do** keep Transparent v1, public addresses and amounts explicit.
- **Do** distinguish pending submission, block inclusion and last known state.
- **Do** retain keyboard focus, mobile drawer behavior and reduced-motion support.
- **Do** show encrypted-file backup and restore; v1 has no seed phrase.
- **Do** distinguish a verifying full node from block authority and reward roles.
- **Do** pair release downloads with actual app evidence and current preview limits.

### Don't:

- **Don't** replace the approved logo with another eye, star or emblem.
- **Don't** introduce bright red, purple, neon accents or decorative glow.
- **Don't** describe v1 transfers as private, anonymous or ready for real funds.
- **Don't** display passwords, private keys or invented recovery phrases in UI examples.
- **Don't** imply automatic mining, a faucet or decentralized finality.
- **Don't** use promotional graphics or illustrative data as real ledger state.
- **Don't** present stale node observations as live or a preview as a public network.
