# Dark checkbox contrast

## Problem

The shared checkbox's dark-mode resting background can override its checked
background. A checked checkbox may therefore render a dark check mark over a
dark translucent surface, making selection difficult to see.

## Design

- Fix the shared checkbox primitive so every consumer receives the correction.
- Preserve Odin's monochrome visual system: checked boxes use `primary` as the
  solid background and `primary-foreground` for the check mark in both themes.
- Ensure the checked state takes precedence over the dark-mode resting
  background.
- Leave unchecked, focus-visible, disabled, and invalid states unchanged.
- Add no new color, dependency, component, prop, or React logic.

## Verification

- Confirm the class cascade gives the checked state precedence in dark mode.
- Run the frontend build/typecheck and lint.
- Run Impeccable's detector once against the changed checkbox component.
