import type { invoke } from "@tauri-apps/api/core";

type PageIndexEntry = { id: string; title: string };
type PageContent = { id: string; title: string; body: string };
type BridgeError = { kind: string; message?: string };

export function initDesktop(invokeCommand: typeof invoke): void {
  const openForm = document.querySelector<HTMLFormElement>("#open-form")!;
  const pathInput = document.querySelector<HTMLInputElement>("#database-path")!;
  const universeName = document.querySelector<HTMLParagraphElement>("#universe-name")!;
  const pageCount = document.querySelector<HTMLSpanElement>("#page-count")!;
  const pageList = document.querySelector<HTMLUListElement>("#page-list")!;
  const status = document.querySelector<HTMLParagraphElement>("#status")!;
  const pageTitle = document.querySelector<HTMLHeadingElement>("#page-title")!;
  const readingArea = document.querySelector<HTMLElement>(".reading-area")!;
  const pageBody = document.querySelector<HTMLDivElement>("#page-body")!;
  const openButton = openForm.querySelector<HTMLButtonElement>("button")!;
  let selectionVersion = 0;
  let opening = false;

  function errorKind(error: unknown): string {
    return typeof error === "object" && error !== null && "kind" in error
      ? String((error as BridgeError).kind)
      : "read_failed";
  }

  function explain(error: unknown): string {
    switch (errorKind(error)) {
      case "empty_path": return "Enter the full path to an existing synthetic database.";
      case "database_missing": return "That database file could not be found. Check the path and try again.";
      case "invalid_database": return "This file is not a valid Narrative Universe database.";
      case "unsupported_database": return "This database version is not supported by this desktop build.";
      case "universe_not_open": return "Open a synthetic universe before selecting a page.";
      case "page_missing": return "That page is no longer available. Reopen the universe to refresh the list.";
      case "state_unavailable": return "The database connection is unavailable. Restart the app and reopen the file.";
      default: return "The database could not be read. Check the file and try again.";
    }
  }

  function clearDocument(message: string): void {
    pageTitle.textContent = "No page selected";
    pageBody.textContent = message;
  }

  async function selectPage(id: string): Promise<void> {
    const version = ++selectionVersion;
    for (const item of pageList.querySelectorAll<HTMLButtonElement>("button")) {
      item.setAttribute("aria-current", String(item.dataset.pageId === id));
    }
    status.textContent = "Reading page…";
    try {
      const page = await invokeCommand<PageContent>("read_page", { pageId: id });
      if (version !== selectionVersion) return;
      readingArea.scrollTop = 0;
      pageTitle.textContent = page.title;
      pageBody.textContent = page.body;
      status.textContent = "Read-only";
    } catch (error) {
      if (version !== selectionVersion) return;
      clearDocument(explain(error));
      status.textContent = explain(error);
    }
  }

  async function refreshIndex(): Promise<void> {
    const pages = await invokeCommand<PageIndexEntry[]>("page_index");
    pageList.replaceChildren();
    pageCount.textContent = String(pages.length);
    if (pages.length === 0) {
      clearDocument("This universe has no pages yet.");
      status.textContent = "No pages in this universe.";
      return;
    }
    for (const page of pages) {
      const item = document.createElement("li");
      const button = document.createElement("button");
      button.type = "button";
      button.className = "page-link";
      button.textContent = page.title;
      button.dataset.pageId = page.id;
      button.addEventListener("click", () => void selectPage(page.id));
      item.append(button);
      pageList.append(item);
    }
    await selectPage(pages[0]!.id);
  }

  openForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (opening) return;
    opening = true;
    openButton.disabled = true;
    const databasePath = pathInput.value;
    ++selectionVersion;
    pageList.replaceChildren();
    pageCount.textContent = "";
    universeName.textContent = "Opening selected file…";
    clearDocument("Loading the selected universe.");
    status.textContent = "Opening database…";
    try {
      await invokeCommand("open_universe", { databasePath });
      universeName.textContent = databasePath.trim().split(/[\\/]/).at(-1) || databasePath.trim();
      await refreshIndex();
    } catch (error) {
      universeName.textContent = "No universe open";
      clearDocument(explain(error));
      status.textContent = explain(error);
    } finally {
      opening = false;
      openButton.disabled = false;
    }
  });

}
