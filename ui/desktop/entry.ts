import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./style.css";
import { initDesktop } from "./main.ts";
const desktop = initDesktop(invoke);
void listen<string>("request-close", event => desktop.requestClose(event.payload));

import {getCurrentWindow} from "@tauri-apps/api/window";
const win=getCurrentWindow();
document.querySelector<HTMLButtonElement>("#library-toggle")!.onclick=()=>{document.body.classList.toggle("collapsed");document.querySelector("#library-toggle")!.setAttribute("aria-expanded",String(!document.body.classList.contains("collapsed")));};
document.querySelector<HTMLSelectElement>("#experiment-layout")!.onchange=(ev)=>{document.body.classList.remove("layout-A","layout-B","layout-H");document.body.classList.add("layout-"+(ev.target as HTMLSelectElement).value);};
document.body.classList.add("layout-A");
document.querySelector<HTMLButtonElement>("#minimize")!.onclick=()=>void win.minimize();
document.querySelector<HTMLButtonElement>("#maximize")!.onclick=()=>void win.toggleMaximize();
document.querySelector<HTMLButtonElement>("#close")!.onclick=()=>void invoke("request_window_close");
import {LogicalSize} from "@tauri-apps/api/dpi";
document.querySelector<HTMLSelectElement>("#experiment-size")!.onchange=(ev)=>{const n=(ev.target as HTMLSelectElement).value;if(n==="800")void win.setSize(new LogicalSize(800,600));if(n==="1280")void win.setSize(new LogicalSize(1280,800));};
document.querySelector<HTMLSelectElement>("#experiment-font")!.onchange=(ev)=>{document.body.dataset.font=(ev.target as HTMLSelectElement).value;};
