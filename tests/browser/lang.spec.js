/* The page in six languages. English unless this browser chose otherwise;
   the choice is kept in its storage and applied before the door is drawn. */
import { test, expect } from "@playwright/test";
import { guest, owner, dish, payAndNumber, myOrder } from "./cafe.mjs";

const LANG_KEY = "causewaybay.lang";

test.describe("the page speaks the guest's language", () => {
  test("English by default, and the six choices are on the door", async ({ page }) => {
    await page.goto("/");
    await expect(page.getByTestId("login-guest")).toHaveText("I am a guest");
    const pick = page.getByTestId("lang");
    await expect(pick).toHaveValue("en");
    const labels = await pick.locator("option").allTextContents();
    expect(labels).toEqual(["English", "廣東話", "中文", "한국어", "日本語", "Čeština"]);
    expect(await page.evaluate(() => document.documentElement.lang)).toBe("en");
  });

  test("choosing Korean changes the door at once and is kept across a reload", async ({ page }) => {
    await page.goto("/");
    await page.getByTestId("lang").selectOption("ko");
    await expect(page.getByTestId("login-guest")).toHaveText("손님입니다");
    await expect(page.getByTestId("login-owner")).toHaveText("사장님입니다");
    expect(await page.evaluate((k) => localStorage.getItem(k), LANG_KEY)).toBe("ko");

    await page.reload();
    await expect(page.getByTestId("login-guest")).toHaveText("손님입니다");
    await expect(page.getByTestId("lang")).toHaveValue("ko");
    expect(await page.evaluate(() => document.documentElement.lang)).toBe("ko");

    // Inside, the guest's page is Korean too: the pay button, the chat, the card.
    await page.getByTestId("guest-name").fill("Kim");
    await page.getByTestId("login-guest").click();
    await expect(page.getByTestId("stage-app")).toBeVisible();
    await expect(page.getByTestId("role-label")).toHaveText("손님");
    await expect(page.getByTestId("pay-usdc")).toHaveText("결제");
    await expect(page.getByTestId("chat-input")).toHaveAttribute("placeholder", /라떼/);
    await expect(page.getByTestId("mode-badge")).toContainText("테스트 머니");
    await dish(page, "latte").click();
    await expect(page.getByTestId("sheet-summary")).toHaveText("1개, HK$38.00");
    const no = await payAndNumber(page);
    await expect(page.getByTestId("paid-banner")).toContainText(`주문 #${no}에 HK$38.00 결제.`);
    await expect(myOrder(page, no)).toContainText("주문 접수");
    await expect(myOrder(page, no).locator(".journey-step").first()).toContainText("접수");
    await expect(page.getByTestId("gdash-now-headline")).toHaveText(`#${no} · 접수`);

    // Back to English from the header, without leaving.
    await page.getByTestId("lang-top").selectOption("en");
    await expect(page.getByTestId("pay-usdc")).toHaveText("Pay");
    await expect(myOrder(page, no)).toContainText("Order received");
    await expect(page.getByTestId("gdash-now-headline")).toHaveText(`#${no} · Received`);
    expect(await page.evaluate((k) => localStorage.getItem(k), LANG_KEY)).toBe("en");
  });

  test("a Chinese reader gets the dish's Chinese name first; Cantonese and Mandarin differ", async ({ page }) => {
    await page.goto("/");
    await page.getByTestId("lang").selectOption("yue");
    await expect(page.getByTestId("login-guest")).toHaveText("我係客人");
    await page.getByTestId("login-guest").click();
    await expect(page.getByTestId("stage-app")).toBeVisible();
    await expect(dish(page, "latte").locator(".name")).toHaveText("熱鮮奶咖啡");
    await expect(dish(page, "latte").locator(".zh")).toHaveText("Hot latte");
    await expect(page.getByTestId("pay-usdc")).toHaveText("畀錢");

    await page.getByTestId("lang-top").selectOption("zh");
    await expect(page.getByTestId("pay-usdc")).toHaveText("付款");
    await expect(dish(page, "latte").locator(".name")).toHaveText("熱鮮奶咖啡");

    await page.getByTestId("lang-top").selectOption("ja");
    await expect(page.getByTestId("pay-usdc")).toHaveText("支払う");
    await expect(dish(page, "latte").locator(".name")).toHaveText("Hot latte");

    await page.getByTestId("lang-top").selectOption("cs");
    await expect(page.getByTestId("pay-usdc")).toHaveText("Zaplatit");
    await page.getByTestId("lang-top").selectOption("en");
  });

  test("the owner's counter follows too, and an unknown stored choice falls back to English", async ({ page }) => {
    await page.goto("/");
    await page.evaluate((k) => localStorage.setItem(k, "xx"), LANG_KEY);
    await page.reload();
    await expect(page.getByTestId("login-guest")).toHaveText("I am a guest");
    await page.getByTestId("lang").selectOption("ja");
    await page.getByTestId("owner-pin").fill("panda");
    await page.getByTestId("login-owner").click();
    await expect(page.getByTestId("owner-tools")).toBeVisible();
    await expect(page.getByTestId("role-label")).toHaveText("店主");
    await expect(page.getByTestId("kitchen-auto")).toHaveText("パンダに厨房を任せる");
    await expect(page.getByTestId("new-add")).toHaveText("メニューに追加");
    await expect(page.getByTestId("takings-count")).toContainText("件");
    await page.getByTestId("lang-top").selectOption("en");
    await expect(page.getByTestId("kitchen-auto")).toHaveText("Let the panda work the kitchen");
  });
});
