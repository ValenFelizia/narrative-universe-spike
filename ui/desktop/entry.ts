import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./style.css";
import { initDesktop } from "./main.ts";
const desktop = initDesktop(invoke);
void listen<string>("request-close", event => desktop.requestClose(event.payload));

import { getCurrentWindow } from "@tauri-apps/api/window";
const windowHandle = getCurrentWindow();
const drag = document.querySelector<HTMLElement>("#window-drag")!;
drag.addEventListener("mousedown", event => { if (event.button !== 0) return; if (event.detail === 2) void windowHandle.toggleMaximize(); else void windowHandle.startDragging(); });
document.querySelector<HTMLButtonElement>("#window-minimize")!.addEventListener("click", () => void windowHandle.minimize());
document.querySelector<HTMLButtonElement>("#window-maximize")!.addEventListener("click", () => void windowHandle.toggleMaximize());
document.querySelector<HTMLButtonElement>("#window-close")!.addEventListener("click", () => void invoke("request_window_close"));
