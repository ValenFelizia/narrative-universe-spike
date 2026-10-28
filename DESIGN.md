---
version: alpha
name: "Narrative Universe / Write Workspace"
description: "Provisional dark-first editorial design system for the first local desktop writing experience (PA-I3/A2)."
colors:
  primary: "#17191C"
  background: "#17191C"
  surface-secondary: "#1F2226"
  page-selected: "#2A2C33"
  text-primary: "#ECE7DE"
  text-secondary: "#ABA9A5"
  divider: "#393B40"
  accent: "#7AA89A"
typography:
  document-title:
    fontFamily: "Source Serif 4"
    fontSize: "32px"
    fontWeight: 600
    lineHeight: 1.2
  prose:
    fontFamily: "Source Serif 4"
    fontSize: "18px"
    fontWeight: 400
    lineHeight: 1.65
  ui:
    fontFamily: "Instrument Sans"
    fontSize: "14px"
    fontWeight: 400
    lineHeight: 1.5
  ui-label:
    fontFamily: "Instrument Sans"
    fontSize: "12px"
    fontWeight: 500
    lineHeight: 1.4
spacing:
  xs: "4px"
  sm: "8px"
  md: "12px"
  base: "16px"
  lg: "24px"
  xl: "32px"
  xxl: "48px"
  sidebar-width: "256px"
rounded:
  none: "0px"
  sm: "4px"
  md: "8px"
components:
  writing-surface:
    backgroundColor: "{colors.background}"
    textColor: "{colors.text-primary}"
    typography: "{typography.prose}"
    rounded: "{rounded.none}"
  document-title:
    backgroundColor: "{colors.background}"
    textColor: "{colors.text-primary}"
    typography: "{typography.document-title}"
  sidebar:
    backgroundColor: "{colors.surface-secondary}"
    textColor: "{colors.text-secondary}"
    typography: "{typography.ui}"
  page-selected:
    backgroundColor: "{colors.page-selected}"
    textColor: "{colors.text-primary}"
    rounded: "{rounded.sm}"
    typography: "{typography.ui}"
  save-status:
    backgroundColor: "{colors.background}"
    textColor: "{colors.text-secondary}"
    typography: "{typography.ui-label}"
---

# Narrative Universe — Design Direction

## Overview

**Status:** Design baseline for PA-I3/A2; provisional tokens for the first visual prototype, not a frozen brand system. Writing this file does **not** authorize a Tauri scaffold, dependencies, implementation, PR merge, or expansion of A2.

**Product principle:** *Narrative Universe is a place to write and explore fictional worlds. While writing, the world supports you without interrupting.*

There may eventually be several workflows over **one universe and one data model**:

- **Write:** focused prose, scenes, dialogue, and notes. This is the **only workspace in scope** for the first desktop window.
- **Explore / Archive:** inspect entities, relations, assertions, provenance, competing claims, and canon. Future work. Information density will be appropriate here when analysis actually requires it.
- **Planning:** a hypothesis for high-level scene, chapter, arc, or entity organization. Not an approved feature, navigation section, or schema.

**First job to validate:** open a *synthetic* local universe, pick a page, write a scene, save intentionally, close, reopen, and find the exact persisted prose. The personal inspiration is writing scenes to discover the narrator of *War Archivist*. Do not use irreplaceable private writing in this prototype.

A scene is simply a **page** for now. Do not invent a Scene or Chapter domain model.

**Experience:** contemporary editorial calm, visually literary rather than an IDE or a fantasy-themed parchment. The interface must be comfortable for a nontechnical writer without explaining programming concepts.

**Decision hierarchy:** approved composition and safety rules prevail over agent creativity; the YAML tokens are the *initial implementation baseline* where a value is specified; documented comparisons are experiments, not permission to silently redesign the app. Change visual tokens only through a reviewable comparison.

## Colors

Dark mode is the design starting point, **not a light UI with inverted colors**.

