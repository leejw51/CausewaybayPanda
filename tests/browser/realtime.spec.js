/* Two people in the cafe at once. The owner writes the board, the guest reads
   it, and neither has to reload. */
import { test, expect } from "@playwright/test";
import { guest, owner, say, dish, uniqueDish } from "./cafe.mjs";

test.describe("the room stays in step", () => {
  test("a dish the owner hides leaves the guest's board", async ({ page, browser }) => {
    const shop = await owner(page);
    const table = await browser.newPage();
    await guest(table);
    await expect(dish(table, "french_toast")).toBeVisible();

    await say(shop, "hide french toast");
    await expect(dish(table, "french_toast")).toHaveCount(0);

    // And it comes back the same way.
    await say(shop, "show french toast");
    await expect(dish(table, "french_toast")).toBeVisible();
    await table.close();
  });

  test("a guest cannot order a dish that is off the board", async ({ page, browser }) => {
    const shop = await owner(page);
    const table = await browser.newPage();
    await guest(table);

    await say(shop, "hide yuenyeung");
    await expect(dish(table, "yuenyeung")).toHaveCount(0);
    await say(table, "yuenyeung");
    await expect(table.getByTestId("cart-lines").locator("li")).toHaveCount(0);

    await say(shop, "show yuenyeung");
    await expect(dish(table, "yuenyeung")).toBeVisible();
    await table.close();
  });

  test("a dish the owner adds appears on the guest's board", async ({ page, browser }) => {
    const { name, id } = uniqueDish("tofufa");
    const shop = await owner(page);
    const table = await browser.newPage();
    await guest(table);
    await expect(dish(table, id)).toHaveCount(0);

    await say(shop, `add item ${name} 2.80 dessert`);
    await expect(dish(table, id)).toBeVisible();

    // And the guest can order it straight away, by name.
    await say(table, name);
    await expect(table.getByTestId(`cart-${id}`)).toBeVisible();
    await expect(table.getByTestId("cart-total")).toHaveText("2.8");
    await table.close();
  });

  test("a payment reaches the owner without them asking", async ({ page, browser }) => {
    const shop = await owner(page);
    const table = await browser.newPage();
    await guest(table, "Suet");
    await dish(table, "milk_tea").click();
    await table.getByTestId("pay-usdc").click();
    await expect(table.getByTestId("paid-banner")).toContainText("Paid 3.6 USDC");

    await expect(
      shop.getByTestId("payment-row").filter({ hasText: "Suet" }).first()
    ).toContainText("3.6 USDC");
    await table.close();
  });

  test("two guests keep separate carts and separate grants", async ({ browser }) => {
    const a = await browser.newPage();
    const b = await browser.newPage();
    await guest(a, "Ah Fai");
    await guest(b, "Ah Ming");

    await dish(a, "latte").click();
    await expect(a.getByTestId("cart-total")).toHaveText("4.8");
    await expect(b.getByTestId("cart-total")).toHaveText("0");
    await expect(b.getByTestId("cart-lines").locator("li")).toHaveCount(0);

    await a.getByTestId("pay-usdc").click();
    await expect(a.getByTestId("balance")).toHaveText("45.2");
    await expect(b.getByTestId("balance")).toHaveText("50");
    await a.close();
    await b.close();
  });
});

test.describe("the server", () => {
  test("answers /health with the cafe and its menu size", async ({ request }) => {
    const res = await request.get("/health");
    expect(res.ok()).toBeTruthy();
    const body = await res.json();
    expect(body.ok).toBe(true);
    expect(body.cafe).toBe("Causewaybay Coffee");
    expect(body.menu).toBeGreaterThan(0);
    // The harness withholds any XAI key, so the suite exercises the local
    // parser and never a live model. PANDA_PW_GROK=1 opts back in.
    expect(body.grok).toBe(process.env.PANDA_PW_GROK === "1");
  });
});
