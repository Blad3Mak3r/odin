# Sidebar version placement

## Goal

Move Odin's version from the sidebar footer into the product wordmark so the
application identity reads as `Odin vX.X.X` at the top of the navigation.

## Design

- Preserve the existing Odin logo, wordmark, subtitle, navigation, colors, and
  spacing system.
- Render the version inline immediately after `Odin` in both the desktop
  sidebar and mobile header.
- Keep `Odin` at its current semibold title size. Render the version as neutral
  metadata using the design system's label role: `text-xs`, regular weight,
  and muted foreground color, aligned to the wordmark baseline.
- Keep only the version text linked to `/changelog`, including its accessible
  label, keyboard focus ring, hover treatment, and active-route treatment.
- When `VITE_ODIN_VERSION` is absent, render only `Odin` without an empty link
  or layout gap.
- Remove the old version link from the sidebar footer. The footer continues to
  contain the theme toggle and activity button.
- Add no new color, badge, divider, shadow, animation, or component abstraction.

## Responsive behavior

Desktop retains the subtitle `Valheim server dashboard` below the wordmark.
Mobile remains a compact top bar and gains the same inline version treatment.
The version must not wrap independently from `Odin` at supported widths.

## Verification

- Run the frontend build/typecheck and lint.
- Run Impeccable's detector once against the changed component.
- Confirm the version is present once in desktop navigation and once in the
  alternate mobile header, remains a changelog link, and disappears cleanly
  when the build-time version is unavailable.
