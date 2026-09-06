/* Settling in real USDC on Cronos. This project points at a cafe started with
   PANDA_CHAIN=cronos_mainnet and a real treasury; the wallet is stubbed, so
   every assertion is about our own half of the handshake. */
import { test, expect } from "@playwright/test";
import { guest, owner, dish, lastLine } from "./cafe.mjs";
import { installWallet, walletCalls, lastTx, amountWord, ACCOUNT, TREASURY, USDC, FATE } from "./wallet.mjs";

// HK$38 at the pegged 7.8, in the micro-USDC that actually moves.
const LATTE_MICRO = 4_871_795;

test.describe("USDC on Cronos", () => {
  test("the shop reports itself as on-chain", async ({ request }) => {
    const body = await (await request.get("/health")).json();
    expect(body.onchain).toBe(true);
    expect(body.chain).toBe("cronos_mainnet");
    expect(body.mode).toBe("live");
  });

  // Real money: the pin is the lock on the till and stays one.
  test("a live shop refuses a wrong owner pin", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByTestId("local-note")).toBeHidden();
    await page.getByTestId("owner-pin").fill("nope");
    await page.getByTestId("login-owner").click();
    await expect(page.getByTestId("door-error")).toContainText("pin");
    await expect(page.getByTestId("stage-app")).toBeHidden();
    await page.getByTestId("owner-pin").fill("panda");
    await page.getByTestId("login-owner").click();
    await expect(page.getByTestId("owner-tools")).toBeVisible();
  });

  test("a guest with a wallet is offered the chain door", async ({ page }) => {
    await installWallet(page);
    await guest(page);
    const btn = page.getByTestId("pay-wallet");
    await expect(btn).toBeVisible();
    await expect(btn).toContainText("Cronos Mainnet");
    // The USDC contract is named, so a guest can check it before paying.
    await expect(page.getByTestId("chain-note")).toContainText(USDC.slice(0, 6));
    // A live shop holds no purse for the guest; their wallet is the purse,
    // and until they share an account there is nothing to read.
    await expect(page.getByTestId("purse-label")).toHaveText("Wallet");
    await expect(page.getByTestId("balance")).toHaveText("—");
    await expect(page.getByTestId("wallet-connect")).toBeVisible();
    await expect(page.getByTestId("faucet")).toBeHidden();
  });

  // The guest's money is on the chain, so the header reads it from there.
  test("the wallet is the purse: connect, read the balance off the chain, watch it fall", async ({ page }) => {
    await installWallet(page, { chainId: "0x19", balance: "100000000" });
    await guest(page);
    await page.getByTestId("wallet-connect").click();
    await expect(page.getByTestId("purse-label")).toHaveText(`${ACCOUNT.slice(0, 6)}…${ACCOUNT.slice(-4)}`);
    await expect(page.getByTestId("balance")).toHaveText("100.00 USDC");
    // Read as the board reads: 100 USDC at the pegged 7.8.
    await expect(page.getByTestId("wallet-approx")).toHaveText("≈ HK$780.00");
    await expect(page.getByTestId("wallet-connect")).toBeHidden();
    const calls = await walletCalls(page);
    const read = calls.find((c) => c.method === "eth_call");
    expect(read.params[0].to.toLowerCase()).toBe(USDC.toLowerCase());
    expect(read.params[0].data).toBe("0x70a08231" + "0".repeat(24) + ACCOUNT.slice(2).toLowerCase());

    await dish(page, "latte").click();
    await page.getByTestId("pay-wallet").click();
    await expect(page.getByTestId("paid-banner")).toBeVisible();
    // A latte is 4.871795 USDC; the chain now says so.
    await expect(page.getByTestId("balance")).toHaveText("95.12 USDC");
  });

  test("a wallet already shared with the page needs no tap", async ({ page }) => {
    await installWallet(page, { chainId: "0x19", connected: true, balance: "12500000" });
    await guest(page);
    await expect(page.getByTestId("balance")).toHaveText("12.50 USDC");
    await expect(page.getByTestId("wallet-connect")).toBeHidden();
    const calls = await walletCalls(page);
    expect(calls.filter((c) => c.method === "eth_requestAccounts")).toHaveLength(0);
  });

  test("the owner reads the treasury off the chain, and again after a payment", async ({ page, browser }) => {
    const shop = await owner(page);
    const box = shop.getByTestId("dash-treasury-box");
    await expect(box).toBeVisible();
    // The mock chain holds 250 USDC for the treasury.
    await expect(shop.getByTestId("dash-treasury")).toHaveText("HK$1,950.00");
    await expect(shop.getByTestId("dash-treasury-usdc")).toContainText("250 USDC");
    const link = shop.getByTestId("dash-treasury-link");
    await expect(link).toHaveAttribute("href", `https://cronoscan.com/address/${TREASURY}`);
    await expect(link).toContainText("Cronos Mainnet");

    // A payment lands: the counter asks the chain again without a reload.
    const before = await shop.evaluate(() => (window.__treasuryReads = 0));
    await shop.evaluate(() => {
      window.__treasuryReads = 0;
      const seen = document.getElementById("dash-treasury");
      new MutationObserver(() => window.__treasuryReads++).observe(seen, { childList: true, characterData: true, subtree: true });
    });
    const table = await browser.newPage();
    await installWallet(table, { chainId: "0x19" });
    await guest(table, "Treasury Chan");
    await dish(table, "egg_tart").click();
    await table.getByTestId("pay-wallet").click();
    await expect(table.getByTestId("paid-banner")).toBeVisible();
    await expect.poll(() => shop.evaluate(() => window.__treasuryReads), { timeout: 5_000 }).toBeGreaterThan(before);
    await table.close();
  });

  test("a chain payment in the till links to its transaction", async ({ page, browser }) => {
    const shop = await owner(page);
    const table = await browser.newPage();
    await installWallet(table, { chainId: "0x19" });
    await guest(table, "Link Chan");
    await dish(table, "egg_tart").click();
    await table.getByTestId("pay-wallet").click();
    await expect(table.getByTestId("paid-banner")).toBeVisible();
    const tx = await lastTx(table);
    const row = shop.getByTestId("payment-row").filter({ hasText: "Link Chan" }).first();
    await expect(row.getByTestId("payment-link")).toHaveAttribute("href", `https://cronoscan.com/tx/${tx}`);
    await table.close();
  });

  test("a browser with no wallet is told why it cannot pay on chain", async ({ page }) => {
    await guest(page); // no installWallet
    await expect(page.getByTestId("pay-wallet")).toBeHidden();
    await expect(page.getByTestId("chain-note")).toContainText("Open in a wallet browser");
  });

  test("the owner is never offered the guest's wallet door", async ({ page }) => {
    await installWallet(page);
    await owner(page);
    await expect(page.getByTestId("pay-wallet")).toBeHidden();
  });

  test("paying signs a transfer of the cart total to the shop's treasury", async ({ page }) => {
    await installWallet(page, { chainId: "0x19" });
    await guest(page);
    await dish(page, "latte").click();
    await expect(page.getByTestId("cart-total")).toHaveText("HK$38.00");

    await page.getByTestId("pay-wallet").click();
    await expect(page.getByTestId("paid-banner")).toBeVisible();

    const calls = await walletCalls(page);
    const sent = calls.find((c) => c.method === "eth_sendTransaction");
    expect(sent, "the page must ask the wallet to send").toBeTruthy();
    const tx = sent.params[0];

    expect(tx.from.toLowerCase()).toBe(ACCOUNT.toLowerCase());
    // The call goes to the USDC contract, never straight to the treasury.
    expect(tx.to.toLowerCase()).toBe(USDC.toLowerCase());
    expect(tx.value).toBe("0x0");
    // transfer(address,uint256) — payee and amount both readable in the bytes.
    expect(tx.data.startsWith("0xa9059cbb")).toBeTruthy();
    expect(tx.data.toLowerCase()).toContain(TREASURY.slice(2).toLowerCase());
    expect(tx.data.toLowerCase().endsWith(amountWord(LATTE_MICRO))).toBeTruthy();
    expect(tx.data.length).toBe(2 + 8 + 64 + 64);
  });

  test("the recorded payment is the chain's hash, linked to the explorer", async ({ page }) => {
    await installWallet(page, { chainId: "0x19" });
    await guest(page);
    await dish(page, "latte").click();
    await page.getByTestId("pay-wallet").click();

    const banner = page.getByTestId("paid-banner");
    await expect(banner).toContainText("Paid HK$38.00");
    const tx = await lastTx(page);
    const link = page.getByTestId("paid-link");
    await expect(link).toHaveAttribute("href", `https://cronoscan.com/tx/${tx}`);
    await expect(link).toHaveAttribute("target", "_blank");
    await expect(link).toHaveAttribute("rel", /noopener/);
  });

  test("an on-chain payment does not touch the play-money grant", async ({ page }) => {
    await installWallet(page, { chainId: "0x19" });
    await guest(page);
    await dish(page, "latte").click();
    await page.getByTestId("pay-wallet").click();
    await expect(page.getByTestId("paid-banner")).toBeVisible();

    // Real USDC left the guest's own wallet, so nothing was drawn from the
    // house and the cart is settled.
    await expect(page.getByTestId("cart-total")).toHaveText("HK$0.00");
    await expect(page.getByTestId("cart-lines").locator("li")).toHaveCount(0);
  });

  test("a wallet on the wrong network is switched to Cronos first", async ({ page }) => {
    await installWallet(page, { chainId: "0x1" }); // sitting on Ethereum
    await guest(page);
    await dish(page, "latte").click();
    await page.getByTestId("pay-wallet").click();
    await expect(page.getByTestId("paid-banner")).toBeVisible();

    const calls = await walletCalls(page);
    const order = calls.map((c) => c.method);
    expect(order).toContain("wallet_switchEthereumChain");
    const sw = calls.find((c) => c.method === "wallet_switchEthereumChain");
    expect(sw.params[0].chainId).toBe("0x19");
    // And the switch happens before anything is signed.
    expect(order.indexOf("wallet_switchEthereumChain")).toBeLessThan(
      order.indexOf("eth_sendTransaction")
    );
  });

  test("a wallet that has never heard of Cronos is offered the network", async ({ page }) => {
    await installWallet(page, { chainId: "0x1", unknownChain: true });
    await guest(page);
    await dish(page, "latte").click();
    await page.getByTestId("pay-wallet").click();
    await expect(page.getByTestId("paid-banner")).toBeVisible();

    const calls = await walletCalls(page);
    const add = calls.find((c) => c.method === "wallet_addEthereumChain");
    expect(add, "a 4902 must be answered with wallet_addEthereumChain").toBeTruthy();
    const net = add.params[0];
    expect(net.chainId).toBe("0x19");
    expect(net.chainName).toBe("Cronos Mainnet");
    expect(net.nativeCurrency).toMatchObject({ symbol: "CRO", decimals: 18 });
    expect(net.rpcUrls).toEqual(["https://evm.cronos.org"]);
    expect(net.blockExplorerUrls).toEqual(["https://cronoscan.com"]);
  });

  test("turning the payment down leaves the cart and the books alone", async ({ page }) => {
    await installWallet(page, { chainId: "0x19", rejectSend: true });
    await guest(page);
    await dish(page, "latte").click();
    await page.getByTestId("pay-wallet").click();

    await expect(lastLine(page)).toContainText("turned the payment down");
    await expect(page.getByTestId("paid-banner")).toBeHidden();
    // The cart survives, so the guest can try again.
    await expect(page.getByTestId("cart-total")).toHaveText("HK$38.00");
    // And the button is usable again, not stuck on "Check your wallet…".
    await expect(page.getByTestId("pay-wallet")).toBeEnabled();
  });

  test("an empty cart never opens the wallet", async ({ page }) => {
    await installWallet(page, { chainId: "0x19" });
    await guest(page);
    // Shut rather than answering with an error, and no wallet prompt.
    await expect(page.getByTestId("pay-wallet")).toBeDisabled();
    const calls = await walletCalls(page);
    expect(calls.filter((c) => c.method === "eth_sendTransaction")).toHaveLength(0);
  });

  test("the owner sees the on-chain payment in the till", async ({ page, browser }) => {
    const shop = await owner(page);
    const table = await browser.newPage();
    await installWallet(table, { chainId: "0x19" });
    await guest(table, "Chan");
    await dish(table, "egg_tart").click();
    await table.getByTestId("pay-wallet").click();
    await expect(table.getByTestId("paid-banner")).toBeVisible();

    const row = shop.getByTestId("payment-row").filter({ hasText: "Chan" }).first();
    await expect(row).toContainText("HK$10.00");
    await expect(row).toContainText("wallet");
    await table.close();
  });

  // The hash a browser reports is a claim. The till reads the receipt from
  // the chain before it books anything, so a transaction that never landed,
  // reverted, or paid too little is refused and the cart is kept.
  test("a transaction the chain has not confirmed is not a payment", async ({ page }) => {
    await installWallet(page, { chainId: "0x19", fate: FATE.pending });
    await guest(page);
    await dish(page, "latte").click();
    await page.getByTestId("pay-wallet").click();
    await expect(lastLine(page)).toContainText("not confirmed", { timeout: 15_000 });
    await expect(page.getByTestId("paid-banner")).toBeHidden();
    await expect(page.getByTestId("cart-total")).toHaveText("HK$38.00");
  });

  test("a reverted transaction is not a payment", async ({ page }) => {
    await installWallet(page, { chainId: "0x19", fate: FATE.reverted });
    await guest(page);
    await dish(page, "latte").click();
    await page.getByTestId("pay-wallet").click();
    await expect(lastLine(page)).toContainText("reverted");
    await expect(page.getByTestId("paid-banner")).toBeHidden();
    await expect(page.getByTestId("cart-total")).toHaveText("HK$38.00");
  });

  test("a transaction that paid too little is refused with the shortfall", async ({ page }) => {
    await installWallet(page, { chainId: "0x19", fate: FATE.underpaid });
    await guest(page);
    await dish(page, "latte").click();
    await page.getByTestId("pay-wallet").click();
    await expect(lastLine(page)).toContainText("bill is HK$38.00");
    await expect(page.getByTestId("paid-banner")).toBeHidden();
    await expect(page.getByTestId("cart-total")).toHaveText("HK$38.00");
  });

  test("the guest is told the chain is being checked", async ({ page }) => {
    await installWallet(page, { chainId: "0x19" });
    await guest(page);
    await dish(page, "latte").click();
    await page.getByTestId("pay-wallet").click();
    await expect(page.getByTestId("transcript")).toContainText("Checking Cronos Mainnet");
    await expect(page.getByTestId("paid-banner")).toBeVisible();
  });

  // A live shop has no test money and no faucet: the plain Pay button is the
  // same on-chain settlement as the wallet button.
  test("a live shop hands out no test money", async ({ page }) => {
    await installWallet(page, { chainId: "0x19" });
    await guest(page);
    await expect(page.getByTestId("faucet")).toBeHidden();
    await expect(page.getByTestId("mode-badge")).toContainText("Real USDC");
    await expect(page.getByTestId("purse-label")).toHaveText("Wallet");
    await expect(page.getByTestId("balance")).toHaveText("—");

    await dish(page, "latte").click();
    await page.getByTestId("pay-usdc").click();
    await expect(page.getByTestId("paid-banner")).toContainText("Paid HK$38.00");
    // It settled on chain, so there is a transaction to follow.
    await expect(page.getByTestId("paid-link")).toBeVisible();
    const calls = await walletCalls(page);
    expect(calls.filter((c) => c.method === "eth_sendTransaction")).toHaveLength(1);
  });

  // A script must never spend real USDC.
  test("a live shop refuses to run itself", async ({ page }) => {
    await owner(page);
    await page.getByTestId("auto-owner").click();
    await expect(lastLine(page)).toContainText("real USDC");
    await expect(page.getByTestId("auto-owner")).toHaveText("Run the cafe on its own");
    await expect(page.getByTestId("auto-dot")).toBeHidden();
    // Whatever earlier tests left on the counter stays exactly as it was.
    const before = await page.locator(".ticket-row").count();
    await page.waitForTimeout(1200);
    await expect(page.locator(".ticket-row")).toHaveCount(before);
  });
});
