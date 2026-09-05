/* The cafe running itself on a real server. The owner throws the switch, a
   tokio task seats regulars and works the tickets, and every connected page
   hears about it through the hub — including a real guest whose own order
   the simulated kitchen picks up. The harness beats every 250 ms. */
import { test, expect } from "@playwright/test";
import { guest, owner, say, lastLine, dish, ticket, myOrder, payAndNumber } from "./cafe.mjs";

/** Whatever happens, leave the shop quiet for the next test. */
async function stopAuto(shop) {
  const sw = shop.getByTestId("auto-owner");
  if ((await sw.textContent()) === "Stop the cafe") await sw.click();
  await expect(sw).toHaveText("Run the cafe on its own");
}

/** Throw the switch on and wait for the shop to confirm it. Tickets left by
    earlier tests make "a ticket is visible" true at once, so that is never
    the signal; the button reading "Stop the cafe" is. */
async function startAuto(shop) {
  const sw = shop.getByTestId("auto-owner");
  await sw.click();
  await expect(sw).toHaveText("Stop the cafe");
}

/** An owner at a counter whose switch is known to be off, whatever an
    earlier test left behind. The switch state arrives a frame after the
    welcome, so wait for it to settle before reading it. */
async function quietOwner(page) {
  const shop = await owner(page);
  await page.waitForTimeout(300);
  await stopAuto(shop);
  return shop;
}

test.describe("the cafe runs itself on the server", () => {
  test("the owner throws the switch and tickets arrive on their own", async ({ page }) => {
    const shop = await quietOwner(page);
    const sw = shop.getByTestId("auto-owner");
    const before = Number((await shop.getByTestId("takings-count").innerText()).match(/\d+/)[0]);
    try {
      await startAuto(shop);
      await expect(shop.getByTestId("auto-dot")).toBeVisible();
      await expect(lastLine(shop)).toContainText("running on its own");

      // Nobody else is in the room, yet tickets land and money comes in.
      await expect(shop.locator(".ticket-row").first()).toBeVisible({ timeout: 10_000 });
      await expect
        .poll(async () => Number((await shop.getByTestId("takings-count").innerText()).match(/\d+/)[0]), {
          timeout: 15_000,
        })
        .toBeGreaterThanOrEqual(before + 3);
      // Tickets name regulars, not blanks.
      await expect(shop.locator(".ticket-row").first()).toContainText(/Mei|Wing|Ling|Kwok|Ah Fai|Suet|Chan|Yuki|Ho Yin|Priya/);
    } finally {
      await stopAuto(shop);
    }
    await expect(shop.getByTestId("auto-dot")).toBeHidden();
  });

  test("a second owner sees the switch is on, and only the owner may throw it", async ({ page, browser }) => {
    const shop = await quietOwner(page);
    try {
      await startAuto(shop);

      // A second counter opened now is told at once.
      const second = await browser.newPage();
      await owner(second);
      await expect(second.getByTestId("auto-owner")).toHaveText("Stop the cafe");
      await expect(second.getByTestId("auto-dot")).toBeVisible();
      await second.close();

      // A guest on a server has no switch and is refused if they ask.
      const table = await browser.newPage();
      await guest(table);
      await expect(table.getByTestId("auto-guest")).toBeHidden();
      await say(table, "auto");
      await expect(lastLine(table)).toContainText("only the owner");
      await table.close();
    } finally {
      await stopAuto(shop);
    }
  });

  test("a real guest's order is worked by the simulated kitchen too", async ({ page, browser }) => {
    const shop = await quietOwner(page);
    const table = await browser.newPage();
    await guest(table, "Real Mei");
    await dish(table, "egg_tart").click();
    const no = await payAndNumber(table);
    await expect(myOrder(table, no)).toContainText("Order received");
    await expect(ticket(shop, no)).toBeVisible();
    try {
      await startAuto(shop);
      // Oldest first: the real ticket moves before the regulars' do.
      await expect(myOrder(table, no)).toHaveCount(0, { timeout: 20_000 });
      await expect(ticket(shop, no)).toHaveCount(0);
    } finally {
      await stopAuto(shop);
      await table.close();
    }
  });

  test("stopping stops: no new tickets once the switch is off", async ({ page }) => {
    const shop = await quietOwner(page);
    await startAuto(shop);
    await expect(shop.locator(".ticket-row").first()).toBeVisible({ timeout: 10_000 });
    await stopAuto(shop);
    // The reply itself is pinned over a raw socket in ws_flow.rs; here the
    // test is the effect — the counter goes quiet. A beat that began just
    // before the switch still lands, so let two pass before taking the mark.
    await shop.waitForTimeout(600);
    const count = await shop.getByTestId("takings-count").innerText();
    await shop.waitForTimeout(1500); // six beats' worth
    await expect(shop.getByTestId("takings-count")).toHaveText(count);
  });

  test("chat throws the switch too", async ({ page }) => {
    const shop = await quietOwner(page);
    try {
      await say(shop, "run the cafe on its own");
      await expect(shop.getByTestId("auto-owner")).toHaveText("Stop the cafe");
      await say(shop, "stop");
      await expect(shop.getByTestId("auto-owner")).toHaveText("Run the cafe on its own");
    } finally {
      await stopAuto(shop);
    }
  });
});