- **Primary/background:** warm near-black graphite (#17191C), uninterrupted behind the writing column.
- **Secondary surface:** slightly lifted graphite (#1F2226) for the slim sidebar, not a collection of cards.
- **Selected page:** restrained tonal difference (#2A2C33); use shape/position as well as color to indicate selection.
- **Primary text:** soft ivory (#ECE7DE), never stark white body prose.
- **Secondary text:** subdued warm gray (#ABA9A5), legible for labels, navigation, and save feedback.
- **Divider:** low-emphasis #393B40; use sparingly.
- **Accent:** muted green (#7AA89A) for purposeful feedback/focus as appropriate, **not** decoration.

The values in the front matter are a **testable starting palette**. Verify WCAG contrast, selection, caret, focus, hover, disabled text, and real error states on the actual desktop window before claiming accessibility. Warning/error colors are intentionally **not fixed yet**; choose and review them in context instead of introducing arbitrary semantic tokens.

Do not add a decorative gradient, colored glow, or saturated brand color to make the screen feel less empty.

## Typography

The editor should feel like writing prose in a modern book, not editing source code.

- **Prose and title:** a legible proportional serif. Compare **Source Serif 4** (initial token baseline) against **Literata**, at identical size, measure, and line height. The font decision is not final.
- **UI and controls:** a restrained, distinctive sans-serif. Compare **Instrument Sans** (initial token baseline) against **Source Sans 3**, in small navigation labels, controls, dialogs, and save/error messages. **Do not default to Inter**; it was specifically rejected as too generic for this identity.
- **Prose sizing:** start around 17–19 px and line height 1.6–1.75. Tokens select 18 px / 1.65 solely for the first comparison.
- **Headings:** clear editorial hierarchy without oversized hero text; document title should feel integrated with the page.
- **Monospace:** only for technical diagnostics or identifiers when unavoidable; never as the scene body font.

CodeMirror 6 is the *editing engine*, not the visual reference. By default, **no line-number gutter, minimap, syntax colors, line/column status, IDE tabs, encoding labels, or code-toolbar metaphors**. Preserve real editor affordances: selection, keyboard shortcuts, undo/redo, paste, IME composition, caret visibility, and natural scrolling.

Check each candidate font's license and distribution requirements before bundling it. Do not distribute unlicensed font assets.

## Layout

### First desktop window

A small, stable composition:

1. **Quiet top bar:** product identity and current universe, without an oversized toolbar.
2. **Minimal left sidebar:** context/name of the universe, a **Pages** heading, ordered page list, selected item, and a discreet **New page** action. Do not fabricate a folder hierarchy or mode menu.
3. **Central writing area:** integrated editable page title and long-form CodeMirror prose on the same continuous background. No visible paper, card, frame, or right-hand panel.
4. **Save control and status:** easy to find but visually secondary to the page.

The sidebar token starts at **256 px**, within a provisional 240–280 px target. The prose measure should remain around **65–75 characters per line**, with a predictable vertical anchor and generous margins. Those dimensions are hypotheses to test at **1024×768, 1280×800, and 1440×900**; desktop first, no implied mobile scope.

**Layout invariant:** hiding secondary UI, showing save feedback, or returning from concentration must **not shift the prose column, caret, paragraph wraps, or reading position**.

### Empty and long documents

An empty page must still feel intentional: visible title, discoverable insertion point, and at most **one** quiet prompt to begin writing. No onboarding card, illustration, autogenerated suggestion, or motivational copy to fill whitespace.

A short scene should feel anchored, not stranded in an infinite void. A long scene must scroll naturally without a tiny fixed-height editor inside another scrolling panel.

### Concentration (interaction hypothesis, not an A2 save/reopen blocker)

Concentration is an **explicitly enabled state of Write**, never automatic hiding in ordinary Write. Once enabled, typing can fade secondary chrome (initial experiment: ~160–220 ms), leaving a reliable restore control and a keyboard escape path. Respect reduced-motion preferences. The page column and selection must stay physically stable.

Ship the first correct save/reopen loop before treating this polish as mandatory.

## Elevation & Depth

Use **tonal hierarchy, alignment, spacing, and text weight** instead of shadows and floating cards. The writing area is **Option A: continuous background, free of a visible container**.

**B1 is a fallback experiment, not a second concurrent design:** if empty, short, and long-page testing shows that Option A feels unanchored, try an *almost imperceptible tonal difference* under the writing area. No obvious rounded card, border, paper texture, or structural layout change.

Do not solve the fear of an empty screen by filling it with widgets.

## Shapes

Keep geometry quiet and functional. The editor surface has **no bounding card radius**. A 4 px radius may soften a selected row; 8 px is available for a real dialog or input when needed. Avoid pill-shaped controls everywhere, nested rounded rectangles, and ornamental frames.

Use low-contrast separators only where navigation or hierarchy requires them. Interactive elements still need a visible keyboard focus treatment.

## Components

Only specify components that serve a working A2 task. The front-matter component references capture the baseline visual roles, **not** permission to build a component library preemptively.

### Page navigation

Show the current universe, a single Pages section, its page list, a clear selection state, and the create action. Page titles are human-readable. Order deterministically; introduce Recents, search, folders, filters, or entity navigation **only after a real workflow demonstrates a need**.

### Document editor

Title and content should feel part of one document, not separate administrative forms. The reading column should be comfortable for sustained writing. Editing ordinary text must remain visually simple even though the underlying adapter guards UTF-16 positions and Rust page revisions.

A page already containing linked mentions is **read-only in PA-I3**, with a plain-language explanation. The UI must not offer an apparent write affordance that fails only after prolonged editing.

### Save and data safety

- **Manual Save** and a discoverable **Ctrl+S** on Windows; no autosave in this increment.
- Distinct, understated **saved**, **unsaved**, **saving**, **stale/conflict**, **busy**, and **error** states. Never show an unsupported success animation or a celebratory toast.
- A writer can continue typing while a save runs. Acknowledgement belongs to the **sent snapshot**, and later edits remain dirty; do not replace the live draft with the returned body.
- Block unsafe page changes/closing while dirty with an explicit **continue editing / discard / save then proceed** choice. Proceed after Save only if persistence succeeds.
- On stale, Busy, failure, or uncertain completion, **keep the draft** and offer an understandable recovery path (including copy, where helpful). Never silently normalize, rebase, discard, or overwrite it.
- Show a concrete read-only reason for linked mentions, CR/CRLF/mixed line endings, NUL, invalid Unicode, or exhausted/unrepresentable revisions. Refuse text transformations that could change persisted content unnoticed.
- Real-data safety remains limited: **no crash/force-kill draft recovery guarantee**, no supported backup workflow yet. Use only synthetic/replaceable universes until a separate data-safety gate.

Rust/SQLite owns persistence. All edits go through the reviewed CodeMirror adapter and revision-checked domain operations, not direct SQL from the webview.

### Errors and dialogs

Use concise human language with optional technical detail, visible keyboard focus, and safe default actions. Dialogs are for **real decisions**, not decorative overlays. Error/warning visual tokens remain to be calibrated in actual states.

### Future modes are not current components

Do **not** ship fake Explore, Planning, AI, entity-peek, or graph controls in A2. A richer archive view may eventually be information-dense because its job is analysis; Write stays quiet.

## Do's and Don'ts

### Do

- Maintain literary hierarchy, stable text geometry, restrained chrome, and a genuinely polished dark experience.
- Favor whitespace, legibility, keyboard behavior, honest states, and real navigation over decoration.
- Show **screenshots from the running application**, not polished mockups alone, when asking for frontend approval.
- Test **empty / three-paragraph / long** pages; visible/hidden sidebar; 1024×768, 1280×800, and 1440×900; dirty/saving/stale/error/read-only states.
- Test basic opening, writing, saving, and finding the page again with a **non-programmer**, without instructing them where to click.
- Compare the two serif and two sans candidates on the **same** screen/content, not isolated typography specimens.
- If proposing a new component or section, explain the actual user job and ask for a design decision before changing the composition.

### Don't

- Add dashboard cards, productivity metrics, empty analytics, a fake folder tree, or three layers of navigation.
- Add gradients, glassmorphism, glows, dramatic shadows, pulse dots, shiny chips, AI badges, or stock illustrations.
- Add an inspector/sidebar on the right, placeholders for future modes, synthetic AI actions, or unused controls.
- Add heavy toolbar chrome, status-bar jargon, excessive microcopy, welcome tutorials, or animation that shifts the text.
- Treat a passing unit test as visual approval. Treat an attractive screenshot as a data-safety approval.
- Interpret open questions as authorization for an agent to choose the product direction.

### Reviewable experiments and authority

These are **questions to settle by implementation evidence**, not prerequisites to opening a technical A2 proof:

| Open question | Comparison | Decision criterion |
| --- | --- | --- |
| Prose serif | Source Serif 4 vs Literata | Sustained readability in Spanish, rhythm, accents, special characters |
| UI sans | Instrument Sans vs Source Sans 3 | Small-label clarity, editorial character, compatibility with serif |
| Surface | A, then B1 only if necessary | Empty, short, and long writing comfort without a sense of void |
| Save location | Two restrained placements | Discoverable without interrupting prose; Ctrl+S works |
| Page ordering | Simple deterministic list | Add Recents only when actual navigation warrants it |
| Concentration | Stable hide/restore trial | No text shift, lost selection, keyboard trap, or forced animation |

An implementing agent may create **comparable experiments** but may not approve its own visual direction. Review each consequential change with Valen and Ari. The initial technical A2 proof (Tauri/IPC) and the later working editor are separately scoped gates; **this design file alone authorizes neither**.

**Reference format:** Google Labs' DESIGN.md specification (YAML design tokens + human-readable sections). Project decisions and safety constraints remain authoritative even if tooling supports additional tokens or components.
