import { expect, test } from "@playwright/test";

test("auth login opens the lobby and creates a room", async ({ page }) => {
  const browserErrors = [];
  page.on("console", (message) => {
    if (message.type() === "error") {
      browserErrors.push(message.text());
    }
  });
  page.on("pageerror", (error) => {
    browserErrors.push(error.message);
  });

  await page.goto("/");

  await expect(page).toHaveTitle("登录 - Remote Voice");
  await expect(page).toHaveURL(/\/login\?next=%2F$/);
  await page.getByLabel("用户名").fill("admin");
  await page.getByLabel("密码").fill("password");
  await page.getByRole("button", { name: "登录" }).click();

  await expect(page).toHaveTitle("Remote Voice");
  await expect(page.getByRole("heading", { name: "进入语音房间" })).toBeVisible();
  await expect(page.locator("#auth-controls")).toContainText("管理员");

  await page.getByLabel("昵称").fill("浏览器房主");
  await page.getByRole("button", { name: "创建" }).click();

  await expect(page).toHaveURL(/\/rooms\/[A-Z0-9]+$/);
  await expect(page.locator("#room-connection")).toHaveText("已连接");
  await expect(page.locator("#members-meta")).toHaveText("1 位成员");
  await expect(page.locator("#member-list")).toContainText("浏览器房主");
  await expect(page.locator("#member-list")).toContainText("房主");

  expect(browserErrors).toEqual([]);
});
