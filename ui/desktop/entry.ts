import { invoke } from "@tauri-apps/api/core";
import "./style.css";
import { initDesktop } from "./main.ts";
initDesktop(invoke);
