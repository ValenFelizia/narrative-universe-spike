import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./style.css";
import { initDesktop } from "./main.ts";
const desktop = initDesktop(invoke);
void listen<string>("request-close", event => desktop.requestClose(event.payload));
