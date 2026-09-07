/* The shop on a static host — Cloudflare Pages, GitHub Pages, a bucket.
   Nothing is running behind the page: /health goes to a 404 and the tab has
   to work that out for itself. Every other tab-only spec forces the fallback
   with ?local; this one is the deployment as it will actually be served, so
   it is the spec that says a deploy works. */
import { test, expect } from "@playwright/test";
import { dish, cartLine, payAndNumber, myOrder, ticket, say, GRANT } from "./cafe.mjs";

/** Open the deployed page and watch every socket and request it makes. */
async function openPages(page, path = "/") {
  await page.addInitScript(() => {
    const Real = window.WebSocket;
    window.__sockets = [];
    const Wrapped = function (url, ...rest) {
      window.__sockets.push(String(url));
      return new Real(url, ...rest);
    };
    for (const k of ["CONNECTING", "OPEN", "CLOSING", "CLOSED"]) Wrapped[k] = Real[k];
    Wrapped.prototype = Real.prototype;
    window.WebSocket = Wrapped;
  });
  const seen = [];
  page.on("request", (r) => seen.push(r.url()));
  await page.goto(path);
  await expect(page.getByTestId("login-guest")).toBeVisible();
  return {
    sockets: () => page.evaluate(() => window.__sockets),
    requests: () => seen.slice(),
  };
}

async function guestOnPages(page, name = "Mei") {
  const watch = await openPages(page);
  await page.getByTestId("guest-name").fill(name);
  await page.getByTestId("login-guest").click();
  await expect(page.getByTestId("stage-app")).toBeVisible();
  return watch;
}

