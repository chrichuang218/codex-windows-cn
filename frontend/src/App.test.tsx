import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, test } from "vitest";
import { App } from "./App";
import { LoadingSplash } from "./components/Shell";
import type {
  AppBridge,
  AppStatus,
  InstallEvent,
  InstallerDefaults,
  LaunchInstalledRequest,
  LaunchRequest,
  LauncherUpdateEvent,
  ProxyLaunchResult,
  UninstallEvent,
  UpdateEvent,
  VersionInventory
} from "./bridge";

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
      keepVersions: 5,
      keepAllVersions: false,
      useCurrentJunction: true
    },
    {
      mode: "user",
      label: "当前用户",
      defaultRoot: "C:\\Users\\tester\\AppData\\Local\\Codex",
      createShortcut: true,
      registerUninstall: true,
      keepVersions: 5,
      keepAllVersions: false,
      useCurrentJunction: true
    },
    {
      mode: "system",
      label: "所有用户",
      defaultRoot: "C:\\Program Files\\Codex",
      createShortcut: true,
      registerUninstall: true,
      keepVersions: 5,
      keepAllVersions: false,
      useCurrentJunction: true
    }
  ],
  fetchers: ["direct", "winget", "localFile"]
};

const appStatus: AppStatus = {
  productName: "Codex Windows 中文助手",
  v1Boundary: "中文安装更新助手",
  mainPaths: ["install", "proxyLaunch", "checkAndUpdate", "uninstall", "launcherSelfUpdate"]
};

const updateStatus = {
  kind: "available" as const,
  title: "发现 ChatGPT 新版本",
  message: "当前版本 1.0.0，可更新到 1.2.0",
  currentVersion: "1.0.0",
  latestVersion: "1.2.0",
  productName: "ChatGPT",
  actions: [
    "updateNow" as const,
    "notNow" as const,
    "skipThisVersion" as const,
    "snoozeOneDay" as const,
    "snoozeSevenDays" as const,
    "never" as const
  ]
};

const launchedResult: ProxyLaunchResult = {
  launched: true,
  switchRequired: false,
  version: "1.2.0",
  productName: "ChatGPT",
  runningVersions: [],
  message: "已启动 ChatGPT 1.2.0"
};

const versionInventory: VersionInventory = {
  productName: "ChatGPT",
  root: "C:\\Users\\tester\\AppData\\Local\\Codex",
  defaultVersion: "1.2.0",
  runningVersions: [],
  keepVersions: 5,
  keepAllVersions: false,
  fetcher: "direct",
  useCurrentJunction: true,
  versions: [
    {
      version: "1.2.0",
      appKind: "chatGpt",
      productName: "ChatGPT",
      executable: "C:\\Users\\tester\\AppData\\Local\\Codex\\versions\\1.2.0\\ChatGPT.exe",
      sizeBytes: 1_820_000_000,
      installedAtUnix: 1_783_650_000,
      isDefault: true,
      isRunning: false,
      canDelete: true
    },
    {
      version: "1.0.0",
      appKind: "codex",
      productName: "Codex",
      executable: "C:\\Users\\tester\\AppData\\Local\\Codex\\versions\\1.0.0\\Codex.exe",
      sizeBytes: 1_640_000_000,
      installedAtUnix: 1_783_000_000,
      isDefault: false,
      isRunning: false,
      canDelete: true
    }
  ]
};

const uninstallConfirmation = {
  title: "确认卸载 Codex Windows 中文助手",
  root: "C:\\Users\\tester\\AppData\\Local\\Codex",
  deleteItems: ["已安装的桌面应用版本", "下载缓存", "启动器配置"],
  preserveItems: ["Codex/ChatGPT 登录数据", "日志和诊断信息"]
};

const uninstallStatus = {
  kind: "ready" as const,
  title: "可以卸载",
  message: "将只删除启动器管理的文件"
};

const launcherUpdateStatus = {
  kind: "available" as const,
  title: "发现启动器新版本",
  message: "当前版本 0.1.2，可更新到 0.2.0",
  currentVersion: "0.1.2",
  latestVersion: "0.2.0",
  releaseUrl: "https://github.com/chrichuang218/codex-windows-cn/releases/tag/v0.2.0",
  actions: [
    "updateNow" as const,
    "viewRelease" as const,
    "notNow" as const,
    "skipThisVersion" as const,
    "snoozeOneDay" as const,
    "snoozeSevenDays" as const,
    "never" as const
  ]
};

afterEach(() => {
  cleanup();
});

