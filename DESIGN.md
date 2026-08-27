---
name: Odin
description: The dashboard for a self-hosted Valheim server fleet
colors:
  paper: "oklch(1 0 0)"
  ink: "oklch(0.145 0 0)"
  charcoal: "oklch(0.205 0 0)"
  charcoal-foreground: "oklch(0.985 0 0)"
  mist: "oklch(0.97 0 0)"
  slate: "oklch(0.556 0 0)"
  hairline: "oklch(0.922 0 0)"
  focus-ring: "oklch(0.708 0 0)"
  alarm: "oklch(0.577 0.245 27.325)"
  signal-ok: "#10b981"
  signal-warn: "#f59e0b"
typography:
  heading:
    fontFamily: "'Geist Variable', sans-serif"
    fontSize: "1.5rem"
    fontWeight: 600
    lineHeight: 1.2
    letterSpacing: "-0.01em"
  title:
    fontFamily: "'Geist Variable', sans-serif"
    fontSize: "1rem"
    fontWeight: 500
    lineHeight: 1.375
  body:
    fontFamily: "'Geist Variable', sans-serif"
    fontSize: "0.875rem"
    fontWeight: 400
    lineHeight: 1.5
  label:
    fontFamily: "'Geist Variable', sans-serif"
    fontSize: "0.75rem"
    fontWeight: 400
    lineHeight: 1.4
  mono:
    fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace"
    fontSize: "0.75rem"
    fontWeight: 400
    lineHeight: 1.5
rounded:
  sm: "0.375rem"
  md: "0.5rem"
  lg: "0.625rem"
  xl: "0.875rem"
  2xl: "1.125rem"
  3xl: "1.375rem"
  4xl: "1.625rem"
spacing:
  xs: "0.5rem"
  sm: "0.75rem"
  md: "1rem"
  lg: "1.5rem"
  xl: "2rem"
components:
  button-primary:
    backgroundColor: "{colors.charcoal}"
    textColor: "{colors.charcoal-foreground}"
    rounded: "{rounded.lg}"
    height: "2rem"
    padding: "0 0.625rem"
  button-primary-hover:
    backgroundColor: "{colors.charcoal}"
  button-secondary:
    backgroundColor: "{colors.mist}"
    textColor: "{colors.charcoal}"
    rounded: "{rounded.lg}"
    height: "2rem"
  badge-default:
    backgroundColor: "{colors.charcoal}"
    textColor: "{colors.charcoal-foreground}"
    rounded: "{rounded.4xl}"
    height: "1.25rem"
  badge-secondary:
    backgroundColor: "{colors.mist}"
    textColor: "{colors.charcoal}"
    rounded: "{rounded.4xl}"
    height: "1.25rem"
  card:
    backgroundColor: "{colors.paper}"
    rounded: "{rounded.xl}"
    padding: "{spacing.md}"
  input:
    textColor: "{colors.ink}"
    rounded: "{rounded.lg}"
    height: "2rem"
    padding: "0.25rem 0.625rem"
  checkbox:
    backgroundColor: "{colors.charcoal}"
    textColor: "{colors.charcoal-foreground}"
    rounded: "{rounded.sm}"
    size: "1rem"
---

# Design System: Odin

## Overview

**Creative North Star: "The All-Father's Ledger"**

Odin's dashboard is a record-keeping tool, not a showcase. It exists so an
admin running several Valheim servers for other people can see, at a glance,
exactly what's true right now — which instances are up, who's connected,
what a job is doing — and act on it without ceremony. The visual system
reflects that: a near-monochrome charcoal-on-paper (and paper-on-charcoal in
dark mode) surface, Geist Variable as the one typeface doing all the work,
flat ring-bordered cards, and color spent nowhere except where it reports a
fact the admin needs to notice. This is a deliberate, confirmed identity —
not a placeholder waiting for a brand pass.

The mark (`web/public/logo.png`) — a painterly, dark-stone Odin figure with
glowing cyan runes and molten amber accents — lives outside this system on
purpose. It signs the product; it does not style it. The dashboard's
restraint and the mark's illustrated intensity are two different registers
by design, not an inconsistency to reconcile.

