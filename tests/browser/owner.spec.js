import { test, expect } from "@playwright/test";
import { guest, owner, say, lastLine, dish, uniqueDish } from "./cafe.mjs";

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

  test("the quick buttons reach payments and orders", async ({ page }) => {
    await owner(page);
    await page.getByTestId("quick-list_payments").click();
    await expect(page.getByTestId("payments-list")).toContainText("Payments");
    await page.getByTestId("quick-list_orders").click();
    await expect(page.getByTestId("orders-list")).toBeVisible();
  });
});
