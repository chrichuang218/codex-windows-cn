import type {
  Fetcher,
  InstallEvent,
  InstallModeDefaults,
  InstallerDefaults,
  LauncherUpdateAction,
  UpdateAction,
  UpdateStatus
} from "./bridge";
import type { InstallForm, InstallerStep, WorkspacePanel } from "./appTypes";

export function initialInstallEvent(fetcher: Fetcher): InstallEvent {
  return {
    kind: "phase",
    title: initialInstallTitle(fetcher),
    detail: initialInstallDetail(fetcher),
    progress: null,
    version: null,
    message: null
  };
}

export function mergeInstallEvent(current: InstallEvent | null, event: InstallEvent): InstallEvent {
  if (event.kind !== "progress" || event.detail.trim() !== "" || !current) {
    return event;
  }

  if (current.title === "正在连接 Microsoft Store" && event.progress !== null) {
    return {
      ...event,
      title: "正在下载",
      detail: "正在下载官方桌面应用安装包。"
    };
  }

  return {
    ...event,
    title: current.title,
    detail: current.detail
  };
}

export function formFromMode(defaults: InstallerDefaults, mode: InstallForm["mode"]): InstallForm {
  const modeDefaults =
    defaults.modes.find((candidate) => candidate.mode === mode) ?? defaults.modes[0];

  return {
    mode: modeDefaults.mode,
    root: modeDefaults.defaultRoot,
    createShortcut: modeDefaults.createShortcut,
    registerUninstall: modeDefaults.registerUninstall,
    keepVersions: modeDefaults.keepVersions,
    keepAllVersions: modeDefaults.keepAllVersions,
    fetcher: defaults.recommendedFetcher,
    useCurrentJunction: modeDefaults.useCurrentJunction
  };
}

export function modeSubtitle(mode: InstallModeDefaults) {
  if (mode.mode === "portable") {
    return "自包含目录，不写系统卸载入口";
  }
  if (mode.mode === "system") {
    return "安装到 Program Files，需要管理员权限";
  }
  return "安装到当前用户目录，无需管理员权限";
}

export function toPercent(progress: number | null | undefined) {
  if (progress === null || progress === undefined) {
    return null;
  }
  return Math.round(progress * 100);
}

function initialInstallTitle(fetcher: Fetcher) {
  if (fetcher === "winget") {
    return "正在启动 winget";
  }
  if (fetcher === "localFile") {
    return "正在读取本地 MSIX";
  }
  return "正在连接 Microsoft Store";
}

function initialInstallDetail(fetcher: Fetcher) {
  if (fetcher === "winget") {
    return "正在调用 Windows 包管理器下载官方桌面应用。";
  }
  if (fetcher === "localFile") {
    return "正在准备从本地安装包安装官方桌面应用。";
  }
  return "正在解析官方桌面应用下载地址，首次安装可能需要几分钟。";
}

export const installerStepLabel: Record<InstallerStep, string> = {
  welcome: "欢迎",
  mode: "步骤 1 / 3",
  path: "步骤 2 / 3",
  options: "步骤 3 / 3",
  progress: "正在处理",
  done: "完成",
  error: "错误"
};

export const workspacePanelLabel: Record<WorkspacePanel, string> = {
  home: "概览",
  versions: "版本管理",
  settings: "设置",
  uninstall: "卸载助手",
  launcherUpdate: "启动器更新"
};

export function updateStatusLabel(status: UpdateStatus) {
  if (status.kind === "available") {
    return "有可用更新";
  }
  if (status.kind === "upToDate") {
    return "已是最新";
  }
  if (status.kind === "error") {
    return "更新检查失败";
  }
  return "代理模式";
}

export const fetcherLabels: Record<Fetcher, string> = {
  direct: "直连 Microsoft Store",
  winget: "winget",
  localFile: "本地 MSIX"
};

export const updateActionLabels: Record<Exclude<UpdateAction, "updateNow">, string> = {
  notNow: "稍后提醒",
  skipThisVersion: "跳过此版本",
  snoozeOneDay: "1 天后提醒",
  snoozeSevenDays: "7 天后提醒",
  never: "关闭提醒"
};

export const launcherUpdateActionLabels: Record<
  Exclude<LauncherUpdateAction, "updateNow" | "viewRelease">,
  string
> = {
  notNow: "稍后提醒",
  skipThisVersion: "跳过此版本",
  snoozeOneDay: "1 天后提醒",
  snoozeSevenDays: "7 天后提醒",
  never: "关闭提醒"
};