describe("Codex Windows 中文助手 shell", () => {
  test("first launch presents a single-window Chinese assistant shell", async () => {
    render(<App bridge={makeBridge({ installed: false })} />);

    expect(await screen.findByRole("heading", { name: "欢迎使用 Codex Windows 中文助手" })).toBeVisible();
    expect(screen.getByRole("heading", { name: "Codex Windows 中文助手" })).toBeVisible();
    expect(screen.queryByRole("heading", { name: "Codex Updater" })).toBeNull();
    expect(screen.getByLabelText("当前页面")).toHaveTextContent("欢迎");
    expect(screen.queryByText("安装 · 更新 · 启动")).toBeNull();
    expect(screen.queryByText("五条主路径")).toBeNull();
  });

  test("shows the React loading progress while local app state is pending", async () => {
    let resolveStatus: ((status: AppStatus) => void) | null = null;
    const bridge = makeBridge({
      installed: false,
      getAppStatus: () =>
        new Promise((resolve) => {
          resolveStatus = resolve;
        })
    });

    render(<App bridge={bridge} />);

    expect(
      await screen.findByRole("heading", { name: "正在启动 Codex Windows 中文助手" })
    ).toBeVisible();
    expect(
      screen.getByRole("progressbar", { name: "正在启动 Codex Windows 中文助手" })
    ).toBeVisible();

    act(() => {
      resolveStatus?.(appStatus);
    });

    expect(await screen.findByRole("heading", { name: "欢迎使用 Codex Windows 中文助手" })).toBeVisible();
  });

  test("React loading preserves the preboot presentation without a visual jump", () => {
    render(<LoadingSplash />);

    expect(screen.getByRole("heading", { name: "Codex Windows 中文助手" })).toBeVisible();
    expect(screen.getByText("安装向导")).toBeVisible();
    expect(screen.getByLabelText("当前页面")).toHaveTextContent("正在加载");
    expect(screen.getByText("C")).toBeVisible();
    expect(screen.getByRole("heading", { name: "正在启动 Codex Windows 中文助手" })).toBeVisible();
    expect(screen.getByText("正在读取安装、版本和更新状态。")).toBeVisible();
    expect(screen.getByText("官方 Microsoft Store 分发")).toBeVisible();
  });

  test("first launch is a focused installer window, not a sidebar workspace", async () => {
    render(<App bridge={makeBridge({ installed: false })} />);

    expect(await screen.findByRole("heading", { name: "欢迎使用 Codex Windows 中文助手" })).toBeVisible();
    expect(screen.getByText("中文安装更新助手")).toBeVisible();
    expect(screen.queryByLabelText("产品信息")).toBeNull();
    expect(screen.queryByRole("heading", { name: "Codex 已就绪" })).toBeNull();
    expect(screen.queryByText("确认卸载 Codex Windows 中文助手")).toBeNull();
    expect(screen.queryByText("发现启动器新版本")).toBeNull();
  });

  test("first launch offers one clear installation entry point", async () => {
    render(<App bridge={makeBridge({ installed: false })} />);

    expect(await screen.findByRole("heading", { name: "欢迎使用 Codex Windows 中文助手" })).toBeVisible();

    expect(screen.getByRole("button", { name: "开始安装" })).toBeEnabled();
    expect(screen.getByText("官方 Microsoft Store 分发")).toBeVisible();
    expect(screen.queryByRole("button", { name: "更新" })).toBeNull();
    expect(screen.queryByRole("button", { name: "启动" })).toBeNull();
  });

  test("walks the installer screens and starts installation with selected options", async () => {
    let submittedRoot = "";
    const bridge = makeBridge({
      installed: false,
      startInstall: async (request) => {
        submittedRoot = request.root;
        return { accepted: true };
      }
    });

    render(<App bridge={bridge} />);

    fireEvent.click(await screen.findByRole("button", { name: "开始安装" }));
    expect(await screen.findByRole("heading", { name: "选择安装范围" })).toBeVisible();
    expect(screen.getByLabelText("当前页面")).toHaveTextContent("步骤 1 / 3");
    expect(screen.getByRole("button", { name: /当前用户/ })).toHaveAttribute(
      "aria-pressed",
      "true"
    );

    fireEvent.click(screen.getByRole("button", { name: /便携模式/ }));
    fireEvent.click(screen.getByRole("button", { name: "下一步" }));
    expect(await screen.findByRole("heading", { name: "安装位置" })).toBeVisible();
    expect(screen.getByDisplayValue("D:\\Tools\\CodexPortable")).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "下一步" }));
    expect(await screen.findByRole("heading", { name: "安装选项" })).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "安装" }));

    expect(await screen.findByRole("heading", { name: "正在连接 Microsoft Store" })).toBeVisible();
    expect(
      screen.getByText("正在解析官方桌面应用下载地址，首次安装可能需要几分钟。")
    ).toBeVisible();
    expect(submittedRoot).toBe("D:\\Tools\\CodexPortable");
  });

  test("shows installation progress and completion as exclusive wizard states", async () => {
    let emitInstallEvent: ((event: InstallEvent) => void) | null = null;
    const bridge = makeBridge({
      installed: false,
      onInstallEvent: (handler) => {
        emitInstallEvent = handler;
        return () => {};
      }
    });

    render(<App bridge={bridge} />);
    fireEvent.click(await screen.findByRole("button", { name: "开始安装" }));
    fireEvent.click(await screen.findByRole("button", { name: "下一步" }));
    fireEvent.click(await screen.findByRole("button", { name: "下一步" }));
    fireEvent.click(await screen.findByRole("button", { name: "安装" }));

    act(() => {
      emitInstallEvent?.({
        kind: "phase",
        title: "正在下载",
        detail: "通过直连 Microsoft Store",
        progress: null,
        version: null,
        message: null
      });
      emitInstallEvent?.({
        kind: "progress",
        title: "安装进度",
        detail: "",
        progress: 0.35,
        version: null,
        message: null
      });
    });

    expect(await screen.findByRole("heading", { name: "正在下载" })).toBeVisible();
    expect(screen.getByText("通过直连 Microsoft Store")).toBeVisible();
    expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "35");

    act(() => {
      emitInstallEvent?.({
        kind: "done",
        title: "安装完成",
        detail: "",
        progress: 1,
        version: "1.2.0",
        message: null
      });
    });

    expect(await screen.findByText("ChatGPT 已安装")).toBeVisible();
    expect(screen.getByText("版本 1.2.0")).toBeVisible();
    expect(screen.queryByRole("heading", { name: "安装进度" })).toBeNull();
  });

  test("launches from the just-installed root after installation completes", async () => {
    let launchedFrom: LaunchInstalledRequest | null = null;
    let emitInstallEvent: ((event: InstallEvent) => void) | null = null;
    const bridge = makeBridge({
      installed: false,
      launchInstalledCodex: async (request) => {
        launchedFrom = request;
        return launchedResult;
      },
      launchCodex: async () => {
        throw new Error("should launch from the installed root");
      },
      onInstallEvent: (handler) => {
        emitInstallEvent = handler;
        return () => {};
      }
    });

    render(<App bridge={bridge} />);
    fireEvent.click(await screen.findByRole("button", { name: "开始安装" }));
    fireEvent.click(await screen.findByRole("button", { name: /便携模式/ }));
    fireEvent.click(screen.getByRole("button", { name: "下一步" }));
    expect(await screen.findByRole("heading", { name: "安装位置" })).toBeVisible();
    fireEvent.change(screen.getByDisplayValue("D:\\Tools\\CodexPortable"), {
      target: { value: "D:\\Apps\\CodexPortable" }
    });
    fireEvent.click(screen.getByRole("button", { name: "下一步" }));
    fireEvent.click(await screen.findByRole("button", { name: "安装" }));

    act(() => {
      emitInstallEvent?.({
        kind: "done",
        title: "安装完成",
        detail: "",
        progress: 1,
        version: "1.2.0",
        message: null
      });
    });

    fireEvent.click(await screen.findByRole("button", { name: "启动 ChatGPT" }));

    expect(await screen.findByText("已启动 ChatGPT 1.2.0")).toBeVisible();
    expect(launchedFrom).toEqual({
      root: "D:\\Apps\\CodexPortable",
      useCurrentJunction: true
    });
  });

  test("polls install status when install events are not delivered", async () => {
    const bridge = makeBridge({
      installed: false,
      getInstallStatus: async () => ({
        kind: "phase",
        title: "正在下载",
        detail: "通过直连 Microsoft Store",
        progress: null,
        version: null,
        message: null
      }),
      onInstallEvent: () => () => {}
    });

    render(<App bridge={bridge} />);
    fireEvent.click(await screen.findByRole("button", { name: "开始安装" }));
    fireEvent.click(await screen.findByRole("button", { name: "下一步" }));
    fireEvent.click(await screen.findByRole("button", { name: "下一步" }));
    fireEvent.click(await screen.findByRole("button", { name: "安装" }));

    expect(await screen.findByRole("heading", { name: "正在下载" })).toBeVisible();
    expect(screen.getByText("通过直连 Microsoft Store")).toBeVisible();
  });

  test("polled download progress shows size detail instead of stale store connection text", async () => {
    const bridge = makeBridge({
      installed: false,
      getInstallStatus: async () => ({
        kind: "progress",
        title: "正在下载",
        detail: "538 / 639 MB",
        progress: 0.842,
        version: null,
        message: null
      }),
      onInstallEvent: () => () => {}
    });

    render(<App bridge={bridge} />);
    fireEvent.click(await screen.findByRole("button", { name: "开始安装" }));
    fireEvent.click(await screen.findByRole("button", { name: "下一步" }));
    fireEvent.click(await screen.findByRole("button", { name: "下一步" }));
    fireEvent.click(await screen.findByRole("button", { name: "安装" }));

    expect(await screen.findByRole("heading", { name: "正在下载" })).toBeVisible();
    expect(screen.getByText("538 / 639 MB")).toBeVisible();
    expect(screen.getByText("84%")).toBeVisible();
    expect(screen.queryByRole("heading", { name: "正在连接 Microsoft Store" })).toBeNull();
  });

  test("polled extract progress shows file count detail", async () => {
    const bridge = makeBridge({
      installed: false,
      getInstallStatus: async () => ({
        kind: "progress",
        title: "正在解压",
        detail: "318 / 900 files",
        progress: 0.353,
        version: null,
        message: null
      }),
      onInstallEvent: () => () => {}
    });

    render(<App bridge={bridge} />);
    fireEvent.click(await screen.findByRole("button", { name: "开始安装" }));
    fireEvent.click(await screen.findByRole("button", { name: "下一步" }));
    fireEvent.click(await screen.findByRole("button", { name: "下一步" }));
    fireEvent.click(await screen.findByRole("button", { name: "安装" }));

    expect(await screen.findByRole("heading", { name: "正在解压" })).toBeVisible();
    expect(screen.getByText("318 / 900 files")).toBeVisible();
    expect(screen.getByText("35%")).toBeVisible();
  });

  test("legacy blank progress does not keep the store connection title after bytes start", async () => {
    const bridge = makeBridge({
      installed: false,
      getInstallStatus: async () => ({
        kind: "progress",
        title: "安装进度",
        detail: "",
        progress: 0.23,
        version: null,
        message: null
      }),
      onInstallEvent: () => () => {}
    });

    render(<App bridge={bridge} />);
    fireEvent.click(await screen.findByRole("button", { name: "开始安装" }));
    fireEvent.click(await screen.findByRole("button", { name: "下一步" }));
    fireEvent.click(await screen.findByRole("button", { name: "下一步" }));
    fireEvent.click(await screen.findByRole("button", { name: "安装" }));

    expect(await screen.findByRole("heading", { name: "正在下载" })).toBeVisible();
    expect(screen.getByText("正在下载官方桌面应用安装包。")).toBeVisible();
    expect(screen.getByText("23%")).toBeVisible();
  });

  test("cancels a running installation from the progress screen", async () => {
    let cancelled = false;
    let emitInstallEvent: ((event: InstallEvent) => void) | null = null;
    const bridge = makeBridge({
      installed: false,
      cancelInstall: async () => {
        cancelled = true;
        return { accepted: true };
      },
      onInstallEvent: (handler) => {
        emitInstallEvent = handler;
        return () => {};
      }
    });

    render(<App bridge={bridge} />);
    fireEvent.click(await screen.findByRole("button", { name: "开始安装" }));
    fireEvent.click(await screen.findByRole("button", { name: "下一步" }));
    fireEvent.click(await screen.findByRole("button", { name: "下一步" }));
    fireEvent.click(await screen.findByRole("button", { name: "安装" }));

    fireEvent.click(await screen.findByRole("button", { name: "取消安装" }));

    expect(cancelled).toBe(true);
    expect(await screen.findByRole("button", { name: "取消中" })).toBeDisabled();

    act(() => {
      emitInstallEvent?.({
        kind: "error",
        title: "安装已取消",
        detail: "",
        progress: null,
        version: null,
        message: "安装已取消"
      });
    });

    expect(await screen.findByText("安装已取消")).toBeVisible();
  });

  test("installed mode opens the ChatGPT overview and launches the default version", async () => {
    const launchRequests: (LaunchRequest | undefined)[] = [];
    const bridge = makeBridge({
      installed: true,
      launchCodex: async (request) => {
        launchRequests.push(request);
        return launchedResult;
      }
    });

    render(<App bridge={bridge} />);

    expect(await screen.findByRole("button", { name: "概览" })).toBeVisible();
    expect(screen.getByRole("heading", { name: "Codex Windows 中文助手" })).toBeVisible();
    expect(screen.getByRole("button", { name: "概览" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("button", { name: "版本" })).toBeVisible();
    expect(screen.getByRole("button", { name: "设置" })).toBeVisible();
    expect(screen.getByText("发现 ChatGPT 新版本")).toBeVisible();
    expect(screen.getByText("当前版本 1.0.0，可更新到 1.2.0")).toBeVisible();
    expect(screen.queryByRole("button", { name: /卸载助手/ })).toBeNull();
    expect(screen.getByRole("button", { name: "立即更新" })).toBeVisible();
    expect(screen.getByRole("button", { name: "稍后提醒" })).toBeVisible();
    expect(screen.getByRole("button", { name: "跳过此版本" })).toBeVisible();
    expect(screen.getByRole("button", { name: "1 天后提醒" })).toBeVisible();
    expect(screen.getByRole("button", { name: "7 天后提醒" })).toBeVisible();
    expect(screen.getByRole("button", { name: "关闭提醒" })).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "启动 ChatGPT" }));

    expect(await screen.findByText("已启动 ChatGPT 1.2.0")).toBeVisible();
    expect(launchRequests).toEqual([{ version: null, switchRunning: false }]);
  });

  test("version inventory launches a retained historical version without changing the default target", async () => {
    const launchRequests: LaunchRequest[] = [];
    const bridge = makeBridge({
      installed: true,
      launchCodex: async (request) => {
        if (request) {
          launchRequests.push(request);
        }
        return {
          launched: true,
          switchRequired: false,
          version: request?.version ?? "1.2.0",
          productName: request?.version === "1.0.0" ? "Codex" : "ChatGPT",
          runningVersions: [],
          message: request?.version === "1.0.0" ? "已启动 Codex 1.0.0" : "已启动 ChatGPT 1.2.0"
        };
      }
    });

    render(<App bridge={bridge} />);
    fireEvent.click(await screen.findByRole("button", { name: "版本" }));
    fireEvent.click(await screen.findByRole("button", { name: "启动 Codex 1.0.0" }));

    expect(await screen.findByText("已启动 Codex 1.0.0")).toBeVisible();
    expect(launchRequests).toEqual([{ version: "1.0.0", switchRunning: false }]);
    expect(versionInventory.defaultVersion).toBe("1.2.0");
  });

  test("running version status refreshes after the managed main process exits", async () => {
    let running = true;
    const bridge = makeBridge({
      installed: true,
      getVersionInventory: async () => ({
        ...versionInventory,
        runningVersions: running ? ["1.2.0"] : [],
        versions: versionInventory.versions.map((item) =>
          item.version === "1.2.0"
            ? { ...item, isRunning: running, canDelete: !running }
            : item
        )
      })
    });

    render(<App bridge={bridge} />);
    fireEvent.click(await screen.findByRole("button", { name: "版本" }));
    expect(await screen.findByText("运行中")).toBeVisible();

    running = false;
    await waitFor(() => expect(screen.queryByText("运行中")).toBeNull(), { timeout: 2500 });
  });

  test("switching versions requires confirmation before closing the running version", async () => {
    const launchRequests: LaunchRequest[] = [];
    const bridge = makeBridge({
      installed: true,
      launchCodex: async (request) => {
        if (request) {
          launchRequests.push(request);
        }
        if (!request?.switchRunning) {
          return {
            launched: false,
            switchRequired: true,
            version: "1.0.0",
            productName: "Codex",
            runningVersions: ["1.2.0"],
            message: "需要先关闭正在运行的版本"
          };
        }
        return {
          launched: true,
          switchRequired: false,
          version: "1.0.0",
          productName: "Codex",
          runningVersions: [],
          message: "已切换到 Codex 1.0.0"
        };
      }
    });

    render(<App bridge={bridge} />);
    fireEvent.click(await screen.findByRole("button", { name: "版本" }));
    fireEvent.click(await screen.findByRole("button", { name: "启动 Codex 1.0.0" }));

    expect(await screen.findByRole("heading", { name: "切换运行版本" })).toBeVisible();
    expect(screen.getByText(/当前正在运行 1.2.0/)).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "关闭并切换" }));

    expect(await screen.findByText("已切换到 Codex 1.0.0")).toBeVisible();
    expect(launchRequests).toEqual([
      { version: "1.0.0", switchRunning: false },
      { version: "1.0.0", switchRunning: true }
    ]);
  });

  test("version deletion uses an in-app confirmation and protects running versions", async () => {
    const inventory: VersionInventory = {
      ...versionInventory,
      runningVersions: ["1.0.0"],
      versions: versionInventory.versions.map((item) =>
        item.version === "1.0.0"
          ? { ...item, isRunning: true, canDelete: false }
          : item
      )
    };
    const deleted: string[] = [];
    const bridge = makeBridge({
      installed: true,
      getVersionInventory: async () => inventory,
      deleteInstalledVersion: async (version) => {
        deleted.push(version);
        return {
          applied: true,
          message: `已删除版本 ${version}`,
          inventory: {
            ...inventory,
            defaultVersion: "1.0.0",
            versions: [
              { ...inventory.versions[1], isDefault: true, isRunning: true, canDelete: false }
            ]
          }
        };
      }
    });

    render(<App bridge={bridge} />);
    fireEvent.click(await screen.findByRole("button", { name: "版本" }));

    expect(await screen.findByRole("button", { name: "删除 Codex 1.0.0" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "删除 ChatGPT 1.2.0" }));
    expect(await screen.findByRole("heading", { name: "确认删除" })).toBeVisible();
    expect(screen.getByText(/登录和项目数据会保留/)).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "删除版本" }));

    expect(await screen.findByText("已删除版本 1.2.0")).toBeVisible();
    expect(deleted).toEqual(["1.2.0"]);
  });

  test("settings persist both a recent-version count and keep-all mode", async () => {
    const requests: Array<{ keepVersions: number; keepAllVersions: boolean }> = [];
    const bridge = makeBridge({
      installed: true,
      saveRetentionSettings: async (request) => {
        requests.push(request);
        return {
          applied: true,
          message: request.keepAllVersions ? "已设置为全部保留" : "已保存版本保留设置",
          inventory: { ...versionInventory, ...request }
        };
      }
    });

    render(<App bridge={bridge} />);
    fireEvent.click(await screen.findByRole("button", { name: "设置" }));
    expect(await screen.findByRole("heading", { name: "保留与维护" })).toBeVisible();

    fireEvent.change(screen.getByRole("spinbutton", { name: "自动保留版本数量" }), {
      target: { value: "7" }
    });
    fireEvent.click(screen.getByRole("button", { name: "保存设置" }));
    expect(await screen.findByText("已保存版本保留设置")).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "全部保留" }));
    fireEvent.click(screen.getByRole("button", { name: "保存设置" }));
    expect(await screen.findByText("已设置为全部保留")).toBeVisible();
    expect(requests).toEqual([
      { keepVersions: 7, keepAllVersions: false },
      { keepVersions: 7, keepAllVersions: true }
    ]);
  });

  test("installed mode keeps the loading progress visible while update checks are pending", async () => {
    render(
      <App
        bridge={makeBridge({
          installed: true,
          checkUpdateStatus: () => new Promise(() => {}),
          checkLauncherUpdateStatus: () => new Promise(() => {})
        })}
      />
    );

    expect(
      await screen.findByRole("heading", { name: "正在启动 Codex Windows 中文助手" })
    ).toBeVisible();
    expect(
      screen.getByRole("progressbar", { name: "正在启动 Codex Windows 中文助手" })
    ).toBeVisible();
    expect(screen.queryByText("发现 ChatGPT 新版本")).toBeNull();
    expect(screen.queryByRole("button", { name: "启动 ChatGPT" })).toBeNull();
  });

  test("installed mode goes from loading progress to final up-to-date screen", async () => {
    render(
      <App
        bridge={makeBridge({
          installed: true,
          getProxyLaunchStatus: async () => ({
            managedInstall: true,
            currentVersion: "1.2.0",
            knownLatest: "1.2.0",
            canLaunch: true,
            codexExe: "C:\\Users\\tester\\AppData\\Local\\Codex\\versions\\1.2.0\\Codex.exe",
            productName: "ChatGPT",
            runningVersions: [],
            message: "可以启动 Codex"
          }),
          checkUpdateStatus: async () => ({
            kind: "upToDate",
            title: "ChatGPT 已是最新版本",
            message: "当前版本 1.2.0",
            currentVersion: "1.2.0",
            latestVersion: "1.2.0",
            productName: "ChatGPT",
            actions: []
          }),
          checkLauncherUpdateStatus: () => new Promise(() => {})
        })}
      />
    );

    expect((await screen.findAllByText("ChatGPT 已是最新版本")).length).toBe(2);
    expect(screen.getByText("已同步")).toBeVisible();
    expect(screen.queryByRole("button", { name: "立即更新" })).toBeNull();
    expect(screen.getByRole("button", { name: "启动 ChatGPT" })).toBeVisible();
  });

  test("installed mode keeps the home screen visible when update checks fail", async () => {
    render(
      <App
        bridge={makeBridge({
          installed: true,
          checkUpdateStatus: async () => {
            throw new Error("Store timeout");
          },
          checkLauncherUpdateStatus: async () => {
            throw new Error("GitHub timeout");
          }
        })}
      />
    );

    expect((await screen.findAllByText("检查更新失败")).length).toBe(2);
    expect(screen.getByText("Store timeout")).toBeVisible();
    expect(screen.getByRole("button", { name: "启动 ChatGPT" })).toBeVisible();
  });

  test("managed install without launchable exe still opens the version screen", async () => {
    const emptyInventory: VersionInventory = {
      ...versionInventory,
      defaultVersion: null,
      versions: []
    };
    const bridge = makeBridge({
      installed: true,
      getProxyLaunchStatus: async () => ({
        managedInstall: true,
        currentVersion: "1.0.0",
        knownLatest: "1.2.0",
        canLaunch: false,
        codexExe: null,
        productName: "ChatGPT",
        runningVersions: [],
        message: "未找到可启动应用"
      }),
      getVersionInventory: async () => emptyInventory
    });

    render(<App bridge={bridge} />);

    expect(await screen.findByRole("button", { name: "概览" })).toBeVisible();
    expect(screen.getByRole("heading", { name: "Codex Windows 中文助手" })).toBeVisible();
    expect(screen.queryByRole("heading", { name: "欢迎使用 Codex Windows 中文助手" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "版本" }));
    expect(await screen.findByText("未找到可启动版本")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "概览" }));
    expect(screen.getByRole("button", { name: "启动 ChatGPT" })).toBeDisabled();
  });

  test("installed mode shows an up-to-date parity screen when no Codex update exists", async () => {
    render(
      <App
        bridge={makeBridge({
          installed: true,
          checkUpdateStatus: async () => ({
            kind: "upToDate",
            title: "ChatGPT 已是最新",
            message: "当前版本 1.2.3",
            currentVersion: "1.2.3",
            latestVersion: "1.2.3",
            productName: "ChatGPT",
            actions: []
          })
        })}
      />
    );

    expect((await screen.findAllByText("ChatGPT 已是最新")).length).toBeGreaterThan(0);
    expect(screen.getByText("已同步")).toBeVisible();
    expect(screen.queryByRole("button", { name: "立即更新" })).toBeNull();
  });

  test("starts update from installed mode and switches to an exclusive update progress screen", async () => {
    let startedUpdate = false;
    let emitUpdateEvent: ((event: UpdateEvent) => void) | null = null;
    let proxyStatusCalls = 0;
    const bridge = makeBridge({
      installed: true,
      getProxyLaunchStatus: async () => {
        proxyStatusCalls++;
        return proxyStatusCalls === 1
          ? {
              managedInstall: true,
              currentVersion: "1.0.0",
              knownLatest: "1.2.0",
              canLaunch: false,
              codexExe: null,
              productName: "ChatGPT",
              runningVersions: [],
              message: "未找到可启动应用"
            }
          : {
              managedInstall: true,
              currentVersion: "1.2.0",
              knownLatest: "1.2.0",
              canLaunch: true,
              codexExe: "C:\\Users\\tester\\AppData\\Local\\Codex\\versions\\1.2.0\\Codex.exe",
              productName: "ChatGPT",
              runningVersions: [],
              message: "可以启动 ChatGPT"
            };
      },
      startUpdate: async () => {
        startedUpdate = true;
        return { accepted: true };
      },
      onUpdateEvent: (handler) => {
        emitUpdateEvent = handler;
        return () => {};
      }
    });

    render(<App bridge={bridge} />);

    fireEvent.click(await screen.findByRole("button", { name: "立即更新" }));
    expect(startedUpdate).toBe(true);
    expect(await screen.findByRole("heading", { name: "正在下载更新" })).toBeVisible();
    expect(screen.getByText("正在使用已配置的下载方式获取 ChatGPT 更新包。")).toBeVisible();
    expect(screen.queryByText("发现 ChatGPT 新版本")).toBeNull();
    expect(screen.queryByRole("button", { name: "稍后提醒" })).toBeNull();

    act(() => {
      emitUpdateEvent?.({
        kind: "done",
        title: "更新完成",
        detail: "已更新到 ChatGPT 1.2.0",
        progress: 1,
        version: "1.2.0",
        message: null
      });
    });

    expect(await screen.findByText("已更新到 ChatGPT 1.2.0")).toBeVisible();
    expect(screen.queryByRole("button", { name: "立即更新" })).toBeNull();
    expect(await screen.findByRole("button", { name: "启动 ChatGPT" })).toBeEnabled();
  });

  test("polls update status when update events are not delivered", async () => {
    let polls = 0;
    const bridge = makeBridge({
      installed: true,
      onUpdateEvent: () => () => {},
      getUpdateStatus: async () => {
        polls++;
        return polls < 2
          ? null
          : {
              kind: "progress",
              title: "正在解压更新",
              detail: "318 / 900 files",
              progress: 0.353,
              version: null,
              message: null
            };
      }
    });

    render(<App bridge={bridge} />);

    fireEvent.click(await screen.findByRole("button", { name: "立即更新" }));

    expect(await screen.findByRole("heading", { name: "正在解压更新" })).toBeVisible();
    expect(screen.getByText("318 / 900 files")).toBeVisible();
    expect(screen.getByText("35%")).toBeVisible();
  });

  test("uninstall is available as a secondary screen, not a permanent panel", async () => {
    let startedUninstall = false;
    let emitUninstallEvent: ((event: UninstallEvent) => void) | null = null;
    const bridge = makeBridge({
      installed: true,
      startUninstall: async () => {
        startedUninstall = true;
        return { accepted: true };
      },
      onUninstallEvent: (handler) => {
        emitUninstallEvent = handler;
        return () => {};
      }
    });

    render(<App bridge={bridge} />);

    expect(await screen.findByRole("button", { name: "概览" })).toBeVisible();
    expect(screen.getByRole("heading", { name: "Codex Windows 中文助手" })).toBeVisible();
    expect(screen.queryByText("已安装的 Codex 版本")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "设置" }));
    fireEvent.click(await screen.findByRole("button", { name: /卸载助手/ }));

    expect(await screen.findByText("确认卸载 Codex Windows 中文助手")).toBeVisible();
    expect(screen.getByText("已安装的桌面应用版本")).toBeVisible();
    expect(screen.getByText("Codex/ChatGPT 登录数据")).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "确认卸载" }));
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

  test("polls uninstall progress when uninstall events are not delivered", async () => {
    let polls = 0;
    const bridge = makeBridge({
      installed: true,
      onUninstallEvent: () => () => {},
      getUninstallProgress: async () => {
        polls++;
        return polls < 2
          ? null
          : {
              kind: "done",
              title: "卸载完成",
              detail: "卸载日志：C:\\Temp\\codex-uninstall.log",
              progress: 1,
              logPath: "C:\\Temp\\codex-uninstall.log",
              message: null
            };
      }
    });

    render(<App bridge={bridge} />);

    fireEvent.click(await screen.findByRole("button", { name: "设置" }));
    fireEvent.click(await screen.findByRole("button", { name: /卸载助手/ }));
    fireEvent.click(await screen.findByRole("button", { name: "确认卸载" }));

    expect(await screen.findByText("卸载完成")).toBeVisible();
    expect(screen.getByText("卸载日志：C:\\Temp\\codex-uninstall.log")).toBeVisible();
  });

  test("launcher self-update is available as a secondary screen", async () => {
    let startedLauncherUpdate = false;
    let emitLauncherUpdateEvent: ((event: LauncherUpdateEvent) => void) | null = null;
    const bridge = makeBridge({
      installed: true,
      startLauncherUpdate: async () => {
        startedLauncherUpdate = true;
        return { accepted: true };
      },
      onLauncherUpdateEvent: (handler) => {
        emitLauncherUpdateEvent = handler;
        return () => {};
      }
    });

    render(<App bridge={bridge} />);

    expect(await screen.findByRole("button", { name: "概览" })).toBeVisible();
    expect(screen.getByRole("heading", { name: "Codex Windows 中文助手" })).toBeVisible();
    expect(screen.queryByText("发现启动器新版本")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "设置" }));
    fireEvent.click(await screen.findByRole("button", { name: /启动器更新/ }));

    expect(await screen.findByText("发现启动器新版本")).toBeVisible();
    expect(screen.getByRole("link", { name: "查看发布页" })).toHaveAttribute(
      "href",
      "https://github.com/chrichuang218/codex-windows-cn/releases/tag/v0.2.0"
    );

    fireEvent.click(screen.getByRole("button", { name: "应用更新" }));
    expect(startedLauncherUpdate).toBe(true);

    act(() => {
      emitLauncherUpdateEvent?.({
        kind: "done",
        title: "自更新完成",
        detail: "启动器已更新，重启后生效",
        progress: 1,
        message: null
      });
    });

    expect(await screen.findByText("自更新完成")).toBeVisible();
    expect(screen.getByText("启动器已更新，重启后生效")).toBeVisible();
  });

  test("polls launcher self-update progress when launcher events are not delivered", async () => {
    let polls = 0;
    const bridge = makeBridge({
      installed: true,
      onLauncherUpdateEvent: () => () => {},
      getLauncherUpdateProgress: async () => {
        polls++;
        return polls < 2
          ? null
          : {
              kind: "progress",
              title: "正在下载启动器",
              detail: "512 / 1024 KB",
              progress: 0.5,
              message: null
            };
      }
    });

    render(<App bridge={bridge} />);

    fireEvent.click(await screen.findByRole("button", { name: "设置" }));
    fireEvent.click(await screen.findByRole("button", { name: /启动器更新/ }));
    fireEvent.click(await screen.findByRole("button", { name: "应用更新" }));

    expect(await screen.findByRole("heading", { name: "正在下载启动器" })).toBeVisible();
    expect(screen.getByText("512 / 1024 KB")).toBeVisible();
    expect(screen.getByText("50%")).toBeVisible();
  });

  test("secondary maintenance screens return to the installed home without leaking content", async () => {
    render(<App bridge={makeBridge({ installed: true })} />);

    expect(await screen.findByRole("button", { name: "概览" })).toBeVisible();
    expect(screen.getByRole("heading", { name: "Codex Windows 中文助手" })).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "设置" }));
    expect(await screen.findByRole("heading", { name: "保留与维护" })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: /卸载助手/ }));
    expect(await screen.findByText("确认卸载 Codex Windows 中文助手")).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "返回设置" }));
    expect(await screen.findByRole("heading", { name: "保留与维护" })).toBeVisible();
    expect(screen.queryByText("确认卸载 Codex Windows 中文助手")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: /启动器更新/ }));
    expect(await screen.findByText("发现启动器新版本")).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "返回设置" }));
    expect(await screen.findByRole("heading", { name: "保留与维护" })).toBeVisible();
    expect(screen.queryByText("发现启动器新版本")).toBeNull();
  });
});

