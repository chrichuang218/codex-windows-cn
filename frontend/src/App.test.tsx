import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, test } from "vitest";
import { App } from "./App";
import type {
  AppBridge,
  AppStatus,
  InstallEvent,
  InstallerDefaults,
  LaunchInstalledRequest,
  LauncherUpdateEvent,
  UninstallEvent,
  UpdateEvent
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

const appStatus: AppStatus = {
  productName: "Codex Windows 中文助手",
  v1Boundary: "中文安装更新助手",
  mainPaths: ["install", "proxyLaunch", "checkAndUpdate", "uninstall", "launcherSelfUpdate"]
};

const updateStatus = {
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
};

const uninstallConfirmation = {
  title: "确认卸载 Codex Windows 中文助手",
  root: "C:\\Users\\tester\\AppData\\Local\\Codex",
  deleteItems: ["已安装的 Codex 版本", "下载缓存", "启动器配置"],
  preserveItems: ["Codex 登录数据", "日志和诊断信息"]
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

    expect(await screen.findByRole("heading", { name: "欢迎使用 Codex 安装助手" })).toBeVisible();
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

    expect(await screen.findByRole("heading", { name: "正在检查更新" })).toBeVisible();
    expect(screen.getByRole("progressbar", { name: "正在检查更新" })).toBeVisible();

    act(() => {
      resolveStatus?.(appStatus);
    });

    expect(await screen.findByRole("heading", { name: "欢迎使用 Codex 安装助手" })).toBeVisible();
  });

  test("first launch is a focused installer window, not a sidebar workspace", async () => {
    render(<App bridge={makeBridge({ installed: false })} />);

    expect(await screen.findByRole("heading", { name: "欢迎使用 Codex 安装助手" })).toBeVisible();
    expect(screen.getByText("中文安装更新助手")).toBeVisible();
    expect(screen.queryByLabelText("产品信息")).toBeNull();
    expect(screen.queryByRole("heading", { name: "Codex 已就绪" })).toBeNull();
    expect(screen.queryByText("确认卸载 Codex Windows 中文助手")).toBeNull();
    expect(screen.queryByText("发现启动器新版本")).toBeNull();
  });

  test("first launch does not route unavailable update or launch actions into dead ends", async () => {
    render(<App bridge={makeBridge({ installed: false })} />);

    expect(await screen.findByRole("heading", { name: "欢迎使用 Codex 安装助手" })).toBeVisible();

    expect(screen.getByRole("button", { name: "安装" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "更新" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "启动" })).toBeDisabled();
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

    fireEvent.click(await screen.findByRole("button", { name: "下一步" }));
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
    expect(screen.getByText("正在解析 Codex 下载地址，首次安装可能需要几分钟。")).toBeVisible();
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
    fireEvent.click(await screen.findByRole("button", { name: "下一步" }));
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

    expect(await screen.findByText("Codex 已安装")).toBeVisible();
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
        return { launched: true, message: "已启动 Codex" };
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
    fireEvent.click(await screen.findByRole("button", { name: "下一步" }));
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

    fireEvent.click(await screen.findByRole("button", { name: "启动 Codex" }));

    expect(await screen.findByText("已启动 Codex")).toBeVisible();
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
    fireEvent.click(await screen.findByRole("button", { name: "下一步" }));
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
    fireEvent.click(await screen.findByRole("button", { name: "下一步" }));
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
    fireEvent.click(await screen.findByRole("button", { name: "下一步" }));
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
    fireEvent.click(await screen.findByRole("button", { name: "下一步" }));
    fireEvent.click(await screen.findByRole("button", { name: "下一步" }));
    fireEvent.click(await screen.findByRole("button", { name: "下一步" }));
    fireEvent.click(await screen.findByRole("button", { name: "安装" }));

    expect(await screen.findByRole("heading", { name: "正在下载" })).toBeVisible();
    expect(screen.getByText("正在下载 Codex 安装包。")).toBeVisible();
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
    fireEvent.click(await screen.findByRole("button", { name: "下一步" }));
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

  test("installed mode starts Codex and keeps update controls on the main screen", async () => {
    let launched = false;
    const bridge = makeBridge({
      installed: true,
      launchCodex: async () => {
        launched = true;
        return { launched: true, message: "已启动 Codex" };
      }
    });

    render(<App bridge={bridge} />);

    expect(await screen.findByRole("heading", { name: "发现 Codex 新版本" })).toBeVisible();
    expect(screen.getByRole("heading", { name: "Codex Windows 中文助手" })).toBeVisible();
    expect(screen.getByLabelText("当前页面")).toHaveTextContent("有可用更新");
    expect(screen.queryByRole("heading", { name: "Codex 工作台" })).toBeNull();
    expect(screen.queryByText(/versions\\1.2.0\\Codex.exe/)).toBeNull();
    expect(screen.getByText("发现 Codex 新版本")).toBeVisible();
    expect(screen.queryByText("当前版本 1.0.0，可更新到 1.2.0")).toBeNull();
    expect(screen.queryByText("已安装的 Codex 版本")).toBeNull();
    expect(screen.getByRole("button", { name: "卸载" })).toBeVisible();
    expect(screen.getByRole("button", { name: "启动器更新" })).toBeVisible();
    expect(screen.getByRole("button", { name: "启动 Codex" })).toBeVisible();
    expect(screen.getByRole("button", { name: "立即更新" })).toBeVisible();
    expect(screen.getByRole("button", { name: "稍后提醒" })).toBeVisible();
    expect(screen.getByRole("button", { name: "跳过此版本" })).toBeVisible();
    expect(screen.getByRole("button", { name: "1 天后提醒" })).toBeVisible();
    expect(screen.getByRole("button", { name: "7 天后提醒" })).toBeVisible();
    expect(screen.getByRole("button", { name: "关闭提醒" })).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "启动 Codex" }));

    expect(await screen.findByText("已启动 Codex")).toBeVisible();
    expect(launched).toBe(true);
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

    expect(await screen.findByRole("heading", { name: "正在检查更新" })).toBeVisible();
    expect(screen.getByRole("progressbar", { name: "正在检查更新" })).toBeVisible();
    expect(screen.queryByRole("heading", { name: "发现 Codex 新版本" })).toBeNull();
    expect(screen.queryByRole("heading", { name: "正在检查 Codex 更新" })).toBeNull();
    expect(screen.queryByRole("button", { name: "启动 Codex" })).toBeNull();
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
            message: "可以启动 Codex"
          }),
          checkUpdateStatus: async () => ({
            kind: "upToDate",
            title: "Codex 已是最新版本",
            message: "当前版本 1.2.0",
            currentVersion: "1.2.0",
            latestVersion: "1.2.0",
            actions: []
          }),
          checkLauncherUpdateStatus: () => new Promise(() => {})
        })}
      />
    );

    expect(await screen.findByRole("heading", { name: "Codex 已是最新版本" })).toBeVisible();
    expect(screen.getByLabelText("当前页面")).toHaveTextContent("已是最新");
    expect(screen.queryByRole("heading", { name: "正在检查 Codex 更新" })).toBeNull();
    expect(screen.queryByRole("button", { name: "立即更新" })).toBeNull();
    expect(screen.getByRole("button", { name: "启动 Codex" })).toBeVisible();
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

    expect(await screen.findByRole("heading", { name: "检查更新失败" })).toBeVisible();
    expect(screen.getByText("Store timeout")).toBeVisible();
    expect(screen.getByRole("button", { name: "启动 Codex" })).toBeVisible();
  });

  test("managed install without launchable exe still opens the version screen", async () => {
    const bridge = makeBridge({
      installed: true,
      getProxyLaunchStatus: async () => ({
        managedInstall: true,
        currentVersion: "1.0.0",
        knownLatest: "1.2.0",
        canLaunch: false,
        codexExe: null,
        message: "未找到可启动的 Codex.exe"
      })
    });

    render(<App bridge={bridge} />);

    expect(await screen.findByRole("heading", { name: "发现 Codex 新版本" })).toBeVisible();
    expect(screen.queryByRole("heading", { name: "欢迎使用 Codex 安装助手" })).toBeNull();
    expect(screen.getByText("已安装")).toBeVisible();
    expect(screen.getByText("最新版本")).toBeVisible();
    expect(screen.getByText("1.0.0")).toBeVisible();
    expect(screen.getByText("1.2.0")).toBeVisible();
    expect(screen.getByRole("button", { name: "启动 Codex" })).toBeDisabled();
  });

  test("installed mode shows an up-to-date parity screen when no Codex update exists", async () => {
    render(
      <App
        bridge={makeBridge({
          installed: true,
          checkUpdateStatus: async () => ({
            kind: "upToDate",
            title: "Codex 已是最新",
            message: "当前版本 1.2.3",
            currentVersion: "1.2.3",
            latestVersion: "1.2.3",
            actions: []
          })
        })}
      />
    );

    expect(await screen.findByRole("heading", { name: "Codex 已是最新" })).toBeVisible();
    expect(screen.getByLabelText("当前页面")).toHaveTextContent("已是最新");
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
              message: "未找到可启动的 Codex.exe"
            }
          : {
              managedInstall: true,
              currentVersion: "1.2.0",
              knownLatest: "1.2.0",
              canLaunch: true,
              codexExe: "C:\\Users\\tester\\AppData\\Local\\Codex\\versions\\1.2.0\\Codex.exe",
              message: "可以启动 Codex"
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
    expect(screen.getByText("正在使用已配置的下载方式获取 Codex 更新包。")).toBeVisible();
    expect(screen.queryByRole("heading", { name: "发现 Codex 新版本" })).toBeNull();
    expect(screen.queryByRole("button", { name: "稍后提醒" })).toBeNull();

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

    expect(await screen.findByText("已更新到 Codex 1.2.0")).toBeVisible();
    expect(screen.getByLabelText("当前页面")).toHaveTextContent("更新完成");
    expect(screen.queryByRole("button", { name: "立即更新" })).toBeNull();
    expect(await screen.findByRole("button", { name: "启动 Codex" })).toBeEnabled();
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

    expect(await screen.findByRole("heading", { name: "发现 Codex 新版本" })).toBeVisible();
    expect(screen.queryByText("已安装的 Codex 版本")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "卸载" }));

    expect(await screen.findByText("确认卸载 Codex Windows 中文助手")).toBeVisible();
    expect(screen.getByText("已安装的 Codex 版本")).toBeVisible();
    expect(screen.getByText("Codex 登录数据")).toBeVisible();

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

    fireEvent.click(await screen.findByRole("button", { name: "卸载" }));
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

    expect(await screen.findByRole("heading", { name: "发现 Codex 新版本" })).toBeVisible();
    expect(screen.queryByText("发现启动器新版本")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "启动器更新" }));

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

    fireEvent.click(await screen.findByRole("button", { name: "启动器更新" }));
    fireEvent.click(await screen.findByRole("button", { name: "应用更新" }));

    expect(await screen.findByRole("heading", { name: "正在下载启动器" })).toBeVisible();
    expect(screen.getByText("512 / 1024 KB")).toBeVisible();
    expect(screen.getByText("50%")).toBeVisible();
  });

  test("secondary maintenance screens return to the installed home without leaking content", async () => {
    render(<App bridge={makeBridge({ installed: true })} />);

    expect(await screen.findByRole("heading", { name: "发现 Codex 新版本" })).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "卸载" }));
    expect(await screen.findByText("确认卸载 Codex Windows 中文助手")).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "返回" }));
    expect(await screen.findByRole("heading", { name: "发现 Codex 新版本" })).toBeVisible();
    expect(screen.queryByText("确认卸载 Codex Windows 中文助手")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "启动器更新" }));
    expect(await screen.findByText("发现启动器新版本")).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "返回" }));
    expect(await screen.findByRole("heading", { name: "发现 Codex 新版本" })).toBeVisible();
    expect(screen.queryByText("发现启动器新版本")).toBeNull();
    expect(screen.getByText("发现 Codex 新版本")).toBeVisible();
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
            message: "可以启动 Codex"
          }
        : {
            managedInstall: false,
            currentVersion: null,
            knownLatest: null,
            canLaunch: false,
            codexExe: null,
            message: "尚未完成安装"
          },
    launchCodex: async () => ({ launched: true, message: "已启动 Codex" }),
    launchInstalledCodex: async () => ({ launched: true, message: "已启动 Codex" }),
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
