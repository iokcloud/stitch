import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const crateRoot = path.resolve(__dirname, "..");
const workspaceTarget = path.resolve(crateRoot, "../../target");

// debug 构建默认用 stitch-dev 配置目录（config.rs debug_assertions 分支）——
// 真机探针语义是「用正式配置测 debug 构建」，注入正式配置目录（tauri-service
// 启动的 exe 继承本环境变量）。可被 STITCH_CONFIG_DIR 显式覆盖。
if (!process.env.STITCH_CONFIG_DIR) {
  const appData = process.env.APPDATA ?? "";
  if (appData) {
    process.env.STITCH_CONFIG_DIR = path.join(appData, "promptstdio", "stitch");
  }
}

const isWin = process.platform === "win32";
const binaryName = isWin ? "stitch-desktop.exe" : "stitch-desktop";
const defaultBinary = path.join(workspaceTarget, "debug", binaryName);
const appBinaryPath = process.env.STITCH_APP_BINARY
  ? path.resolve(process.env.STITCH_APP_BINARY)
  : defaultBinary;

/**
 * Desktop smoke via WebdriverIO + @wdio/tauri-service.
 * Windows/Linux: official tauri-driver (no Rust plugin in the app).
 * Set STITCH_APP_BINARY to override the debug exe path.
 */
export const config: WebdriverIO.Config = {
  runner: "local",
  specs: ["./specs/**/*.spec.ts"],
  maxInstances: 1,
  capabilities: [
    {
      browserName: "tauri",
      "tauri:options": {
        application: appBinaryPath,
      },
    },
  ],
  logLevel: "warn",
  bail: 0,
  waitforTimeout: 15_000,
  connectionRetryTimeout: 120_000,
  connectionRetryCount: 2,
  framework: "mocha",
  reporters: ["spec"],
  mochaOpts: {
    ui: "bdd",
    // Real LLM turns (incl. agent-rich multi-file) need long headroom
    timeout: 420_000,
  },
  services: [
    [
      "@wdio/tauri-service",
      {
        appBinaryPath,
        // Embedded server inside the app (feature `webdriver`). Avoids Edge
        // DevToolsActivePort failures while the splash keeps the window hidden.
        driverProvider: "embedded",
        // Avoid default 4445 — leftover msedgedriver often owns it on Windows.
        embeddedPort: Number(process.env.WDIO_EMBEDDED_PORT || 17445),
        startTimeout: 90_000,
        captureFrontendLogs: true,
      },
    ],
  ],
};
