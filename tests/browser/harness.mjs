import { spawn } from "node:child_process";
import { existsSync, mkdtempSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const root = process.env.PANDA_ROOT || path.resolve(here, "../..");
const port = process.env.PW_PORT || "8799";
const home = mkdtempSync(path.join(tmpdir(), "panda-pw-"));

function resolveBin() {
  const candidates = [
    process.env.PANDA_BIN,
    process.env.CARGO_TARGET_DIR
      ? path.join(process.env.CARGO_TARGET_DIR, "debug", "panda")
      : null,
    path.join(root, "target/debug/panda"),
  ].filter(Boolean);
  for (const file of candidates) {
    if (existsSync(file)) return { cmd: file, args: [] };
  }
  return { cmd: "cargo", args: ["run", "-q", "-p", "causewaybay-panda-server"] };
}

const { cmd, args } = resolveBin();

// A developer with XAI_API_KEY exported would otherwise have every unparsed
// line in the suite answered by a live model — slow, billed, and different
// each run. Pin the local parser unless the run explicitly asks for Grok.
const env = {
  ...process.env,
  PANDA_PORT: port,
  PANDA_HOME: home,
  PANDA_ROOT: root,
};
// A stand-in model: the shape OpenAI, Grok, OpenRouter and Ollama all speak.
// It reads a few words and answers with an intent, so the whole path from a
// free sentence to a changed cart runs with nothing on the wire but this.
// "fail" in the sentence makes it break, to prove the parser's answer stands.
if (process.env.PW_MOCK_AI_PORT) {
  const ai = createServer((req, res) => {
    let body = "";
    req.on("data", (c) => (body += c));
    req.on("end", () => {
      let text = "";
      try {
        const q = JSON.parse(body);
        text = String((q.messages || []).slice(-1)[0]?.content || "").toLowerCase();
      } catch {
        /* fall through to help */
      }
      if (text.includes("fail")) {
        res.statusCode = 500;
        res.end("{}");
        return;
      }
      const intent = text.includes("warm")
        ? { intent: "add", item_id: "latte", qty: 2 }
        : text.includes("sweet")
          ? { intent: "add", item_id: "egg_tart", qty: 1 }
          : text.includes("bill")
            ? { intent: "pay" }
            : { intent: "help" };
      res.setHeader("content-type", "application/json");
      res.end(JSON.stringify({ choices: [{ message: { content: JSON.stringify(intent) } }] }));
    });
  });
  ai.listen(Number(process.env.PW_MOCK_AI_PORT), "127.0.0.1");
  process.on("exit", () => ai.close());
}

if (process.env.PW_MOCK_AI_PORT) {
  // The shop is pointed at the stand-in, as any OpenAI-shaped provider.
  env.PANDA_AI_PROVIDER = "openrouter";
  env.OPENROUTER_API_KEY = "test-key";
  env.PANDA_AI_BASE_URL = `http://127.0.0.1:${process.env.PW_MOCK_AI_PORT}`;
  for (const k of ["XAI_API_KEY", "GROK_API_KEY", "OPENAI_API_KEY", "ANTHROPIC_API_KEY", "OLLAMA_HOST"]) {
    delete env[k];
  }
} else if (process.env.PANDA_PW_GROK !== "1") {
  // Any provider key in the developer's shell would otherwise answer the
  // suite's unparsed lines — slow, billed, and different each run.
  env.PANDA_AI_PROVIDER = "off";
  for (const k of [
    "XAI_API_KEY",
    "GROK_API_KEY",
    "OPENAI_API_KEY",
    "ANTHROPIC_API_KEY",
    "OPENROUTER_API_KEY",
    "OLLAMA_HOST",
  ]) {
    delete env[k];
  }
}

// An on-chain cafe reads receipts from the chain's RPC. In the suite that is
// this stub, so no test ever waits on Cronos or needs a funded wallet. The
// first byte of a hash decides its fate, the same convention the Rust tests
// use: 0xaa… paid, 0xbb… never mined, 0xcc… reverted, 0xee… underpaid.
const TRANSFER_TOPIC = "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";
const topic = (addr) => "0x" + "0".repeat(24) + addr.slice(2).toLowerCase();
if (process.env.PW_MOCK_RPC_PORT) {
  const treasury = env.PANDA_TREASURY;
  const usdc = env.PANDA_USDC_ADDRESS || "0xc21223249CA28397B4B6541dfFaEcC539BfF0c59";
  const guest = "0x3333333333333333333333333333333333333333";
  const receipt = (status, to, amount) => ({
    status,
    blockNumber: "0x10",
    to: usdc,
    logs: [{ address: usdc, topics: [TRANSFER_TOPIC, topic(guest), topic(to)],
             data: "0x" + amount.toString(16).padStart(64, "0") }],
  });
  const rpc = createServer((req, res) => {
    let body = "";
    req.on("data", (c) => (body += c));
    req.on("end", () => {
      let out = { jsonrpc: "2.0", id: null, result: null };
      try {
        const q = JSON.parse(body);
        out.id = q.id;
        if (q.method === "eth_getTransactionReceipt") {
          const h = String(q.params[0]).toLowerCase();
          out.result =
            h.slice(2, 4) === "aa" ? receipt("0x1", treasury, 100_000_000n)
            : h.slice(2, 4) === "cc" ? receipt("0x0", treasury, 100_000_000n)
            : h.slice(2, 4) === "ee" ? receipt("0x1", treasury, 1_000_000n)
            : null;
        }
      } catch {
        out.error = { code: -32700, message: "parse error" };
      }
      res.setHeader("content-type", "application/json");
      res.end(JSON.stringify(out));
    });
  });
  rpc.listen(Number(process.env.PW_MOCK_RPC_PORT), "127.0.0.1");
  env.PANDA_RPC_URL = `http://127.0.0.1:${process.env.PW_MOCK_RPC_PORT}/`;
  process.on("exit", () => rpc.close());
}

const child = spawn(cmd, args, { cwd: root, env, stdio: "inherit" });

child.on("error", (err) => {
  console.error("failed to start panda:", err.message);
  process.exit(1);
});

const stop = () => {
  try {
    child.kill("SIGTERM");
  } catch {
    /* already gone */
  }
};
process.on("exit", stop);
process.on("SIGINT", () => {
  stop();
  process.exit(130);
});
process.on("SIGTERM", () => {
  stop();
  process.exit(143);
});
