import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, test } from "vitest";
import { App } from "./App";
import type { AppBridge, InstallerDefaults } from "./bridge";

const installerDefaults: InstallerDefaults = {
  recommendedMode: "user",
  recommendedFetcher: "direct",
  modes: [
    {
      mode: "portable",
      label: "便携模式",
      defaultRoot: "D:\\Tools\\CodexPortable",
      createShortcut: false,
      registerUninstall: false,
      keepVersions: 2,
      useCurrentJunction: true
    },
    {
      mode: "user",
      label: "当前用户",
      defaultRoot: "C:\\Users\\tester\\AppData\\Local\\Codex",
      createShortcut: true,
      registerUninstall: true,
      keepVersions: 2,
      useCurrentJunction: true
    },
    {
      mode: "system",
      label: "所有用户",
      defaultRoot: "C:\\Program Files\\Codex",
      createShortcut: true,
      registerUninstall: true,
      keepVersions: 2,
      useCurrentJunction: true
    }
  ],
  fetchers: ["direct", "winget", "localFile"]
};

afterEach(() => {
  cleanup();
});

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
      }),
      getInstallerDefaults: async () => installerDefaults,
      startInstall: async () => ({ accepted: true }),
      onInstallEvent: () => () => {}
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

  test("renders the install wizard from installer defaults", async () => {
    const bridge: AppBridge = {
      getAppStatus: async () => ({
        productName: "Codex Windows 中文助手",
        v1Boundary: "中文安装更新助手",
        mainPaths: ["install", "proxyLaunch", "checkAndUpdate", "uninstall", "launcherSelfUpdate"]
      }),
      getInstallerDefaults: async () => installerDefaults,
      startInstall: async () => ({ accepted: true }),
      onInstallEvent: () => () => {}
    };

    render(<App bridge={bridge} />);

    expect(await screen.findByRole("heading", { name: "安装 Codex" })).toBeVisible();
    expect(screen.getByRole("button", { name: /当前用户/ })).toHaveAttribute(
      "aria-pressed",
      "true"
    );
    expect(screen.getByDisplayValue("C:\\Users\\tester\\AppData\\Local\\Codex")).toBeVisible();
    expect(screen.getByText("保留 2 个版本")).toBeVisible();
    expect(screen.getByText("创建开始菜单快捷方式")).toBeVisible();
    expect(screen.getByText("写入 Windows 卸载入口")).toBeVisible();
  });

  test("starts installation with the selected defaults", async () => {
    let submittedRoot = "";
    const bridge: AppBridge = {
      getAppStatus: async () => ({
        productName: "Codex Windows 中文助手",
        v1Boundary: "中文安装更新助手",
        mainPaths: ["install", "proxyLaunch", "checkAndUpdate", "uninstall", "launcherSelfUpdate"]
      }),
      getInstallerDefaults: async () => installerDefaults,
      startInstall: async (request) => {
        submittedRoot = request.root;
        return { accepted: true };
      },
      onInstallEvent: () => () => {}
    };

    render(<App bridge={bridge} />);

    fireEvent.click(await screen.findByRole("button", { name: "开始安装" }));

    expect(await screen.findByText("正在安装")).toBeVisible();
    expect(submittedRoot).toBe("C:\\Users\\tester\\AppData\\Local\\Codex");
  });

  test("shows installation progress events", async () => {
    let emitInstallEvent:
      | ((event: {
          kind: "phase" | "progress" | "done" | "error";
          title: string;
          detail: string;
          progress: number | null;
          version: string | null;
          message: string | null;
        }) => void)
      | null = null;
    const bridge: AppBridge = {
      getAppStatus: async () => ({
        productName: "Codex Windows 中文助手",
        v1Boundary: "中文安装更新助手",
        mainPaths: ["install", "proxyLaunch", "checkAndUpdate", "uninstall", "launcherSelfUpdate"]
      }),
      getInstallerDefaults: async () => installerDefaults,
      startInstall: async () => ({ accepted: true }),
      onInstallEvent: (handler) => {
        emitInstallEvent = handler;
        return () => {};
      }
    };

    render(<App bridge={bridge} />);
    fireEvent.click(await screen.findByRole("button", { name: "开始安装" }));

    act(() => {
      emitInstallEvent?.({
        kind: "phase",
        title: "正在下载",
        detail: "通过直连 Microsoft Store",
        progress: null,
        version: null,
        message: null
      });
    });

    expect(await screen.findByText("正在下载")).toBeVisible();
    expect(screen.getByText("通过直连 Microsoft Store")).toBeVisible();

    act(() => {
      emitInstallEvent?.({
        kind: "progress",
        title: "安装进度",
        detail: "",
        progress: 0.35,
        version: null,
        message: null
      });
    });

    expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "35");
  });
});
