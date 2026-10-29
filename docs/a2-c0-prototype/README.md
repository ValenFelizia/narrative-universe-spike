# A2-C0 disposable visual comparison

Status: preliminary evidence, not C0 approval. This is a standalone design prototype captured from a headed Microsoft Edge window on Windows, not the Tauri app or a WebView2 safety test. The drawn window controls, Save, document list, tab and dialog are illustrative, not operating product controls. The only interactive prototype controls used in the measurement are the library toggle and editable text. No database or real lore is used. The long fixture repeats three invented paragraphs 20 times; it is a length/scroll stress fixture, not literary content or a print-pagination claim.

## Comparisons

All screenshots use CSS pixels at deviceScaleFactor 1. A retains a 230 px reserved library region when hidden. B removes it and recenters the document. C uses a 256 px library and a 48 px top inset, with the alternative fonts. C is a density/font composition, not a controlled font comparison: use the separate font screenshots for that.

| Case | Column x | Width | Lines in short fixture | First line characters |
|---|---:|---:|---:|---:|
| A 800 visible | 262 | 491 | 15 | 55 |
| A 800 hidden | 262 | 491 | 15 | 55 |
| B 800 hidden | 52.5 | 680 | 12 | 80 |
| A 1280 visible / hidden | 407.5 | 680 | 12 | 80 |
| B 1280 hidden | 292.5 | 680 | 12 | 80 |
| C 800 visible | 288 | 465 | 16 | 55 |

These first-line counts are for one specific Spanish paragraph, not a measured average or a claim that the 65–75 target is met. The prototype recorded no document horizontal overflow at the tested viewports. A repeated-text long fixture scrolled to 1200 CSS px at 1024x768, but this is not a test of CodeMirror, print pagination or nested scroll behavior in the real application.

The automated same-window toggle experiment places the caret at text offset 700, scrollTop 400. At 800x600, A retains caret x 464.83, y 260.70 and scrollTop 400; B moves it to x 342.53, y 171.64, keeping the offset and scrollTop. At 1280x600, A retains x 697.53, y 171.64; B moves x to 582.53. This uses contenteditable and a programmatic toggle, so it does not prove keyboard focus behavior on clicking a real button or CodeMirror selection behavior. Raw data is in toggle-results.jsonl.

Recommendation for human evaluation: A protects the existing no-reflow constraint. Its cost is a conspicuous unused region and narrow prose at 800. B uses the space but changes the caret position and wraps, so it would require explicit revision of the invariant. C narrows the minimum layout further and gives more vertical space above the title; its denser reading width and remaining visible library make it a weaker default. All remain visual hypotheses.

## Typography

Each TTF and its corresponding OFL license is included from Google Fonts. The capture script explicitly loads all four faces via the Font Loading API, waits for document.fonts.ready, checks document.fonts.check, and records computed font-family in measurements.json. Source Serif 4 and Literata use 18px/1.65 prose and 32px/1.2 title. Instrument Sans and Source Sans 3 use the same UI sizes. Compare a-1280-visible.png with font-literata.png for serif and with font-source-sans.png for sans. The fixtures include Spanish accents and an emoji; the actual glyph fallback font for the emoji was not identified. Loading a font and matching CSS names does not establish every glyph's fallback or readability. Font selection needs human visual review. Source Serif 4 and Instrument Sans remain provisional baseline choices, not winners of a validated reading study.

## Native risk and outstanding evidence

On origin/main c084560, Rust intercepts CloseRequested and prevents the close, emits a token, and the guarded frontend eventually invokes close_window, which consumes the token before destroying the window. The current main capability only includes event listen and unlisten. Tauri documents decorations:false and separate minimize, toggle-maximize and dragging permissions: https://v2.tauri.app/learn/window-customization/ . Avoid exposing a bypass path and verify the close request path end to end before shipping.

No frameless Tauri shell was built or verified in this experiment. Resize borders, Windows Snap, maximize/restore, dragging, double-click, focus, Alt+F4, close buttons, OS close and dirty-token safety remain unverified. A drawn dialog is not a dirty-guard test. Current main's historical native captures are in docs/screenshots and do not show these alternatives. Native app screenshots of equivalent cases, keyboard interactions and real editor scroll/caret remain outstanding before C0 can be considered complete.

## Reproduction

Use Node with playwright-core installed separately, then run capture.cjs and toggle.cjs on Windows with Microsoft Edge at the path in the scripts. The capture script runs a headed browser; the toggle script runs headless. No server or product code is needed. This is an isolated throwaway experiment, not a shipping interface.
