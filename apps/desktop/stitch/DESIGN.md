# Runlith design system

Runlith is a local desktop control room for AI coding agents. Dark first, calm, dense, honest about where data comes from.

## Typography
- Geist for all UI; Geist Mono for ids, paths and numbers in tables. No other fonts, no italics, weight at most 600.
- Sizes (px / line height): 12/16 caption, 14/20 body, 16/24 title, 20/28 section, 24/32 page title, 36/40 hero.
- Sentence case. UI copy in Russian, without hyphens; identifiers (TASK-0217, ipc-contracts) stay as written.

## Color, dark (default)
- Window canvas #000000 with 1px dots every 16px at rgba(255,255,255,0.07).
- Surfaces: tile #181818, card #1F1F1F, control #272727, line #313131.
- Text #F2F2F2, secondary #A6A6A6.
- Accent #186EF3 for fills (white text on it), #8AB4FF for accent text and lines.
- Amber #F2B45A = needs a human decision. Red #FF8F87 = error or deny. Done #EDEDED.

## Color, light
- Canvas #E9E9EC, tile #FFFFFF, card #F5F5F7, control #EBEBEF, line #DEDEE3.
- Text #111114, secondary #5A5A63, accent #186EF3, accent text #1456C4, amber #8A5600, red #B3261E, done #26262B.
- Every text pair meets 4.5:1.

## Shape and space
- Spacing only 2, 4, 8, 12, 16, 24, 32, 40, 48.
- Radius: tiles 24, cards 16, rows 8, buttons and chips fully rounded. Nested radius = outer radius minus the gap.
- The window is floating tiles on the dark canvas with 8px gaps: sidebar 232, main, optional agent panel 360.
- Borders go all around or not at all.

## Status language
Shape carries the meaning, colour only reinforces it.
- done: filled dot; ready: accent ring; running: filled accent dot with a soft halo; waiting: dashed ring; needs decision: amber diamond; failed: red square.
- Inferred by the agent: dashed border plus confidence 0,00 to 1,00. Verified by a human: solid border.
- A dashed "демо" chip marks anything that does not come from the agentd daemon.

## Motion
- Easing cubic-bezier(0.22, 1, 0.36, 1); layout 500 ms, colour 300 ms; animate only transform and opacity.
- Content enters by rising 24px with a 60 ms stagger. Reduced motion removes all movement.
- The aurora glow (blue #186EF3, violet #7C5CFF, teal #22C3B5) lives only behind the agent message box and brightens while the agent works.

## Icons
Phosphor regular at 20px and 16px, painted with the text colour.
