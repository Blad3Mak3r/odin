---
name: Odin
description: The dashboard for a self-hosted multi-game server fleet
colors:
  paper: "oklch(1 0 0)"
  ink: "oklch(0.145 0 0)"
  charcoal: "oklch(0.205 0 0)"
  charcoal-foreground: "oklch(0.985 0 0)"
  mist: "oklch(0.97 0 0)"
  slate: "oklch(0.556 0 0)"
  rule: "oklch(0.84 0 0)"
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
    fontWeight: 600
    lineHeight: 1.4
    letterSpacing: "0.04em"
  mono:
    fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace"
    fontSize: "0.75rem"
    fontWeight: 400
    lineHeight: 1.5
rounded:
  sm: "0px"
  md: "0px"
  lg: "0px"
  xl: "0px"
  2xl: "0px"
  3xl: "0px"
  4xl: "0px"
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
  badge-default:
    backgroundColor: "{colors.charcoal}"
    textColor: "{colors.charcoal-foreground}"
    rounded: "{rounded.lg}"
    height: "1.25rem"
  card:
    backgroundColor: "{colors.paper}"
    borderColor: "{colors.rule}"
    rounded: "{rounded.xl}"
    padding: "{spacing.md}"
  input:
    textColor: "{colors.ink}"
    borderColor: "{colors.rule}"
    rounded: "{rounded.lg}"
    height: "2rem"
    padding: "0.25rem 0.625rem"
---

# Design System: Odin

## Overview

**Creative North Star: "The Operations Panel"**

Odin is a self-hosted control panel for independent game servers. An
administrator needs to scan live state, find the relevant server, and act
without mistaking routine information for an alert. The dashboard therefore
uses a rectilinear panel language: matte neutral surfaces, 1px rules,
compact controls, and deliberate empty space. It is assertive through
structure, not decoration.

The existing Odin mark (`web/public/logo.png`) remains the product signature.
Its painterly cyan and amber language does not enter the interface system.

## Colors

The interface is nearly achromatic in both themes. Light mode uses paper and
ink; dark mode inverts the surfaces while preserving the same rule hierarchy.
`--border` is intentionally more visible than a decorative hairline so a
panel's boundary reads at a glance. There are no decorative gradients,
textures, or accent colors; the one exception is the game atmosphere described
under Layout and shape.

- **Paper / Ink:** primary surfaces and content in light mode.
- **Charcoal / Charcoal Foreground:** the primary action and its text.
- **Mist / Slate:** subdued fills and secondary text.
- **Rule:** the shared 1px boundary for panels, fields, tables, and control
  groups.
- **Focus Ring:** neutral visible keyboard focus around interactive controls.
- **Alarm, Signal OK, Signal Warn:** the only colors used for destructive,
  failed, passing, or warning states.

**Signal-only rule.** Routine running and stopped states stay neutral. Color
reports a meaningful operational fact, never a route, button, or decoration.
Game banner art is atmosphere, not a signal, and never encodes state.

## Typography and data

Geist Variable is the sole interface typeface. Hierarchy comes from scale and
weight: page headings are 1.5rem/600, panel titles are 1rem/500, and body text
is 0.875rem. Compact labels use the 0.75rem label role with 0.04em tracking
and uppercase only when identifying a bounded datum, such as a table column
or status badge. Monospace remains reserved for raw logs, IDs, and code.

Tables use tabular figures so ports, resource readings, and timestamps align
as measurements rather than decoration.

## Layout and shape

The application uses a fixed `w-56` sidebar above `md` and a drawer below it.
The content area reaches `max-w-[100rem]` with responsive page padding. Lists
remain tables; cards are for distinct dashboard summaries and detail groups,
not repeated records.

Every ordinary radius token is `0px`. Cards, dialogs, inputs, menus, badges,
tabs, image frames, and ad-hoc containers are square. The switch track and
thumb remain circular because that shape expresses a physical toggle, not a
decorative container.

Depth is never simulated with a shadow. A 1px rule plus an adjacent surface
change establishes every panel. Browser surfaces belong to the system too:
selection uses the primary pairing, scrollbars use the rule color, and focus
remains visible in either theme.

### Game atmosphere

A game's banner (`web/src/assets/game-banners`) may appear in exactly two
ways: as a thumbnail strip on its card in the games list, and as a page
backdrop behind the game and instance headers. The backdrop is anchored to
the top of the page and fades downward into `--background` (a scrim built
from that token, so it holds in both themes), reaching the plain page
surface within the first screenful. Past the headers, anything that overlaps
the backdrop (tabs, the first panel, a console) sits under a scrim of at least
88%, so reading and controls never depend on the art. The backdrop never
carries state or replaces a rule.

## Components

### Panels and dialogs

Cards, alerts, dialogs, popovers, console output, and local settings groups
are bordered rectangular panels. Card and dialog footers use a muted fill and
a top rule; headers, content, and footers align to the same panel edge.

### Controls

Buttons and inputs are compact rectangles. Primary actions are solid
charcoal; outline and secondary actions use rules or subdued fills. Destructive
actions use alarm text and a low-opacity alarm fill. Disabled and loading
states retain their current behavior and focus always receives the neutral
ring.

Badges are compact rectangular labels with a border, uppercase label role,
and no capsule geometry. They communicate status without competing with the
record around them.

### Navigation and tabs

Sidebar rows are flat. The active route has a 2px neutral left rule and a
subtle background field; hover only strengthens that same relationship. The
theme selector is a ruled three-way control, not a rounded cluster.

Tabs are always line navigation. The active item receives a foreground bottom
rule (or right rule in a vertical list), while inactive items remain on the
shared baseline. This holds for page and nested tabs so selection is clear
without stacked containers.

### Tables and signals

Tables use uppercase compact headers, horizontal row rules, and a muted hover
fill. They do not use zebra stripes, cards, or rounded wrappers. Health rows
pair a neutral description with an icon in Signal OK, Signal Warn, or Alarm;
the color remains confined to that fact.

## Do and don't

Do:

- Use borders and whitespace to establish hierarchy.
- Keep route, status, focus, error, disabled, loading, and empty states
  legible in both themes.
- Preserve square geometry across shared and one-off components.

Don't:

- Add shadows, soft cards, pill badges, or rounded control groups.
- Use the logo's palette or neon effects, or use game imagery anywhere beyond
  the game atmosphere above.
- Use mono as a generic technical costume or color to signify steady state.
