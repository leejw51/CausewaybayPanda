import { defineConfig } from "@playwright/test";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const port = process.env.PW_PORT || "8799";
// A second cafe, wired to real USDC on Cronos. No chain is ever touched: the
// browser's wallet is a stub, so this exercises our side of the settlement.
const chainPort = process.env.PW_CHAIN_PORT || "8800";
const mockRpcPort = process.env.PW_MOCK_RPC_PORT || "8801";
const TREASURY = "0x2222222222222222222222222222222222222222";

export default defineConfig({
  testDir: ".",
  timeout: 45_000,
  retries: 0,
  // One cafe, one menu, one SQLite file behind every page. Tests that hide a
  // dish or add one would race each other across workers, so they run in file
  // order against a server the harness gives a fresh PANDA_HOME each run.
  workers: 1,
  fullyParallel: false,
  use: { headless: true },
  projects: [
    {
      name: "cafe",
      testIgnore: ["wallet.spec.js", "local.spec.js"],
      use: { baseURL: `http://127.0.0.1:${port}` },
    },
    {
      name: "wallet",
      testMatch: "wallet.spec.js",
      use: { baseURL: `http://127.0.0.1:${chainPort}` },
    },
    // The cafe with no server at all: the page is served as plain files and
    // ?local makes it ignore the panda that happens to be serving them. The
    // whole shop runs in the tab, in WebAssembly built from the same crates.
    {
      name: "local",
      testMatch: "local.spec.js",
      use: { baseURL: `http://127.0.0.1:${port}` },
    },
  ],
  webServer: [
    {
      command: `node harness.mjs`,
      url: `http://127.0.0.1:${port}/health`,
      timeout: 60_000,
      reuseExistingServer: false,
      env: { ...process.env, PW_PORT: port, PANDA_ROOT: root },
    },
    {
      command: `node harness.mjs`,
      url: `http://127.0.0.1:${chainPort}/health`,
      timeout: 60_000,
      reuseExistingServer: false,
      env: {
        ...process.env,
        PW_PORT: chainPort,
        PANDA_ROOT: root,
        // A shop that actually takes real USDC, not the simulation default.
        PANDA_MODE: "live",
        PANDA_CHAIN: "cronos_mainnet",
        PANDA_TREASURY: TREASURY,
        PW_MOCK_RPC_PORT: mockRpcPort,
        // The stub never mines a 0xbb… hash; do not wait the real 90 seconds.
        PANDA_RECEIPT_WAIT_SECS: "3",
      },
    },
  ],
});
