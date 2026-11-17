import { spawnSync } from "node:child_process";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));
const uiDir = join(root, "ui");
const tauriDir = join(uiDir, "src-tauri");

function parseArgs(argv) {
  let diffBase = process.env.VERIFY_DIFF_BASE || "";
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === "--diff-base" && argv[i + 1]) {
      diffBase = argv[i + 1];
      i += 1;
    } else if (argv[i].startsWith("--diff-base=")) {
      diffBase = argv[i].slice("--diff-base=".length);
    } else if (argv[i] === "--help" || argv[i] === "-h") {
      console.log("Usage: node scripts/verify.mjs [--diff-base <git-ref>]");
      process.exit(0);
    } else {
      throw new Error(`Unknown argument: ${argv[i]}`);
    }
  }
  return { diffBase };
}

function run(label, cmd, args, cwd = root) {
  console.log(`\n==> ${label}: ${cmd} ${args.join(" ")}`);
  const isWinNpm = process.platform === "win32" && cmd === "npm";
  const actualCmd = isWinNpm ? "cmd.exe" : cmd;
  const actualArgs = isWinNpm ? ["/d", "/s", "/c", "npm", ...args] : args;
  const result = spawnSync(actualCmd, actualArgs, {
    cwd,
    stdio: "inherit",
    encoding: "utf8",
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`${label} failed with exit code ${result.status}`);
  }
}

function capture(cmd, args, cwd = root) {
  const result = spawnSync(cmd, args, { cwd, encoding: "utf8" });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`${cmd} ${args.join(" ")} failed: ${result.stderr || result.stdout}`);
  }
  return result.stdout.trim();
}

function checkPinnedRustToolchain() {
  console.log("\n==> Check pinned Rust toolchain (rust-toolchain.toml)");
  const toml = readFileSync(join(root, "rust-toolchain.toml"), "utf8");
  const match = toml.match(/channel\s*=\s*"([^"]+)"/);
  if (!match) {
    throw new Error("Could not find pinned channel in rust-toolchain.toml");
  }
  const expectedChannel = match[1];
  const active = capture("rustup", ["show", "active-toolchain"]);
  if (!active.startsWith(`${expectedChannel}-`)) {
    throw new Error(
      `Active Rust toolchain '${active}' does not match rust-toolchain.toml channel '${expectedChannel}'`,
    );
  }
  console.log(`Pinned Rust toolchain active: ${active}`);
}

function checkWebView2Scripts() {
  console.log("\n==> Check WebView2 scripts use shared EnumWindows harness");
  const scriptsDir = join(uiDir, "scripts");
  const files = readdirSync(scriptsDir).filter((name) => name.endsWith(".mjs"));
  const failures = [];

  for (const name of files) {
    const filePath = join(scriptsDir, name);
    const source = readFileSync(filePath, "utf8");
    const rel = `ui/scripts/${name}`;

    if (/\bMainWindowHandle\b/.test(source)) {
      failures.push(
        `${rel}: uses banned Process.MainWindowHandle (flaky on Windows 11); use resizeWindow from ui/scripts/webview2-harness.mjs (EnumWindows).`,
      );
    }

    if (name.startsWith("webview2-") && name !== "webview2-harness.mjs") {
      if (!source.includes("./webview2-harness.mjs")) {
        failures.push(
          `${rel}: must import shared helpers from ./webview2-harness.mjs instead of duplicating launcher/CDP boilerplate.`,
        );
      }
      if (/\bnew WebSocket\s*\(/.test(source)) {
        failures.push(
          `${rel}: inlines WebSocket CDP plumbing; use cdpCommand/evaluate from ui/scripts/webview2-harness.mjs.`,
        );
      }
      if (/\bEnumWindows\b/.test(source)) {
        failures.push(
          `${rel}: inlines EnumWindows PowerShell lookup; use resizeWindow from ui/scripts/webview2-harness.mjs.`,
        );
      }
      if (/function\s+waitPageReady\b/.test(source)) {
        failures.push(
          `${rel}: redefines waitPageReady; import waitPageReady from ui/scripts/webview2-harness.mjs.`,
        );
      }
    }
  }

  if (failures.length > 0) {
    throw new Error(`WebView2 script guardrail check failed:\n- ${failures.join("\n- ")}`);
  }
  console.log(`Checked ${files.length} script(s) in ui/scripts/: OK`);
}

function checkGitDiff(diffBase) {
  run("Git working tree whitespace check", "git", ["diff", "--check"]);
  run("Git staged whitespace check", "git", ["diff", "--cached", "--check"]);
  if (diffBase && !/^0+$/.test(diffBase)) {
    run(`PR diff whitespace check (${diffBase}...HEAD)`, "git", [
      "diff",
      "--check",
      `${diffBase}...HEAD`,
    ]);
  }
}

const { diffBase } = parseArgs(process.argv.slice(2));

checkPinnedRustToolchain();

run("Root rustfmt", "cargo", ["fmt", "--check"]);
run("Root clippy", "cargo", ["clippy", "--locked", "--all-targets", "--", "-D", "warnings"]);
run("Root tests", "cargo", ["test", "--locked", "--all-targets"]);

run("Tauri rustfmt", "cargo", ["fmt", "--check"], tauriDir);
run(
  "Tauri clippy",
  "cargo",
  ["clippy", "--locked", "--all-targets", "--", "-D", "warnings"],
  tauriDir,
);
run("Tauri tests", "cargo", ["test", "--locked", "--all-targets"], tauriDir);

run("UI tests", "npm", ["test"], uiDir);
run("UI typecheck", "npm", ["run", "check"], uiDir);
run("Desktop typecheck", "npm", ["run", "desktop:check"], uiDir);
run("Desktop frontend build", "npm", ["run", "desktop:frontend:build"], uiDir);

checkWebView2Scripts();
checkGitDiff(diffBase);

console.log("\nAll verification checks passed.");
