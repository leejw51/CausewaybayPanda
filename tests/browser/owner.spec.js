import { test, expect } from "@playwright/test";
import { guest, owner, say, lastLine, dish, uniqueDish, payAndNumber, ticket } from "./cafe.mjs";

test.describe("the owner runs the shop", () => {
  test("the tools lead the column and the guest cart is gone", async ({ page }) => {
    await owner(page);
    const tools = page.getByTestId("owner-tools");
    await expect(tools).toBeVisible();
    await expect(page.locator(".ticket")).toBeHidden();
    // The tools come first: beside the board on a laptop, above it on a
    // phone — never below eleven dishes.
    const t = await tools.evaluate((el) => el.getBoundingClientRect());
    const g = await page.getByTestId("menu-grid").evaluate((el) => el.getBoundingClientRect());
    expect(t.left < g.left || t.top < g.top).toBeTruthy();
    expect(t.top).toBeLessThanOrEqual(g.top);
  });

  test("the form puts a dish on the board", async ({ page }) => {
    const { name, id } = uniqueDish("pudding");
    await owner(page);
    await page.getByTestId("new-name").fill(name);
    await page.getByTestId("new-price").fill("38");
    await page.getByTestId("new-cat").fill("dessert");
    await page.getByTestId("new-add").click();
    await expect(page.getByTestId("transcript")).toContainText(name);
    await expect(dish(page, id)).toBeVisible();
    await expect(dish(page, id)).toContainText("HK$38.00");
  });

  // A dish the owner just wrote has no picture; an empty <img src> drew a
  // broken-image glyph, so a lettered tile stands in.
  test("a new dish shows a lettered plate, not a broken image", async ({ page }) => {
    const { name, id } = uniqueDish("sundae");
    await owner(page);
    await page.getByTestId("new-name").fill(name);
    await page.getByTestId("new-price").fill("40");
    await page.getByTestId("new-add").click();
    await expect(dish(page, id)).toBeVisible();
    await expect(dish(page, id).locator("img")).toHaveCount(0);
    const plate = dish(page, id).locator(".plate");
    await expect(plate).toBeVisible();
    await expect(plate).toHaveText(name.charAt(0).toUpperCase());
  });

  test("the form refuses a dish with no price", async ({ page }) => {
    await owner(page);
    const before = await page.getByTestId("menu-grid").locator(".dish").count();
    await page.getByTestId("new-name").fill("Priceless thing");
    await page.getByTestId("new-price").fill("");
    await page.getByTestId("new-add").click();
    await page.waitForTimeout(300);
    await expect(page.getByTestId("menu-grid").locator(".dish")).toHaveCount(before);
  });

  test("chat puts a dish on the board", async ({ page }) => {
    const { name, id } = uniqueDish("jelly");
    await owner(page);
    await say(page, `add item ${name} 36 dessert`);
    await expect(dish(page, id)).toBeVisible();
    await expect(dish(page, id)).toContainText("HK$36.00");
  });

  // "hide macaroni" names a dish, and the matcher used to read it as an order,
  // which the server then refused because owners do not order.
  test("chat takes a dish off the board and puts it back", async ({ page }) => {
    await owner(page);
    await say(page, "hide macaroni");
    await expect(lastLine(page)).toContainText("off the board");
    await expect(dish(page, "macaroni")).toHaveClass(/off/);

    await say(page, "show macaroni");
    await expect(lastLine(page)).toContainText("back on the board");
    await expect(dish(page, "macaroni")).not.toHaveClass(/off/);
  });

  test("tapping a dish toggles it off the board and back", async ({ page }) => {
    await owner(page);
    const tile = dish(page, "panda_bun");
    await expect(tile).not.toHaveClass(/off/);

    await tile.click();
    await expect(tile).toHaveClass(/off/);
    await expect(tile).toContainText("off the board");

    await tile.click();
    await expect(tile).not.toHaveClass(/off/);
    await expect(tile).not.toContainText("off the board");
  });

  test("the owner does not order from their own board", async ({ page }) => {
    await owner(page);
    await say(page, "latte");
    await expect(lastLine(page)).toContainText("guests order from it");
    await expect(page.getByTestId("cart-lines").locator("li")).toHaveCount(0);
  });

  test("the owner cannot pay a till", async ({ page }) => {
    await owner(page);
    await expect(page.getByTestId("pay-usdc")).toBeHidden();
    await say(page, "pay");
    await expect(lastLine(page)).toContainText("does not pay");
  });

  test("the payments list names the guest and the amount", async ({ page, browser }) => {
    const shop = await owner(page);
    const buyer = await browser.newPage();
    await guest(buyer, "Ling");
    await dish(buyer, "lemon_tea").click();
    await buyer.getByTestId("pay-usdc").click();
    await expect(buyer.getByTestId("paid-banner")).toContainText("Paid HK$26.00");

    await say(shop, "payments");
    const row = shop.getByTestId("payment-row").filter({ hasText: "Ling" }).first();
    await expect(row).toContainText("HK$26.00");
    await expect(row).toContainText("coin");
    await buyer.close();
  });

  test("the orders book records what was paid", async ({ page, browser }) => {
    const shop = await owner(page);
    const buyer = await browser.newPage();
    await guest(buyer, "Kwok");
    await dish(buyer, "cappuccino").click();
    await buyer.getByTestId("pay-usdc").click();
    await expect(buyer.getByTestId("paid-banner")).toBeVisible();

    await say(shop, "orders");
    const row = shop.getByTestId("order-row").filter({ hasText: "Kwok" }).first();
    await expect(row).toContainText("HK$38.00");
    await expect(row).toContainText("placed");
    await buyer.close();
  });

  // The owner chooses who listens to the chat, from the counter. Nothing here
  // reaches a model: choosing one is a setting, not a call.
  test("the owner can choose which model listens, and the key is never shown", async ({ page }) => {
    await owner(page);
    const setup = page.getByTestId("ai-setup");
    await expect(page.getByTestId("ai-status")).toHaveText("Local parser only");
    await setup.locator("summary").click();

    const sel = page.getByTestId("ai-provider");
    await expect(sel.locator("option")).toHaveCount(5);
    for (const key of ["grok", "openai", "anthropic", "openrouter", "ollama"]) {
      await expect(sel.locator(`option[value="${key}"]`)).toHaveCount(1);
    }

    // A key is required where the provider wants one.
    await sel.selectOption("anthropic");
    await expect(page.getByTestId("ai-hint")).toContainText("console.anthropic.com");
    await page.getByTestId("ai-save").click();
    await expect(lastLine(page)).toContainText("needs an API key");
    await expect(page.getByTestId("ai-status")).toHaveText("Local parser only");

    // With one, it is on — and the box is emptied so the key is not on screen.
    await sel.selectOption("openrouter");
    await page.getByTestId("ai-key").fill("or-test-key");
    await page.getByTestId("ai-save").click();
    await expect(page.getByTestId("ai-status")).toContainText("OpenRouter");
    await expect(page.getByTestId("ai-status")).toContainText("openai/gpt-4o-mini");
    await expect(page.getByTestId("ai-key")).toHaveValue("");
    await expect(page.getByTestId("ai-key")).toHaveAttribute("placeholder", /key is held/);

    // Ollama needs no key at all.
    await sel.selectOption("ollama");
    await expect(page.getByTestId("ai-key")).toBeDisabled();

    // And back to the parser alone.
    await page.getByTestId("ai-off").click();
    await expect(page.getByTestId("ai-status")).toHaveText("Local parser only");
  });

  test("the quick buttons reach payments and orders", async ({ page }) => {
    await owner(page);
    await page.getByTestId("quick-list_payments").click();
    await expect(page.getByTestId("payments-list")).toContainText("Payments");
    await page.getByTestId("quick-list_orders").click();
    await expect(page.getByTestId("orders-list")).toBeVisible();
  });

  // The day on one card. The shop is shared across the run, so every figure
  // is read before and after rather than assumed.
  test("the dashboard moves with a payment and with the kitchen", async ({ page, browser }) => {
    const shop = await owner(page);
    const dash = shop.getByTestId("dash-owner");
    await expect(dash).toBeVisible();
    const num = async (id) => Number(await shop.getByTestId(id).innerText());
    const openBefore = await num("dash-open");
    const guestsBefore = await num("dash-guests");
    const doneBefore = await num("dash-done");

    const table = await browser.newPage();
    await guest(table, `Dash ${Math.random().toString(36).slice(2, 6)}`);
    await say(table, "two egg tarts");
    const no = await payAndNumber(table);

    await expect(shop.getByTestId("dash-open")).toHaveText(String(openBefore + 1));
    await expect(shop.getByTestId("dash-guests")).toHaveText(String(guestsBefore + 1));
    await expect(shop.getByTestId("dash-open-split")).toContainText("waiting");
    await expect(shop.getByTestId("dash-average")).toContainText("HK$");
    const tart = shop.getByTestId("dash-dish-egg_tart");
    await expect(tart).toBeVisible();
    await expect(tart).toContainText("× ·");
    await expect(shop.getByTestId("dash-top-empty")).toBeHidden();

    // Work the ticket through: the kitchen figures follow, then the count of done.
    const next = shop.getByTestId(`ticket-next-${no}`);
    await next.click();
    await expect(shop.getByTestId("dash-open-split")).toContainText("making");
    await next.click();
    await expect(shop.getByTestId("dash-open-split")).toContainText("ready");
    await next.click();
    await expect(ticket(shop, no)).toHaveCount(0);
    await expect(shop.getByTestId("dash-open")).toHaveText(String(openBefore));
    await expect(shop.getByTestId("dash-done")).toHaveText(String(doneBefore + 1));
    await table.close();
  });

  test("saying today reads the card back in the chat", async ({ page }) => {
    await owner(page);
    await say(page, "today");
    // A refresh, not a chat line: the card is the answer.
    await expect(page.getByTestId("dash-owner")).toBeVisible();
    await expect(page.getByTestId("dash-average")).toContainText("HK$");
  });
});
