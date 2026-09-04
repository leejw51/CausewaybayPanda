/* Settling in real USDC on Cronos. This project points at a cafe started with
   PANDA_CHAIN=cronos_mainnet and a real treasury; the wallet is stubbed, so
   every assertion is about our own half of the handshake. */
import { test, expect } from "@playwright/test";
import { guest, owner, dish, lastLine } from "./cafe.mjs";
import { installWallet, walletCalls, lastTx, amountWord, ACCOUNT, TREASURY, USDC, FATE } from "./wallet.mjs";

const LATTE_MICRO = 4_800_000;

test.describe("USDC on Cronos", () => {
  test("the shop reports itself as on-chain", async ({ request }) => {
    const body = await (await request.get("/health")).json();
    expect(body.onchain).toBe(true);
    expect(body.chain).toBe("cronos_mainnet");
  });

  test("a guest with a wallet is offered the chain door", async ({ page }) => {
    await installWallet(page);
    await guest(page);
    const btn = page.getByTestId("pay-wallet");
    await expect(btn).toBeVisible();
    await expect(btn).toContainText("Cronos Mainnet");
    // The USDC contract is named, so a guest can check it before paying.
    await expect(page.getByTestId("chain-note")).toContainText(USDC.slice(0, 6));
    // The play-money door stays, for anyone without a wallet.
    await expect(page.getByTestId("pay-usdc")).toBeVisible();
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
    await expect(page.getByTestId("cart-total")).toHaveText("4.8");

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
    await expect(banner).toContainText("Paid 4.8 USDC");
    const tx = await lastTx(page);
    const link = page.getByTestId("paid-link");
    await expect(link).toHaveAttribute("href", `https://cronoscan.com/tx/${tx}`);
    await expect(link).toHaveAttribute("target", "_blank");
    await expect(link).toHaveAttribute("rel", /noopener/);
  });

  test("an on-chain payment does not touch the play-money grant", async ({ page }) => {
    await installWallet(page, { chainId: "0x19" });
    await guest(page);
    await expect(page.getByTestId("balance")).toHaveText("50");
    await dish(page, "latte").click();
    await page.getByTestId("pay-wallet").click();
    await expect(page.getByTestId("paid-banner")).toBeVisible();

    // Real USDC left the guest's own wallet, so the house grant is unchanged
    // and the cart is settled.
    await expect(page.getByTestId("balance")).toHaveText("50");
    await expect(page.getByTestId("cart-total")).toHaveText("0");
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
    await expect(page.getByTestId("cart-total")).toHaveText("4.8");
    await expect(page.getByTestId("balance")).toHaveText("50");
    // And the button is usable again, not stuck on "Check your wallet…".
    await expect(page.getByTestId("pay-wallet")).toBeEnabled();
  });

  test("an empty cart is refused before the wallet is ever opened", async ({ page }) => {
    await installWallet(page, { chainId: "0x19" });
    await guest(page);
    await page.getByTestId("pay-wallet").click();
    await expect(lastLine(page)).toContainText("empty");
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
    await expect(row).toContainText("2 USDC");
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
    await expect(page.getByTestId("cart-total")).toHaveText("4.8");
  });

  test("a reverted transaction is not a payment", async ({ page }) => {
    await installWallet(page, { chainId: "0x19", fate: FATE.reverted });
    await guest(page);
    await dish(page, "latte").click();
    await page.getByTestId("pay-wallet").click();
    await expect(lastLine(page)).toContainText("reverted");
    await expect(page.getByTestId("paid-banner")).toBeHidden();
    await expect(page.getByTestId("cart-total")).toHaveText("4.8");
  });

  test("a transaction that paid too little is refused with the shortfall", async ({ page }) => {
    await installWallet(page, { chainId: "0x19", fate: FATE.underpaid });
    await guest(page);
    await dish(page, "latte").click();
    await page.getByTestId("pay-wallet").click();
    await expect(lastLine(page)).toContainText("bill is 4.8");
    await expect(page.getByTestId("paid-banner")).toBeHidden();
    await expect(page.getByTestId("cart-total")).toHaveText("4.8");
  });

  test("the guest is told the chain is being checked", async ({ page }) => {
    await installWallet(page, { chainId: "0x19" });
    await guest(page);
    await dish(page, "latte").click();
    await page.getByTestId("pay-wallet").click();
    await expect(page.getByTestId("transcript")).toContainText("Checking Cronos Mainnet");
    await expect(page.getByTestId("paid-banner")).toBeVisible();
  });

  test("play money still works alongside the chain", async ({ page }) => {
    await installWallet(page, { chainId: "0x19" });
    await guest(page);
    await dish(page, "latte").click();
    await page.getByTestId("pay-usdc").click();
    await expect(page.getByTestId("paid-banner")).toContainText("Paid 4.8 USDC");
    // The grant pays this one, and there is no chain link to follow.
    await expect(page.getByTestId("balance")).toHaveText("45.2");
    await expect(page.getByTestId("paid-link")).toHaveCount(0);
    const calls = await walletCalls(page);
    expect(calls.filter((c) => c.method === "eth_sendTransaction")).toHaveLength(0);
  });
});
