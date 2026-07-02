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

export type InstallModeDefaults = {
  mode: InstallMode;
  label: string;
  defaultRoot: string;
  createShortcut: boolean;
  registerUninstall: boolean;
  keepVersions: number;
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
  canLaunch: boolean;
  codexExe: string | null;
  message: string;
};

export type ProxyLaunchResult = {
  launched: boolean;
  message: string;
};

export type AppBridge = {
  getAppStatus: () => Promise<AppStatus>;
  getInstallerDefaults: () => Promise<InstallerDefaults>;
  startInstall: (request: InstallRequest) => Promise<InstallStart>;
  onInstallEvent: (handler: (event: InstallEvent) => void) => () => void;
  getProxyLaunchStatus: () => Promise<ProxyLaunchStatus>;
  launchCodex: () => Promise<ProxyLaunchResult>;
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
      keepVersions: 2,
      useCurrentJunction: true
    },
    {
      mode: "user",
      label: "当前用户",
      defaultRoot: "C:\\Users\\Public\\Codex",
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

const fallbackProxyLaunchStatus: ProxyLaunchStatus = {
  canLaunch: false,
  codexExe: null,
  message: "尚未完成安装"
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
  launchCodex: () => {
    if (!("__TAURI_INTERNALS__" in window)) {
      return Promise.resolve({ launched: true, message: "已启动 Codex" });
    }

    return invoke<ProxyLaunchResult>("launch_codex");
  }
};

export const mainPathLabels: Record<MainPath, string> = {
  install: "安装",
  proxyLaunch: "代理启动",
  checkAndUpdate: "检查更新 / 更新",
  uninstall: "卸载",
  launcherSelfUpdate: "自更新"
};
