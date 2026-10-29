# Issue 25 native smoke

Working tree HEAD 1bee8ef91b2f73c700cc625d4ace3bf65516b512, with pre-existing uncommitted product changes. Windows native Tauri/WebView2, disposable synthetic.sqlite created from src/schema.sql with user_version=2 and random universe ID.

Build: `cd ui; npm run desktop:build -- --no-bundle` succeeded, release executable ui/src-tauri/target/release/narrative-universe-desktop.exe. One unused variable warning in src/migration.rs. Fixture created with Node node:sqlite; Python command unavailable on this machine.

Opened empty fixture, New page, entered title `Canción 🌙`, body `Café y corazón ✨\nSegunda línea 🐉`, Ctrl+S, observed Saved. Renamed to `Renombrada 🐉`, Ctrl+S, observed Saved and updated list. Changed body, clicked New, observed guard; Continue editing retained draft and page count 1. Closed window, guard appeared, explicitly discarded draft. Relaunched and reopened fixture, observed one page with renamed title and both body lines, Saved. SQLite read `select title,body,revision from pages` yielded one row, exact title `Renombrada 🐉`, body `Café y corazón ✨\nSegunda línea 🐉`, revision 2.

Keyboard: Tab then Enter opened universe after path entry; Ctrl+S worked for body and title. No exhaustive keyboard audit. Attempted resize by dragging lower right, window did not resize; minimum window behavior unverified. Screenshots guard.png and reopened.png are native window captures. No product code changes, commit, PR, or merge made by tester.
