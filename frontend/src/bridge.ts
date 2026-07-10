import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type MainPath =
  | "install"
  | "proxyLaunch"
  | "checkAndUpdate"
  | "uninstall"
  | "launcherSelfUpdate";

export type AppStatus = {
  productName: string;
  v1Boundary: string;
  mainPaths: MainPath[];
};

export type InstallMode = "portable" | "user" | "system";
export type Fetcher = "direct" | "winget" | "localFile";
export type AppKind = "codex" | "chatGpt";

export type InstallModeDefaults = {
  mode: InstallMode;
  label: string;
  defaultRoot: string;
  createShortcut: boolean;
  registerUninstall: boolean;
  keepVersions: number;
  keepAllVersions: boolean;
  useCurrentJunction: boolean;
};

export type InstallerDefaults = {
  recommendedMode: InstallMode;
  recommendedFetcher: Fetcher;
  modes: InstallModeDefaults[];
  fetchers: Fetcher[];
};

export type InstallRequest = {
  mode: InstallMode;
  root: string;
  createShortcut: boolean;
  registerUninstall: boolean;
  keepVersions: number;
  keepAllVersions: boolean;
  fetcher: Fetcher;
  useCurrentJunction: boolean;
  localMsix: string | null;
};

export type InstallStart = {
  accepted: boolean;
};

export type InstallEvent = {
  kind: "phase" | "progress" | "done" | "error";
  title: string;
  detail: string;
  progress: number | null;
  version: string | null;
  message: string | null;
};

export type ProxyLaunchStatus = {
  managedInstall: boolean;
  currentVersion: string | null;
  knownLatest: string | null;
  canLaunch: boolean;
  codexExe: string | null;
  productName: string;
  runningVersions: string[];
  message: string;
};

export type ProxyLaunchResult = {
  launched: boolean;
  switchRequired: boolean;
  version: string | null;
  productName: string | null;
  runningVersions: string[];
  message: string;
};

export type LaunchRequest = {
  version: string | null;
  switchRunning: boolean;
};

export type LaunchInstalledRequest = {
  root: string;
  useCurrentJunction: boolean;
};

export type InstalledVersionStatus = {
  version: string;
  appKind: AppKind;
  productName: string;
  executable: string;
  sizeBytes: number;
  installedAtUnix: number;
  isDefault: boolean;
  isRunning: boolean;
  canDelete: boolean;
};

export type VersionInventory = {
  productName: string;
  root: string;
  defaultVersion: string | null;
  runningVersions: string[];
  keepVersions: number;
  keepAllVersions: boolean;
  fetcher: Fetcher;
  useCurrentJunction: boolean;
  versions: InstalledVersionStatus[];
};

export type RetentionSettingsRequest = {
  keepVersions: number;
  keepAllVersions: boolean;
};

export type VersionActionResult = {
  applied: boolean;
  message: string;
  inventory: VersionInventory;
};

export type UpdateStatusKind = "upToDate" | "available" | "skipped" | "error";
export type UpdateAction =
  | "updateNow"
  | "notNow"
  | "skipThisVersion"
  | "snoozeOneDay"
  | "snoozeSevenDays"
  | "never";

export type UpdateStatus = {
  kind: UpdateStatusKind;
  title: string;
  message: string;
  currentVersion: string | null;
  latestVersion: string | null;
  productName?: string | null;
  actions: UpdateAction[];
};

export type UpdateStart = {
  accepted: boolean;
};

export type UpdateActionResult = {
  applied: boolean;
  message: string;
};

export type UpdateEvent = {
  kind: "phase" | "progress" | "done" | "error";
  title: string;
  detail: string;
  progress: number | null;
  version: string | null;
  message: string | null;
};

export type LauncherUpdateStatusKind = "upToDate" | "available" | "skipped" | "error";
export type LauncherUpdateAction =
  | "updateNow"
  | "viewRelease"
  | "notNow"
  | "skipThisVersion"
  | "snoozeOneDay"
  | "snoozeSevenDays"
  | "never";

export type LauncherUpdateStatus = {
  kind: LauncherUpdateStatusKind;
  title: string;
  message: string;
  currentVersion: string | null;
  latestVersion: string | null;
  releaseUrl: string | null;
  actions: LauncherUpdateAction[];
};

export type LauncherUpdateStart = {
  accepted: boolean;
};

export type LauncherUpdateActionResult = {
  applied: boolean;
  message: string;
};

export type LauncherUpdateEvent = {
  kind: "phase" | "progress" | "done" | "error";
  title: string;
  detail: string;
  progress: number | null;
  message: string | null;
};

