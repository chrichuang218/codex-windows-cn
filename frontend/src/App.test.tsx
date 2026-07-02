import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
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

const updateBridgeDefaults = {
  checkUpdateStatus: async () => ({
    kind: "available" as const,
    title: "发现 Codex 新版本",
    message: "当前版本 1.0.0，可更新到 1.2.0",
    currentVersion: "1.0.0",
    latestVersion: "1.2.0",
    actions: [
      "updateNow" as const,
      "notNow" as const,
      "skipThisVersion" as const,
      "snoozeOneDay" as const,
      "snoozeSevenDays" as const,
      "never" as const
    ]
  }),
  startUpdate: async () => ({ accepted: true }),
  applyUpdateAction: async () => ({ applied: true, message: "已保存更新提醒设置" }),
  onUpdateEvent: () => () => {}
};

const uninstallBridgeDefaults = {
  getUninstallConfirmation: async () => ({
    title: "确认卸载 Codex Windows 中文助手",
    root: "C:\\Users\\tester\\AppData\\Local\\Codex",
    deleteItems: ["已安装的 Codex 版本", "下载缓存", "启动器配置"],
    preserveItems: ["Codex 登录数据", "日志和诊断信息"]
  }),
  getUninstallStatus: async () => ({
    kind: "ready" as const,
    title: "可以卸载",
    message: "将只删除启动器管理的文件"
  }),
  startUninstall: async () => ({ accepted: true }),
  onUninstallEvent: () => () => {}
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
      onInstallEvent: () => () => {},
      getProxyLaunchStatus: async () => ({
        canLaunch: true,
        codexExe: "C:\\Users\\tester\\AppData\\Local\\Codex\\versions\\1.2.0\\Codex.exe",
        message: "可以启动 Codex"
      }),
      launchCodex: async () => ({ launched: true, message: "已启动 Codex" }),
      ...updateBridgeDefaults,
      ...uninstallBridgeDefaults
    };

    render(<App bridge={bridge} />);

    expect(await screen.findByRole("heading", { name: "Codex Windows 中文助手" })).toBeVisible();
    expect(screen.getByText("中文安装更新助手")).toBeVisible();
    const mainPaths = within(screen.getByLabelText("v1 五条主路径"));
    expect(mainPaths.getByText("安装")).toBeVisible();
    expect(mainPaths.getByText("代理启动")).toBeVisible();
    expect(mainPaths.getByText("检查更新 / 更新")).toBeVisible();
    expect(mainPaths.getByText("卸载")).toBeVisible();
    expect(mainPaths.getByText("自更新")).toBeVisible();
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
      onInstallEvent: () => () => {},
      getProxyLaunchStatus: async () => ({
        canLaunch: true,
        codexExe: "C:\\Users\\tester\\AppData\\Local\\Codex\\versions\\1.2.0\\Codex.exe",
        message: "可以启动 Codex"
      }),
      launchCodex: async () => ({ launched: true, message: "已启动 Codex" }),
      ...updateBridgeDefaults,
      ...uninstallBridgeDefaults
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
      onInstallEvent: () => () => {},
      getProxyLaunchStatus: async () => ({
        canLaunch: true,
        codexExe: "C:\\Users\\tester\\AppData\\Local\\Codex\\versions\\1.2.0\\Codex.exe",
        message: "可以启动 Codex"
      }),
      launchCodex: async () => ({ launched: true, message: "已启动 Codex" }),
      ...updateBridgeDefaults,
      ...uninstallBridgeDefaults
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
      },
      getProxyLaunchStatus: async () => ({
        canLaunch: true,
        codexExe: "C:\\Users\\tester\\AppData\\Local\\Codex\\versions\\1.2.0\\Codex.exe",
        message: "可以启动 Codex"
      }),
      launchCodex: async () => ({ launched: true, message: "已启动 Codex" }),
      ...updateBridgeDefaults,
      ...uninstallBridgeDefaults
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

  test("renders and starts the proxy launch path", async () => {
    let launched = false;
    const bridge: AppBridge = {
      getAppStatus: async () => ({
        productName: "Codex Windows 中文助手",
        v1Boundary: "中文安装更新助手",
        mainPaths: ["install", "proxyLaunch", "checkAndUpdate", "uninstall", "launcherSelfUpdate"]
      }),
      getInstallerDefaults: async () => installerDefaults,
      startInstall: async () => ({ accepted: true }),
      onInstallEvent: () => () => {},
      getProxyLaunchStatus: async () => ({
        canLaunch: true,
        codexExe: "C:\\Users\\tester\\AppData\\Local\\Codex\\versions\\1.2.0\\Codex.exe",
        message: "可以启动 Codex"
      }),
      launchCodex: async () => {
        launched = true;
        return { launched: true, message: "已启动 Codex" };
      },
      ...updateBridgeDefaults,
      ...uninstallBridgeDefaults
    };

    render(<App bridge={bridge} />);

    expect(await screen.findByRole("heading", { name: "启动 Codex" })).toBeVisible();
    expect(screen.getByText("可以启动 Codex")).toBeVisible();
    expect(screen.getByText(/versions\\1.2.0\\Codex.exe/)).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "启动" }));

    expect(await screen.findByText("已启动 Codex")).toBeVisible();
    expect(launched).toBe(true);
  });

  test("renders and starts the update path", async () => {
    let startedUpdate = false;
    let emitUpdateEvent:
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
      onInstallEvent: () => () => {},
      getProxyLaunchStatus: async () => ({
        canLaunch: true,
        codexExe: "C:\\Users\\tester\\AppData\\Local\\Codex\\versions\\1.2.0\\Codex.exe",
        message: "可以启动 Codex"
      }),
      launchCodex: async () => ({ launched: true, message: "已启动 Codex" }),
      checkUpdateStatus: async () => ({
        kind: "available",
        title: "发现 Codex 新版本",
        message: "当前版本 1.0.0，可更新到 1.2.0",
        currentVersion: "1.0.0",
        latestVersion: "1.2.0",
        actions: ["updateNow", "notNow", "skipThisVersion", "snoozeOneDay", "snoozeSevenDays", "never"]
      }),
      startUpdate: async () => {
        startedUpdate = true;
        return { accepted: true };
      },
      applyUpdateAction: async () => ({ applied: true, message: "已保存更新提醒设置" }),
      onUpdateEvent: (handler) => {
        emitUpdateEvent = handler;
        return () => {};
      },
      ...uninstallBridgeDefaults
    };

    render(<App bridge={bridge} />);

    expect(await screen.findByRole("heading", { name: "检查更新" })).toBeVisible();
    expect(screen.getByText("当前版本 1.0.0，可更新到 1.2.0")).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "立即更新" }));

    expect(await screen.findByText("正在更新")).toBeVisible();
    expect(startedUpdate).toBe(true);

    act(() => {
      emitUpdateEvent?.({
        kind: "done",
        title: "更新完成",
        detail: "已更新到 Codex 1.2.0",
        progress: 1,
        version: "1.2.0",
        message: null
      });
    });

    expect(await screen.findByText("更新完成")).toBeVisible();
    expect(screen.getByText("已更新到 Codex 1.2.0")).toBeVisible();
  });

  test("renders and starts the uninstall path", async () => {
    let startedUninstall = false;
    let emitUninstallEvent:
      | ((event: {
          kind: "phase" | "progress" | "done" | "error";
          title: string;
          detail: string;
          progress: number | null;
          logPath: string | null;
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
      onInstallEvent: () => () => {},
      getProxyLaunchStatus: async () => ({
        canLaunch: true,
        codexExe: "C:\\Users\\tester\\AppData\\Local\\Codex\\versions\\1.2.0\\Codex.exe",
        message: "可以启动 Codex"
      }),
      launchCodex: async () => ({ launched: true, message: "已启动 Codex" }),
      ...updateBridgeDefaults,
      getUninstallConfirmation: uninstallBridgeDefaults.getUninstallConfirmation,
      getUninstallStatus: uninstallBridgeDefaults.getUninstallStatus,
      startUninstall: async () => {
        startedUninstall = true;
        return { accepted: true };
      },
      onUninstallEvent: (handler) => {
        emitUninstallEvent = handler;
        return () => {};
      }
    };

    render(<App bridge={bridge} />);

    expect(await screen.findByRole("heading", { name: "卸载" })).toBeVisible();
    expect(screen.getByText("将只删除启动器管理的文件")).toBeVisible();
    expect(screen.getByText("已安装的 Codex 版本")).toBeVisible();
    expect(screen.getByText("Codex 登录数据")).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "确认卸载" }));

    expect(await screen.findByText("正在卸载")).toBeVisible();
    expect(startedUninstall).toBe(true);

    act(() => {
      emitUninstallEvent?.({
        kind: "done",
        title: "卸载完成",
        detail: "卸载日志：C:\\Temp\\codex-uninstall.log",
        progress: 1,
        logPath: "C:\\Temp\\codex-uninstall.log",
        message: null
      });
    });

    expect(await screen.findByText("卸载完成")).toBeVisible();
    expect(screen.getByText("卸载日志：C:\\Temp\\codex-uninstall.log")).toBeVisible();
  });
});