function makeBridge(
  overrides: Partial<AppBridge> & { installed: boolean }
): AppBridge {
  const bridge: AppBridge = {
    getAppStatus: async () => appStatus,
    getInstallerDefaults: async () => installerDefaults,
    startInstall: async () => ({ accepted: true }),
    cancelInstall: async () => ({ accepted: true }),
    getInstallStatus: async () => null,
    onInstallEvent: () => () => {},
    getProxyLaunchStatus: async () =>
      overrides.installed
        ? {
            managedInstall: true,
            currentVersion: "1.0.0",
            knownLatest: "1.2.0",
            canLaunch: true,
            codexExe: "C:\\Users\\tester\\AppData\\Local\\Codex\\versions\\1.2.0\\Codex.exe",
            productName: "ChatGPT",
            runningVersions: [],
            message: "可以启动 Codex"
          }
        : {
            managedInstall: false,
            currentVersion: null,
            knownLatest: null,
            canLaunch: false,
            codexExe: null,
            productName: "Codex",
            runningVersions: [],
            message: "尚未完成安装"
          },
    launchCodex: async () => launchedResult,
    launchInstalledCodex: async () => launchedResult,
    getVersionInventory: async () => versionInventory,
    deleteInstalledVersion: async (version) => ({
      applied: true,
      message: `已删除版本 ${version}`,
      inventory: versionInventory
    }),
    saveRetentionSettings: async (request) => ({
      applied: true,
      message: request.keepAllVersions ? "已设置为全部保留" : "已保存版本保留设置",
      inventory: { ...versionInventory, ...request }
    }),
    checkUpdateStatus: async () => updateStatus,
    startUpdate: async () => ({ accepted: true }),
    getUpdateStatus: async () => null,
    applyUpdateAction: async () => ({ applied: true, message: "已保存更新提醒设置" }),
    onUpdateEvent: () => () => {},
    getUninstallConfirmation: async () => uninstallConfirmation,
    getUninstallStatus: async () => uninstallStatus,
    startUninstall: async () => ({ accepted: true }),
    getUninstallProgress: async () => null,
    onUninstallEvent: () => () => {},
    checkLauncherUpdateStatus: async () => launcherUpdateStatus,
    startLauncherUpdate: async () => ({ accepted: true }),
    getLauncherUpdateProgress: async () => null,
    applyLauncherUpdateAction: async () => ({
      applied: true,
      message: "已保存自更新提醒设置"
    }),
    onLauncherUpdateEvent: () => () => {}
  };

  return { ...bridge, ...overrides };
}