**Key Characteristics:**
- Near-zero-chroma neutral surface (charcoal/paper), one typeface, one radius scale.
- Color is reserved for status signals (ok / warning / critical) and never used decoratively.
- Depth comes from a hairline ring border, never a shadow.
- The illustrated brand mark is deliberately kept out of the UI's visual language.

## Colors

Almost the entire interface is drawn from a single achromatic scale; the only
hues in the system are the three status signals, and they appear nowhere
else.

### Primary
- **Charcoal** (`oklch(0.205 0 0)`): the near-black surface for primary
  buttons, default badges, and active/selected states. Its foreground is
  **Charcoal Foreground** (`oklch(0.985 0 0)`), a near-white.

### Neutral
- **Paper** (`oklch(1 0 0)`): base background, card, and popover surface.
- **Ink** (`oklch(0.145 0 0)`): default body and heading text.
- **Mist** (`oklch(0.97 0 0)`): secondary/muted/accent surface — table
  header washes, secondary badges, dialog footers, hover fills.
- **Slate** (`oklch(0.556 0 0)`): muted/secondary text — descriptions,
  metadata, placeholder copy.
- **Hairline** (`oklch(0.922 0 0)`): the one border color in the system —
  card rings, table dividers, input outlines.
- **Focus Ring** (`oklch(0.708 0 0)`): the focus-visible ring color, at
  reduced opacity.

Dark mode inverts the paper/charcoal relationship (paper becomes
`oklch(0.145 0 0)`, primary becomes a near-white `oklch(0.922 0 0)`) but
keeps the scale entirely achromatic — no dark-mode-only accent hue is
introduced.

