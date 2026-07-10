import type {
  AppStatus,
  Fetcher,
  InstallMode,
  InstallerDefaults,
  LauncherUpdateStatus,
  ProxyLaunchStatus,
  UninstallConfirmation,
  UninstallStatus,
  UpdateStatus
} from "./bridge";

export type AppProps = {
  bridge?: import("./bridge").AppBridge;
};

export type InstallerStep = "welcome" | "mode" | "path" | "options" | "progress" | "done" | "error";
export type WorkspacePanel = "home" | "versions" | "settings" | "uninstall" | "launcherUpdate";

export type InstallForm = {
  mode: InstallMode;
  root: string;
  createShortcut: boolean;
  registerUninstall: boolean;
  keepVersions: number;
  keepAllVersions: boolean;
  fetcher: Fetcher;
  useCurrentJunction: boolean;
};

export type LoadedAppData = {
  status: AppStatus;
  installerDefaults: InstallerDefaults;
  proxyStatus: ProxyLaunchStatus;
  updateStatus: UpdateStatus;
  launcherUpdateStatus: LauncherUpdateStatus;
  uninstallConfirmation: UninstallConfirmation;
  uninstallStatus: UninstallStatus;
  installForm: InstallForm;
};
