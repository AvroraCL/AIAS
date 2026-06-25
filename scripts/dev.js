const { spawn } = require("node:child_process");
const path = require("node:path");
const electronPath = require("electron");

const rendererUrl = "http://127.0.0.1:5173";
const children = [];

function run(command, args, options = {}) {
  const child = spawn(command, args, {
    cwd: process.cwd(),
    stdio: "inherit",
    ...options
  });
  children.push(child);
  return child;
}

function runBin(command, args) {
  if (process.platform !== "win32") return run(command, args);
  return run("cmd.exe", ["/d", "/c", command, ...args]);
}

function stopAll() {
  for (const child of children) {
    if (!child.killed) child.kill();
  }
}

async function waitForRenderer() {
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(rendererUrl);
      if (response.ok) return;
    } catch {
      // Vite is still warming up.
    }
    await new Promise((resolve) => setTimeout(resolve, 300));
  }
  throw new Error(`Renderer dev server did not start at ${rendererUrl}`);
}

async function main() {
  const viteBin = path.join(process.cwd(), "node_modules", ".bin", process.platform === "win32" ? "vite.cmd" : "vite");
  const vite = runBin(viteBin, ["--host", "127.0.0.1"]);
  vite.on("exit", (code) => {
    if (code) process.exitCode = code;
    stopAll();
  });

  await waitForRenderer();

  const env = { ...process.env, AIAS_RENDERER_URL: rendererUrl };
  delete env.ELECTRON_RUN_AS_NODE;

  const electron = spawn(electronPath, ["."], {
    cwd: process.cwd(),
    env,
    stdio: "inherit",
    windowsHide: false
  });
  children.push(electron);
  electron.on("exit", (code) => {
    process.exitCode = code ?? 0;
    stopAll();
  });
}

process.on("SIGINT", () => {
  stopAll();
  process.exit(0);
});
process.on("SIGTERM", () => {
  stopAll();
  process.exit(0);
});

main().catch((error) => {
  console.error(error);
  stopAll();
  process.exit(1);
});