### Signal colors (status only)
- **Alarm** (`oklch(0.577 0.245 27.325)`, Tailwind's `destructive` token):
  critical failures, destructive actions, failed jobs.
- **Signal OK** (`#10b981`, Tailwind `emerald-500`): a passing dependency
  or health check.
- **Signal Warn** (`#f59e0b`, Tailwind `amber-500`): a non-critical problem
  worth the admin's attention but not blocking.

### Named Rules
**The Signal-Only Rule.** Alarm, Signal OK, and Signal Warn exist to report
a fact about system state (a dependency check, a job outcome) — never to
decorate a button, a nav item, or routine content. An instance's own
running/stopped state deliberately stays inside the neutral scale (`default`
vs. `secondary` badge, not green vs. red): "running" is the expected steady
state, not an alert.

**The One Register Rule.** The logo's cyan/amber/painterly language never
enters component color, icon style, or illustration. If a screen needs to
feel more "alive," reach for typography, density, or motion — not the mark's
palette.

## Typography

**Body/Display Font:** Geist Variable (with `sans-serif` fallback) — the
only typeface in the system; there is no separate display or heading face.

**Character:** A single, versatile grotesk carries every role from a page
title to a table cell. Hierarchy is built entirely with size, weight, and
tracking, not typeface switching.

### Hierarchy
- **Heading** (600, 1.5rem/`text-2xl`, tight tracking): page titles ("Instances", "Dependency status").
- **Title** (500, 1rem/`text-base`, snug line-height): card and dialog titles.
- **Body** (400, 0.875rem/`text-sm`): default UI text — table cells, form labels, descriptions.
- **Label** (400, 0.75rem/`text-xs`): secondary metadata — sidebar subtitle, version tag, mobile-hidden column labels.
- **Mono** (system monospace stack, 0.75rem–0.875rem): reserved for raw technical content only — console/log output and Steam IDs, never UI chrome.

### Named Rules
**The One Typeface Rule.** Never introduce a second font family for emphasis or "personality" — weight and size carry that job, keeping the ledger feel consistent everywhere it's read.

## Layout

A single fixed sidebar (`w-56`, collapsing to a slide-in drawer under `md`)
plus a centered content column capped at `max-w-5xl`, with `px-4 py-6`
padding on mobile widening to `px-6 py-8` at `sm` and up. Content is
predominantly tabular (instance lists, job lists) or card-grid (dashboard
summaries), never a wide, low-density canvas — the ledger stays narrow and
scannable rather than spreading out. Breakpoints follow Tailwind defaults:
`sm` (640px) and `md` (768px) are the two that actually gate layout changes
(mobile drawer nav, column visibility in tables, dialog width).

## Elevation & Depth

**Flat by design.** No `box-shadow` appears anywhere in the implemented
system. Every surface that needs to read as "raised" — cards, dialogs,
dropdowns — is separated from its background with a 1px ring
(`ring-1 ring-foreground/10`) and, where relevant, a background-color step
(paper vs. mist), never a shadow. This is an invariant, not a placeholder:
a ledger doesn't need to fake physical depth to be legible.

### Named Rules
**The No-Shadow Rule.** Depth is a border and a background-color
relationship, never `box-shadow`. A component that "needs to pop" gets a
ring and a fill change, not elevation.

## Shapes

A single radius scale (`--radius: 0.625rem` base) drives every corner in the
system, scaled up or down by role rather than hand-picked per component:
compact controls (small buttons, icon buttons, the theme toggle cluster,
icon avatars) use the smaller steps (`0.375rem`–`0.5rem`), every bordered
*container* — card, dialog, console/log panel, and any ad hoc data row or
box — uses `0.875rem` (`rounded-xl`), and pill-shaped elements (badges) use
the largest step (`1.625rem`) against a short enough height to read as
fully rounded. Borders are hairline (1px) and low-contrast; there is no
double-border, no dashed or decorative border style anywhere in the system.

### Named Rules
**The Container Radius Rule.** Any bordered box that holds content —
whether it's the `Card` component or a one-off `rounded-xl border` div for
a list row — uses the same `0.875rem` corner as Card and Dialog. A
container never invents its own, smaller radius; only true controls
(buttons, icons, toggles) use the tighter steps.

## Components

### Buttons
- **Shape:** rounded corners (`0.625rem` default, tighter at `sm`/`xs`
  sizes), 1px transparent border, compact height (`2rem` default).
- **Primary:** Charcoal background, Charcoal Foreground text; hover drops
  opacity to 80%. Used for the single dominant action per view (Start,
  Create).
- **Secondary:** Mist background, Charcoal text. Used for supporting actions
  inside a form footer.
- **Outline:** transparent/paper background with a Hairline border; hover
  fills Mist. The default choice for reversible per-row actions (Restart,
  Stop).
- **Ghost:** no border or fill at rest; hover fills Mist. Used for icon-only
  chrome actions (menu toggle, close, notification bell).
- **Destructive:** Alarm text on a low-opacity Alarm fill (`10%`/`20%` on
  hover) rather than a solid red button — a deliberately quieter treatment
  than Primary, so destructive actions read as serious without shouting.

### Badges
- **Shape:** fully rounded pill (`1.625rem` radius against `1.25rem`
  height), no border.
- **Default:** Charcoal/Charcoal Foreground — used for a "true/positive"
  state (`running`).
- **Secondary:** Mist/Charcoal — the neutral/"false" or informational state
  (`stopped`, a plain count).
- **Destructive:** low-opacity Alarm fill — reserved for a `failed` job
  status, matching the destructive button's restraint.

### Cards / Containers
- **Corner Style:** `0.875rem` radius.
- **Background:** Paper (Card surface token), no gradient or texture.
- **Depth:** 1px `foreground/10` ring; no shadow (see Elevation & Depth).
- **Internal Padding:** `1rem` default, `0.75rem` in the compact (`sm`) size
  variant.
- **Footer:** when present, a Mist background with a top border and the
  same corner radius mirrored on the bottom edge only.

### Inputs / Fields
- **Style:** transparent background, `0.625rem`-radius Hairline border,
  `2rem` height.
- **Focus:** border shifts to Focus Ring color plus a 3px ring at 50%
  opacity — no glow, no color change beyond the neutral scale.
- **Error:** border and ring shift to Alarm at reduced opacity
  (`aria-invalid` state) — the only place an input touches a signal color.
- **Disabled:** reduced opacity plus a faint Mist fill.
- **Checkbox:** a `1rem` square, `0.125rem`-radius box — Hairline border at
  rest, filled Charcoal/Charcoal Foreground with a check glyph once ticked.
  Same focus-ring treatment as every other control. Reserved for a list of
  independent selections (e.g. picking which instances to install a mod
  on); `Switch` stays the control for a single on/off setting.

### Tabs
- **Outer/page-level tabs** (the instance detail page's Logs/Config/Mods/…
  strip, a page's own top-level Installed/Marketplace split): the `default`
  variant — a Mist pill container with an active tab lifted onto a Paper
  chip.
- **Nested tabs** (a tab strip that lives inside another tab's content,
  e.g. Mods' Installed/Marketplace or Access Lists' Admins/Banned/Permitted
  once you're already inside the Mods or Access Lists tab): the `line`
  variant — no container fill, an underline on the active tab instead of a
  filled chip. This is the one place visual weight is deliberately reduced
  a step, so a second tab strip never gets confused for the page's primary
  navigation.

### Section Labels
Small in-page section headings ("Installed mods", "Configuration files",
"Search Thunderstore") use the Label typographic role: `0.75rem`, uppercase,
tracked, Slate-colored — not a heavier weight or larger size. They mark a
subsection without competing with the page Heading or a Card Title.

### Error State
Every query error (`QueryError`) renders as a destructive `Alert`: an
icon plus message on a Paper surface with a Hairline border, not bare red
text. One component, one treatment, used identically on every page.

### Tables
- Hairline row dividers, no zebra striping, no header background beyond a
  bottom border. Columns hide progressively at `sm`/`md` rather than
  wrapping or scrolling on small screens. The dominant list pattern in the
  system (instances, jobs) — cards are reserved for dashboard summaries and
  detail-page groupings, not for listing many similar records.

### Navigation (Sidebar)
- **Style:** flat list of icon + label rows, `0.5rem` radius, no dividers.
- **Default:** 70%-opacity Sidebar Foreground text, no fill.
- **Hover / Active:** Sidebar Accent fill and full-opacity Sidebar Accent
  Foreground text — identical treatment for hover and active, so the active
  state reads as "you're already here," not a separate visual language.
- **Mobile:** the same list inside a slide-in drawer (`w-64`, translate
  transition), triggered from a top bar that otherwise carries only the
  wordmark and the drawer toggle.

### Status Signal (signature pattern)
The one place color routinely appears: a dependency/health row pairs an
icon or label with text colored Signal OK, Signal Warn, or Alarm depending
on outcome — always text-only, never a filled background, and always next
to a neutral-toned description of what's being checked. This keeps the
signal legible without turning the whole row into an alert banner.

## Do's and Don'ts

### Do:
- **Do** keep color confined to the three signal roles (Alarm, Signal OK,
  Signal Warn) and use them only to report an actual state, never to
  decorate.
- **Do** build hierarchy with size, weight, and tracking on the single
  Geist Variable family — never introduce a second typeface.
- **Do** use a 1px Hairline ring plus a background-color step for anything
  that needs to read as "raised."
- **Do** treat a routine instance's `running` state as the neutral default
  badge, not a colored one — reserve color for things that need attention.
- **Do** keep the destructive button/badge treatment quiet (low-opacity
  fill on Alarm text), matching the system's overall restraint.

### Don't:
- **Don't** pull the logo's cyan/amber/painterly illustration style into
  any UI component, icon, or chrome element — the mark and the interface
  are deliberately different registers.
- **Don't** drift toward a neon/gradient/"gaming rig" aesthetic anywhere in
  the dashboard, even though the product manages a game server.
- **Don't** add `box-shadow` to any surface — depth is a border and fill
  relationship in this system, not elevation.
- **Don't** color-code an instance's own running/stopped state (or any
  other expected, non-alerting state) — that dilutes the signal colors'
  meaning everywhere else.
