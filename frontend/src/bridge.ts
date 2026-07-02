import { invoke } from "@tauri-apps/api/core";

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

export type AppBridge = {
  getAppStatus: () => Promise<AppStatus>;
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

export const tauriBridge: AppBridge = {
  getAppStatus: () => {
    if (!("__TAURI_INTERNALS__" in window)) {
      return Promise.resolve(fallbackStatus);
    }

    return invoke<AppStatus>("app_status");
  }
};

export const mainPathLabels: Record<MainPath, string> = {
  install: "安装",
  proxyLaunch: "代理启动",
  checkAndUpdate: "检查更新 / 更新",
  uninstall: "卸载",
  launcherSelfUpdate: "自更新"
};