export type UninstallConfirmation = {
  title: string;
  root: string;
  deleteItems: string[];
  preserveItems: string[];
};

export type UninstallStatus = {
  kind: "ready" | "blocked";
  title: string;
  message: string;
};

export type UninstallStart = {
  accepted: boolean;
};

export type UninstallEvent = {
  kind: "phase" | "progress" | "done" | "error";
  title: string;
  detail: string;
  progress: number | null;
  logPath: string | null;
  message: string | null;
};

export type AppBridge = {
  getAppStatus: () => Promise<AppStatus>;
  getInstallerDefaults: () => Promise<InstallerDefaults>;
  startInstall: (request: InstallRequest) => Promise<InstallStart>;
  cancelInstall: () => Promise<InstallStart>;
  getInstallStatus: () => Promise<InstallEvent | null>;
  onInstallEvent: (handler: (event: InstallEvent) => void) => () => void;
  getProxyLaunchStatus: () => Promise<ProxyLaunchStatus>;
  launchCodex: (request?: LaunchRequest) => Promise<ProxyLaunchResult>;
  launchInstalledCodex: (request: LaunchInstalledRequest) => Promise<ProxyLaunchResult>;
  getVersionInventory: () => Promise<VersionInventory>;
  deleteInstalledVersion: (version: string) => Promise<VersionActionResult>;
  saveRetentionSettings: (
    request: RetentionSettingsRequest
  ) => Promise<VersionActionResult>;
  checkUpdateStatus: () => Promise<UpdateStatus>;
  startUpdate: () => Promise<UpdateStart>;
  getUpdateStatus: () => Promise<UpdateEvent | null>;
  applyUpdateAction: (
    action: UpdateAction,
    latestVersion: string
  ) => Promise<UpdateActionResult>;
  onUpdateEvent: (handler: (event: UpdateEvent) => void) => () => void;
  getUninstallConfirmation: () => Promise<UninstallConfirmation>;
  getUninstallStatus: () => Promise<UninstallStatus>;
  startUninstall: () => Promise<UninstallStart>;
  getUninstallProgress: () => Promise<UninstallEvent | null>;
  onUninstallEvent: (handler: (event: UninstallEvent) => void) => () => void;
  checkLauncherUpdateStatus: () => Promise<LauncherUpdateStatus>;
  startLauncherUpdate: (latestVersion: string) => Promise<LauncherUpdateStart>;
  getLauncherUpdateProgress: () => Promise<LauncherUpdateEvent | null>;
  applyLauncherUpdateAction: (
    action: LauncherUpdateAction,
    latestVersion: string
  ) => Promise<LauncherUpdateActionResult>;
  onLauncherUpdateEvent: (handler: (event: LauncherUpdateEvent) => void) => () => void;
};

const fallbackStatus: AppStatus = {
  productName: "Codex Windows 中文助手",
  v1Boundary: "中文安装更新助手",
  mainPaths: [
    "install",
    "proxyLaunch",
    "checkAndUpdate",
    "uninstall",
    "launcherSelfUpdate"
  ]
};