test.describe("the shop on a static host", () => {
  test("no cafe answers, so the tab becomes the cafe on its own", async ({ page }) => {
    const watch = await openPages(page);
    // The page really asked, and really got nothing — this is the branch a
    // deploy takes, not the ?local shortcut the other specs use.
    const health = watch.requests().filter((u) => u.endsWith("/health"));
    expect(health, "the page must look for a server before giving up on one").toHaveLength(1);
    await expect(page.getByTestId("local-note")).toContainText("This tab is the whole cafe");

    await page.getByTestId("login-guest").click();
    await expect(page.getByTestId("stage-app")).toBeVisible();
    await page.waitForTimeout(400);
    expect(await watch.sockets()).toEqual([]);
  });

  test("the engine is fetched from beside the page and nothing else is", async ({ page }) => {
    const watch = await guestOnPages(page);
    const outside = watch
      .requests()
      .filter((u) => !u.startsWith("http://127.0.0.1:") && !u.startsWith("data:"));
    expect(outside, "a deployed shop calls nobody until the owner names a model").toEqual([]);
    expect(watch.requests().some((u) => u.endsWith("causewaybay_panda_web_bg.wasm"))).toBeTruthy();
  });

  test("a guest orders, pays and is served, all inside the tab", async ({ page }) => {
    await guestOnPages(page);
    await expect(page.getByTestId("balance")).toHaveText(GRANT);
    await expect(page.getByTestId("mode-badge")).toContainText("Causewaybay Coin");
    await dish(page, "latte").click();
    await expect(cartLine(page, "latte")).toContainText("1×");
    await expect(page.getByTestId("cart-total")).toHaveText("HK$38.00");
    const no = await payAndNumber(page);
    await expect(myOrder(page, no)).toContainText("Order received");
    await expect(page.getByTestId("balance")).toHaveText("HK$352.00");

    // Chat goes through the same parser the server would have run.
    await say(page, "two egg tarts");
    await expect(cartLine(page, "egg_tart")).toContainText("2×");
  });

  test("the shop is still there after a reload", async ({ page }) => {
    await guestOnPages(page, "Mei");
    await dish(page, "latte").click();
    const no = await payAndNumber(page);
    await page.reload();
    await expect(page.getByTestId("stage-app")).toBeVisible();
    await expect(page.getByTestId("role-label")).toHaveText("guest");
    await expect(myOrder(page, no)).toBeVisible();
  });

  test("the counter runs the kitchen with no server behind it", async ({ page, context }) => {
    await guestOnPages(page, "Kitchen Chan");
    await dish(page, "egg_tart").click();
    const no = await payAndNumber(page);

    // The same tab is the counter too: leave, and come back as the owner.
    await page.getByTestId("leave").click();
    await page.getByTestId("owner-pin").fill("anything");
    await page.getByTestId("login-owner").click();
    await expect(page.getByTestId("owner-tools")).toBeVisible();
    await expect(ticket(page, no)).toBeVisible();
    await expect(page.getByTestId("takings-total")).not.toHaveText("HK$0.00");

    await ticket(page, no).getByTestId(`ticket-next-${no}`).click();
    await expect(ticket(page, no)).toHaveClass(/preparing/);
    await expect(ticket(page, no).getByTestId(`ticket-next-${no}`)).toHaveText("Mark ready");
    expect(await context.pages()[0].evaluate(() => window.__sockets)).toEqual([]);
  });

  test("the owner puts a dish on the board and sets the shop up", async ({ page }) => {
    await openPages(page);
    await page.getByTestId("login-owner").click();
    await expect(page.getByTestId("owner-tools")).toBeVisible();

    await page.getByTestId("new-name").fill("Pages pudding");
    await page.getByTestId("new-price").fill("44");
    await page.getByTestId("new-cat").fill("dessert");
    await page.getByTestId("new-add").click();
    await expect(dish(page, "pages_pudding")).toContainText("HK$44.00");

    await page.getByTestId("shop-setup").locator("summary").click();
    await page.getByTestId("setup-name").fill("Pages Panda");
    await page.getByTestId("setup-save").click();
    await expect(page.getByTestId("cafe-name-top")).toHaveText("Pages Panda");
    await page.reload();
    await expect(page.getByTestId("cafe-name-door")).toHaveText("Pages Panda");
  });

  /* The one key in the whole system. It is the owner's, it is typed into
     their own tab, and it goes straight from that tab to x.ai — there is no
     server in between that could have held it, and none is shipped with one. */
  test("the owner's own Grok key is used from the tab and is in nothing that was served", async ({
    page,
  }) => {
    const asked = [];
    await page.route("https://api.x.ai/**", async (route) => {
      const req = route.request();
      asked.push({ auth: req.headers()["authorization"], body: req.postDataJSON() });
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          choices: [{ message: { content: '{"intent":"add","item_id":"latte","qty":2}' } }],
        }),
      });
    });

    const watch = await openPages(page);
    // Nothing served carries a credential: the shop arrives without one.
    for (const url of watch.requests()) {
      const body = await (await page.request.get(url)).text().catch(() => "");
      expect(body).not.toMatch(/xai-[A-Za-z0-9]/);
    }

    await page.getByTestId("login-owner").click();
    await page.getByTestId("ai-setup").locator("summary").click();
    // Grok is the choice the shop offers first; the owner only pastes a key.
    await expect(page.getByTestId("ai-provider")).toHaveValue("grok");
    await page.getByTestId("ai-key").fill("xai-owners-own-key");
    await page.getByTestId("ai-save").click();
    await expect(page.getByTestId("ai-status")).toContainText("Grok");

    await page.getByTestId("leave").click();
    await page.getByTestId("guest-name").fill("Mei");
    await page.getByTestId("login-guest").click();
    await say(page, "something warm for a cold morning please");
    await expect(cartLine(page, "latte")).toContainText("2×", { timeout: 10_000 });

    expect(asked).toHaveLength(1);
    expect(asked[0].auth).toBe("Bearer xai-owners-own-key");
    expect(asked[0].body.model).toBe("grok-4-fast");
    // The key never went anywhere else — there is nowhere else.
    expect(await watch.sockets()).toEqual([]);

    await page.getByTestId("leave").click();
    await page.getByTestId("login-owner").click();
    await page.getByTestId("ai-setup").locator("summary").click();
    await page.getByTestId("ai-off").click();
    await expect(page.getByTestId("ai-status")).toHaveText("Local parser only");
  });
});
