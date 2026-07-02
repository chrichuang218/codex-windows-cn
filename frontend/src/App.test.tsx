import { render, screen } from "@testing-library/react";
import { describe, expect, test } from "vitest";
import { App } from "./App";
import type { AppBridge } from "./bridge";

describe("Codex Windows 中文助手 shell", () => {
  test("renders the v1 boundary and five main paths from the bridge", async () => {
    const bridge: AppBridge = {
      getAppStatus: async () => ({
        productName: "Codex Windows 中文助手",
        v1Boundary: "中文安装更新助手",
        mainPaths: [
          "install",
          "proxyLaunch",
          "checkAndUpdate",
          "uninstall",
          "launcherSelfUpdate"
        ]
      })
    };

    render(<App bridge={bridge} />);

    expect(await screen.findByRole("heading", { name: "Codex Windows 中文助手" })).toBeVisible();
    expect(screen.getByText("中文安装更新助手")).toBeVisible();
    expect(screen.getByText("安装")).toBeVisible();
    expect(screen.getByText("代理启动")).toBeVisible();
    expect(screen.getByText("检查更新 / 更新")).toBeVisible();
    expect(screen.getByText("卸载")).toBeVisible();
    expect(screen.getByText("自更新")).toBeVisible();
  });
});