const fallbackInstallerDefaults: InstallerDefaults = {
  recommendedMode: "user",
  recommendedFetcher: "direct",
  modes: [
    {
      mode: "portable",
      label: "便携模式",
      defaultRoot: ".\\CodexPortable",
      createShortcut: false,
      registerUninstall: false,
      keepVersions: 5,
      keepAllVersions: false,
      useCurrentJunction: true
    },
    {
      mode: "user",
      label: "当前用户",
      defaultRoot: "C:\\Users\\Public\\Codex",
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

export const fallbackProxyLaunchStatus: ProxyLaunchStatus = {
  managedInstall: false,
  currentVersion: null,
  knownLatest: null,
  canLaunch: false,
  codexExe: null,
  productName: "Codex",
  runningVersions: [],
  message: "尚未完成安装"
};

export const fallbackVersionInventory: VersionInventory = {
  productName: "Codex",
  root: "",
  defaultVersion: null,
  runningVersions: [],
  keepVersions: 5,
  keepAllVersions: false,
  fetcher: "direct",
  useCurrentJunction: true,
  versions: []
};

export const fallbackUpdateStatus: UpdateStatus = {
  kind: "skipped",
  title: "暂不检查更新",
  message: "尚未完成安装",
  currentVersion: null,
  latestVersion: null,
  actions: []
};

export const fallbackUninstallConfirmation: UninstallConfirmation = {
  title: "确认卸载 Codex Windows 中文助手",
  root: "尚未完成安装",
  deleteItems: ["已安装的桌面应用版本", "下载缓存", "启动器配置"],
  preserveItems: ["Codex/ChatGPT 登录数据", "日志和诊断信息"]
};

export const fallbackUninstallStatus: UninstallStatus = {
  kind: "blocked",
  title: "无法卸载",
  message: "尚未完成安装"
};

export const fallbackLauncherUpdateStatus: LauncherUpdateStatus = {
  kind: "skipped",
  title: "暂不检查启动器更新",
  message: "仅在 Tauri 桌面环境中检查自更新",
  currentVersion: null,
  latestVersion: null,
  releaseUrl: null,
  actions: []
};

export const tauriBridge: AppBridge = {
  getAppStatus: () => {
    if (!("__TAURI_INTERNALS__" in window)) {
      return Promise.resolve(fallbackStatus);
    }

    return invoke<AppStatus>("app_status");
  },
  getInstallerDefaults: () => {
    if (!("__TAURI_INTERNALS__" in window)) {
      return Promise.resolve(fallbackInstallerDefaults);
    }

    return invoke<InstallerDefaults>("installer_defaults");
  },
  startInstall: (request) => {
    if (!("__TAURI_INTERNALS__" in window)) {
      return Promise.resolve({ accepted: true });
    }

    return invoke<InstallStart>("start_install", { request });
  },
  cancelInstall: () => {
    if (!("__TAURI_INTERNALS__" in window)) {
      return Promise.resolve({ accepted: true });
    }

    return invoke<InstallStart>("cancel_install");
  },
  getInstallStatus: () => {
    if (!("__TAURI_INTERNALS__" in window)) {
      return Promise.resolve(null);
    }

    return invoke<InstallEvent | null>("install_status");
  },
  onInstallEvent: (handler) => {
    if (!("__TAURI_INTERNALS__" in window)) {
      return () => {};
    }

    let active = true;
    let unlisten: (() => void) | null = null;
    listen<InstallEvent>("install://event", (event) => handler(event.payload)).then((nextUnlisten) => {
      if (active) {
        unlisten = nextUnlisten;
      } else {
        nextUnlisten();
      }
    });

    return () => {
      active = false;
      unlisten?.();
    };
  },
  getProxyLaunchStatus: () => {
    if (!("__TAURI_INTERNALS__" in window)) {
      return Promise.resolve(fallbackProxyLaunchStatus);
    }

    return invoke<ProxyLaunchStatus>("proxy_launch_status");
  },
  launchCodex: (request) => {
    if (!("__TAURI_INTERNALS__" in window)) {
      const version = request?.version ?? fallbackVersionInventory.defaultVersion ?? "26.707.3748.0";
      return Promise.resolve({
        launched: true,
        switchRequired: false,
        version,
        productName: "ChatGPT",
        runningVersions: [],
        message: `已启动 ChatGPT ${version}`
      });
    }

    return invoke<ProxyLaunchResult>("launch_codex", { request: request ?? null });
  },
  launchInstalledCodex: (request) => {
    if (!("__TAURI_INTERNALS__" in window)) {
      return Promise.resolve({
        launched: true,
        switchRequired: false,
        version: null,
        productName: "Codex",
        runningVersions: [],
        message: "已启动 Codex"
      });
    }

    return invoke<ProxyLaunchResult>("launch_installed_codex", { request });
  },
  getVersionInventory: () => {
    if (!("__TAURI_INTERNALS__" in window)) {
      return Promise.resolve(fallbackVersionInventory);
    }
    return invoke<VersionInventory>("get_version_inventory");
  },
  deleteInstalledVersion: (version) => {
    if (!("__TAURI_INTERNALS__" in window)) {
      return Promise.resolve({
        applied: true,
        message: `已删除版本 ${version}`,
        inventory: fallbackVersionInventory
      });
    }
    return invoke<VersionActionResult>("delete_installed_version", { version });
  },
  saveRetentionSettings: (request) => {
    if (!("__TAURI_INTERNALS__" in window)) {
      return Promise.resolve({
        applied: true,
        message: request.keepAllVersions
          ? "已设置为全部保留"
          : `将自动保留最近 ${request.keepVersions} 个版本`,
        inventory: { ...fallbackVersionInventory, ...request }
      });
    }
    return invoke<VersionActionResult>("save_retention_settings", { request });
  },
  checkUpdateStatus: () => {
    if (!("__TAURI_INTERNALS__" in window)) {
      return Promise.resolve(fallbackUpdateStatus);
    }

    return invoke<UpdateStatus>("check_update_status");
  },
  startUpdate: () => {
    if (!("__TAURI_INTERNALS__" in window)) {
      return Promise.resolve({ accepted: true });
    }

    return invoke<UpdateStart>("start_update");
  },
  getUpdateStatus: () => {
    if (!("__TAURI_INTERNALS__" in window)) {
      return Promise.resolve(null);
    }

    return invoke<UpdateEvent | null>("update_status");
  },
  applyUpdateAction: (action, latestVersion) => {
    if (!("__TAURI_INTERNALS__" in window)) {
      return Promise.resolve({ applied: true, message: "已保存更新提醒设置" });
    }

    return invoke<UpdateActionResult>("apply_update_action", { action, latestVersion });
  },
  onUpdateEvent: (handler) => {
    if (!("__TAURI_INTERNALS__" in window)) {
      return () => {};
    }

    let active = true;
    let unlisten: (() => void) | null = null;
    listen<UpdateEvent>("update://event", (event) => handler(event.payload)).then((nextUnlisten) => {
      if (active) {
        unlisten = nextUnlisten;
      } else {
        nextUnlisten();
      }
    });

    return () => {
      active = false;
      unlisten?.();
    };
  },
  getUninstallConfirmation: () => {
    if (!("__TAURI_INTERNALS__" in window)) {
      return Promise.resolve(fallbackUninstallConfirmation);
    }

    return invoke<UninstallConfirmation>("uninstall_confirmation");
  },
  getUninstallStatus: () => {
    if (!("__TAURI_INTERNALS__" in window)) {
      return Promise.resolve(fallbackUninstallStatus);
    }

    return invoke<UninstallStatus>("uninstall_status");
  },
  startUninstall: () => {
    if (!("__TAURI_INTERNALS__" in window)) {
      return Promise.resolve({ accepted: true });
    }

    return invoke<UninstallStart>("start_uninstall");
  },
  getUninstallProgress: () => {
    if (!("__TAURI_INTERNALS__" in window)) {
      return Promise.resolve(null);
    }

    return invoke<UninstallEvent | null>("uninstall_progress");
  },
  onUninstallEvent: (handler) => {
    if (!("__TAURI_INTERNALS__" in window)) {
      return () => {};
    }

    let active = true;
    let unlisten: (() => void) | null = null;
    listen<UninstallEvent>("uninstall://event", (event) => handler(event.payload)).then(
      (nextUnlisten) => {
        if (active) {
          unlisten = nextUnlisten;
        } else {
          nextUnlisten();
        }
      }
    );

    return () => {
      active = false;
      unlisten?.();
    };
  },
  checkLauncherUpdateStatus: () => {
    if (!("__TAURI_INTERNALS__" in window)) {
      return Promise.resolve(fallbackLauncherUpdateStatus);
    }

    return invoke<LauncherUpdateStatus>("check_launcher_update_status");
  },
  startLauncherUpdate: (latestVersion) => {
    if (!("__TAURI_INTERNALS__" in window)) {
      return Promise.resolve({ accepted: true });
    }

    return invoke<LauncherUpdateStart>("start_launcher_update", { latestVersion });
  },
  getLauncherUpdateProgress: () => {
    if (!("__TAURI_INTERNALS__" in window)) {
      return Promise.resolve(null);
    }

    return invoke<LauncherUpdateEvent | null>("launcher_update_progress");
  },
  applyLauncherUpdateAction: (action, latestVersion) => {
    if (!("__TAURI_INTERNALS__" in window)) {
      return Promise.resolve({ applied: true, message: "已保存自更新提醒设置" });
    }

    return invoke<LauncherUpdateActionResult>("apply_launcher_update_action", {
      action,
      latestVersion
    });
  },
  onLauncherUpdateEvent: (handler) => {
    if (!("__TAURI_INTERNALS__" in window)) {
      return () => {};
    }

    let active = true;
    let unlisten: (() => void) | null = null;
    listen<LauncherUpdateEvent>("launcher-update://event", (event) =>
      handler(event.payload)
    ).then((nextUnlisten) => {
      if (active) {
        unlisten = nextUnlisten;
      } else {
        nextUnlisten();
      }
    });

    return () => {
      active = false;
      unlisten?.();
    };
  }
};

export const mainPathLabels: Record<MainPath, string> = {
  install: "安装",
  proxyLaunch: "代理启动",
  checkAndUpdate: "检查更新 / 更新",
  uninstall: "卸载",
  launcherSelfUpdate: "自更新"
};
