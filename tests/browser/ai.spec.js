/* A model listening on the server. This project's cafe is pointed at a
   stand-in provider the harness runs — OpenAI-shaped, keyed on a few words —
   so a free sentence travels the whole way: page → socket → parser gives up
   → Rust asks the model → intent → till → cart, with nothing on the wire but
   the stand-in. */
import { test, expect } from "@playwright/test";
import { guest, owner, say, lastLine, cartLine } from "./cafe.mjs";

test.describe("a model listens on the server", () => {
  test("the shop reports who is listening", async ({ request }) => {
    const body = await (await request.get("/health")).json();
    expect(body.ai).toBe("openrouter");
    expect(body.grok).toBe(true);
  });

  test("what the parser cannot read, the model can", async ({ page }) => {
    await guest(page);
    // Nothing here names a dish; the parser alone would shrug.
    await say(page, "something warm to hold, please");
    await expect(cartLine(page, "latte")).toContainText("2×", { timeout: 10_000 });
    await expect(page.getByTestId("cart-total")).toHaveText("HK$76.00");
    await say(page, "and a little sweet thing");
    await expect(cartLine(page, "egg_tart")).toContainText("1×", { timeout: 10_000 });
    await expect(page.getByTestId("cart-total")).toHaveText("HK$86.00");
  });

  test("the parser still answers first, without a round trip", async ({ page }) => {
    await guest(page);
    // A plain dish name never leaves the shop: the stand-in would have
    // answered "help" to this, and it did not get the chance.
    await say(page, "two pineapple buns");
    await expect(cartLine(page, "pineapple_bun")).toContainText("2×");
    await expect(page.getByTestId("cart-total")).toHaveText("HK$24.00");
  });

  test("the model can settle the bill", async ({ page }) => {
    await guest(page);
    await say(page, "latte");
    await expect(cartLine(page, "latte")).toBeVisible();
    await say(page, "could you sort the bill out for me");
    await expect(page.getByTestId("paid-banner")).toContainText("Paid HK$38.00", { timeout: 10_000 });
  });

  test("when the model fails, the parser's answer stands", async ({ page }) => {
    await guest(page);
    await say(page, "this one will fail");
    await expect(lastLine(page)).toContainText("did not catch", { timeout: 10_000 });
    await expect(page.getByTestId("cart-lines").locator("li")).toHaveCount(0);
  });

  test("the owner can hand the chat back to the parser, and take it up again", async ({ page, browser }) => {
    const shop = await owner(page);
    await expect(shop.getByTestId("ai-status")).toContainText("OpenRouter");
    await shop.getByTestId("ai-setup").locator("summary").click();
    await shop.getByTestId("ai-off").click();
    await expect(shop.getByTestId("ai-status")).toHaveText("Local parser only");

    // Now the same sentence gets only the parser's shrug.
    const table = await browser.newPage();
    await guest(table);
    await say(table, "something warm to hold, please");
    await expect(lastLine(table)).toContainText("did not catch");
    await expect(table.getByTestId("cart-lines").locator("li")).toHaveCount(0);

    // Back on: the key is still held, so no need to type it again.
    await shop.getByTestId("ai-provider").selectOption("openrouter");
    await shop.getByTestId("ai-save").click();
    await expect(shop.getByTestId("ai-status")).toContainText("OpenRouter");
    await say(table, "something warm to hold, please");
    await expect(cartLine(table, "latte")).toContainText("2×", { timeout: 10_000 });
    await table.close();
  });

  // The model does more than map a sentence onto the till: asked a
  // question, it answers as the panda, and any dish it names is a button.
  test("a question gets an answer in words, and a way to order what it names", async ({ page }) => {
    await guest(page);
    await say(page, "what's good here?");
    await expect(lastLine(page)).toContainText("silk milk tea", { timeout: 10_000 });
    // Two buttons: the unicorn the model made up is not on the board.
    const adds = page.getByTestId("quick-add");
    await expect(adds).toHaveCount(2);
    await expect(adds.first()).toHaveText("Silk milk tea");
    await adds.first().click();
    await expect(cartLine(page, "milk_tea")).toContainText("1×");
  });

  test("the owner asks the books and is answered from today's figures", async ({ page, browser }) => {
    const shop = await owner(page);
    const table = await browser.newPage();
    await guest(table, "Facts Mei");
    await say(table, "three egg tarts");
    await table.getByTestId("pay-usdc").click();
    await expect(table.getByTestId("paid-banner")).toBeVisible();
    await table.close();

    await say(shop, "what sold today?");
    await expect(lastLine(shop)).toContainText("Selling today", { timeout: 10_000 });
    await expect(lastLine(shop)).toContainText("Egg tart");
    // An owner is answered in words only: no order buttons at the counter.
    await expect(shop.getByTestId("quick-add")).toHaveCount(0);
    await say(shop, "how is the kitchen?");
    await expect(lastLine(shop)).toContainText(/waiting/, { timeout: 10_000 });
  });

  test("a guest asks after their own order and gets their own facts", async ({ page }) => {
    await guest(page, "Own Mei");
    await say(page, "where is my order?");
    await expect(lastLine(page)).toContainText("no order", { timeout: 10_000 });
    await say(page, "latte");
    await page.getByTestId("pay-usdc").click();
    await expect(page.getByTestId("paid-banner")).toBeVisible();
    await say(page, "where is my order?");
    await expect(lastLine(page)).toContainText("open orders: 1", { timeout: 10_000 });
  });
});
