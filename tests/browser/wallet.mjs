/* A stub EIP-1193 wallet, installed before the page's own scripts run. No key,
   no network — it records what the page asked it to sign so a test can read
   the transfer back. */

export const ACCOUNT = "0x3333333333333333333333333333333333333333";
export const TREASURY = "0x2222222222222222222222222222222222222222";
export const USDC = "0xc21223249CA28397B4B6541dfFaEcC539BfF0c59";
/** Fates the mock chain assigns by a hash's first byte. */
export const FATE = { paid: "aa", pending: "bb", reverted: "cc", underpaid: "ee" };

/**
 * @param {import("@playwright/test").Page} page
 * @param {{chainId?: string, rejectSend?: boolean, unknownChain?: boolean, fate?: string}} opts
 */
export async function installWallet(page, opts = {}) {
  await page.addInitScript(
    (o) => {
      const calls = [];
      let chainId = o.chainId;
      // Every send is a new transaction; the prefix says how the chain will
      // treat it, the rest is random so no two tests share a hash.
      const mint = () => {
        const bytes = crypto.getRandomValues(new Uint8Array(31));
        return "0x" + o.fate + Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
      };
      window.__wallet = { calls, sent: [] };
      window.ethereum = {
        isMetaMask: true,
        async request({ method, params }) {
          calls.push({ method, params });
          switch (method) {
            case "eth_requestAccounts":
              return [o.account];
            case "eth_chainId":
              return chainId;
            case "wallet_switchEthereumChain":
              if (o.unknownChain) {
                const err = new Error("Unrecognized chain ID");
                err.code = 4902;
                throw err;
              }
              chainId = params[0].chainId;
              return null;
            case "wallet_addEthereumChain":
              chainId = params[0].chainId;
              return null;
            case "eth_sendTransaction":
              if (o.rejectSend) {
                const err = new Error("User rejected the request.");
                err.code = 4001;
                throw err;
              }
              const h = mint();
              window.__wallet.sent.push(h);
              return h;
            default:
              throw new Error(`unstubbed ${method}`);
          }
        },
        on() {},
        removeListener() {},
      };
    },
    { account: ACCOUNT, chainId: "0x1", fate: FATE.paid, ...opts }
  );
}

/** Every call the page made to the wallet. */
export function walletCalls(page) {
  return page.evaluate(() => window.__wallet.calls);
}

/** The hash the wallet handed back most recently. */
export function lastTx(page) {
  return page.evaluate(() => window.__wallet.sent.at(-1));
}

/** micro-USDC as the 32-byte word an ERC-20 transfer carries. */
export function amountWord(micro) {
  return micro.toString(16).padStart(64, "0");
}
