import { execFileSync, spawn } from "node:child_process";
import { fileURLToPath } from "node:url";

export const uiDir = fileURLToPath(new URL("..", import.meta.url));
export const desktopExe = fileURLToPath(
  new URL("../src-tauri/target/debug/narrative-universe-desktop.exe", import.meta.url),
);
export const spikeExe = fileURLToPath(new URL("../../target/debug/spike.exe", import.meta.url));

export const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

export function editorImports(root = uiDir) {
  const normalized = root.replaceAll("\\", "/").replace(/\/$/, "");
  return `const {EditorView}=await import('/@fs/'+${JSON.stringify(normalized)}+'/node_modules/@codemirror/view/dist/index.js'); const {mentionField}=await import('/@fs/'+${JSON.stringify(normalized)}+'/src/mention-history.ts');`;
}

export async function startVite({
  cwd = uiDir,
  url = "http://127.0.0.1:5173/",
  windowsHide = true,
  attempts = 80,
  delayMs = 250,
} = {}) {
  const vite = spawn(
    process.execPath,
    ["node_modules/vite/bin/vite.js", "--config", "desktop/vite.config.ts"],
    { cwd, stdio: "ignore", windowsHide },
  );
  for (let i = 0; i < attempts; i += 1) {
    try {
      if ((await fetch(url)).ok) return vite;
    } catch {}
    await pause(delayMs);
  }
  vite.kill();
  throw new Error(`vite did not start at ${url}`);
}

export function spawnDesktopApp({
  port,
  userDataDir,
  cwd = uiDir,
  windowsHide = true,
} = {}) {
  const env = {
    ...process.env,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`,
  };
  if (userDataDir) {
    env.WEBVIEW2_USER_DATA_FOLDER = userDataDir;
  }
  return spawn(desktopExe, [], { cwd, stdio: "ignore", windowsHide, env });
}

export async function stopAppAndWait(proc) {
  if (!proc) return;
  if (proc.exitCode !== null || proc.killed) return;
  await new Promise((resolve) => {
    proc.once("exit", resolve);
    proc.kill();
  });
}

export async function waitForTarget(port, { attempts = 120, delayMs = 500 } = {}) {
  for (let i = 0; i < attempts; i += 1) {
    try {
      const pages = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
      const page = pages.find(
        (entry) =>
          entry.type === "page" && /^http:\/\/(localhost|127\.0\.0\.1):5173\//.test(entry.url),
      );
      if (page) return page.webSocketDebuggerUrl;
    } catch {}
    await pause(delayMs);
  }
  throw new Error(`WebView2 debugging target did not appear on port ${port}`);
}

let seq = 0;
export function cdpCommand(url, method, params = {}, timeoutMs = 30000) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(url);
    const id = ++seq;
    const timeout = setTimeout(() => {
      ws.close();
      reject(new Error(`CDP timeout ${method}`));
    }, timeoutMs);
    ws.onopen = () => ws.send(JSON.stringify({ id, method, params }));
    ws.onmessage = (event) => {
      const msg = JSON.parse(event.data);
      if (msg.id !== id) return;
      clearTimeout(timeout);
      ws.close();
      if (msg.error || msg.result?.exceptionDetails) {
        reject(new Error(JSON.stringify(msg)));
      } else {
        resolve(msg.result);
      }
    };
    ws.onerror = (err) => {
      clearTimeout(timeout);
      reject(err);
    };
  });
}

export async function evaluate(url, expression) {
  const result = await cdpCommand(url, "Runtime.evaluate", {
    expression,
    awaitPromise: true,
    returnByValue: true,
  });
  return result.result.value;
}

export async function waitPageReady(url, { attempts = 80, delayMs = 200 } = {}) {
  for (let i = 0; i < attempts; i += 1) {
    try {
      const state = await evaluate(
        url,
        '({ready:document.readyState,href:location.href,hasDb:Boolean(document.querySelector("#database-path")),hasTauri:Boolean(window.__TAURI_INTERNALS__)})',
      );
      if (
        state &&
        state.ready === "complete" &&
        /5173/.test(state.href) &&
        state.hasDb &&
        state.hasTauri
      ) {
        return state;
      }
    } catch {}
    await pause(delayMs);
  }
  throw new Error("Page never became ready on 5173");
}

// Resolves the top-level Tauri HWND via EnumWindows rather than Get-Process main-window
// handle (which on Windows 11 can resolve to transient overlay/IME windows).
export async function resizeWindow(url, pid, width, height, { verify = false } = {}) {
  const dpr = await evaluate(url, "devicePixelRatio");
  const script = (w, h) =>
    `Add-Type -TypeDefinition 'using System;using System.Text;using System.Runtime.InteropServices;public class TauriWinLookup{public delegate bool EnumWindowsProc(IntPtr h,IntPtr l);[DllImport("user32.dll")]public static extern bool EnumWindows(EnumWindowsProc p,IntPtr l);[DllImport("user32.dll")]public static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);[DllImport("user32.dll")]public static extern int GetClassName(IntPtr h,StringBuilder s,int n);[DllImport("user32.dll")]public static extern int GetWindowText(IntPtr h,StringBuilder s,int n);[DllImport("user32.dll")]public static extern bool SetWindowPos(IntPtr h,IntPtr a,int x,int y,int w,int z,uint f);public static IntPtr FindTauri(uint target){IntPtr found=IntPtr.Zero;EnumWindows((h,l)=>{uint pid;GetWindowThreadProcessId(h,out pid);if(pid==target){var c=new StringBuilder(256);var t=new StringBuilder(256);GetClassName(h,c,256);GetWindowText(h,t,256);if(c.ToString()=="Tauri Window"||t.ToString()=="Narrative Universe"){found=h;return false;}}return true;},IntPtr.Zero);return found;}}'; $h=[TauriWinLookup]::FindTauri(${pid}); if($h -eq [IntPtr]::Zero){throw "No Tauri window handle for pid ${pid}"}; if(-not [TauriWinLookup]::SetWindowPos($h,[IntPtr]::Zero,0,0,${Math.round(w * dpr)},${Math.round(h * dpr)},22)){throw "SetWindowPos failed"}`;
  execFileSync("powershell.exe", ["-NoProfile", "-Command", script(width, height)], {
    windowsHide: true,
  });
  await pause(350);
  const actual = await evaluate(url, "({width:innerWidth,height:innerHeight})");
  if (actual.width !== width || actual.height !== height) {
    execFileSync(
      "powershell.exe",
      ["-NoProfile", "-Command", script(2 * width - actual.width, 2 * height - actual.height)],
      { windowsHide: true },
    );
    await pause(350);
  }
  if (verify) {
    const settled = await evaluate(
      url,
      "({width:innerWidth,height:innerHeight,dpr:devicePixelRatio})",
    );
    if (settled.width !== width || settled.height !== height) {
      throw new Error(
        `window dimensions mismatch: ${JSON.stringify(settled)} vs ${width}x${height}`,
      );
    }
    return settled;
  }
  return undefined;
}
