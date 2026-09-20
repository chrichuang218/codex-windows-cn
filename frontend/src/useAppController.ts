import { useCallback, useEffect, useRef, useState } from "react";
import type {
  AppBridge,
  InstallEvent,
  LauncherUpdateAction,
  LauncherUpdateEvent,
  UninstallEvent,
  UpdateAction,
  UpdateEvent
} from "./bridge";
import {
  fallbackLauncherUpdateStatus,
  fallbackProxyLaunchStatus,
  fallbackUninstallConfirmation,
  fallbackUninstallStatus,
  fallbackUpdateStatus
} from "./bridge";
import type { InstallForm, InstallerStep, LoadedAppData, WorkspacePanel } from "./appTypes";
import { formFromMode, initialInstallEvent, mergeInstallEvent, toPercent } from "./appModel";

function errorMessage(cause: unknown, fallback: string) {
  return cause instanceof Error ? cause.message : fallback;
}

const updateReminderActions: UpdateAction[] = [
  "updateNow",
  "notNow",
  "skipThisVersion",
  "snoozeOneDay",
  "snoozeSevenDays",
  "never"
];

function compareDottedVersion(left: string, right: string) {
  const leftParts = left.split(".").map((part) => Number.parseInt(part, 10) || 0);
  const rightParts = right.split(".").map((part) => Number.parseInt(part, 10) || 0);
  const count = Math.max(leftParts.length, rightParts.length);

  for (let index = 0; index < count; index++) {
    const diff = (leftParts[index] ?? 0) - (rightParts[index] ?? 0);
    if (diff !== 0) {
      return diff;
    }
  }

  return 0;
}

function cachedUpdateStatusFromProxy(
  proxyStatus: LoadedAppData["proxyStatus"]
): LoadedAppData["updateStatus"] {
  const currentVersion = proxyStatus.currentVersion;
  const latestVersion = proxyStatus.knownLatest;
  const productName = proxyStatus.productName || "Codex";

  if (
    currentVersion &&
    latestVersion &&
    compareDottedVersion(latestVersion, currentVersion) > 0
  ) {
    return {
      kind: "available",
      title: `发现 ${productName} 新版本`,
      message: `当前版本 ${currentVersion}，可更新到 ${latestVersion}`,
      currentVersion,
      latestVersion,
      productName,
      actions: updateReminderActions
    };
  }

  if (currentVersion && latestVersion) {
    return {
      kind: "upToDate",
      title: `${productName} 已是最新版本`,
      message: `当前版本 ${currentVersion}`,
      currentVersion,
      latestVersion,
      productName,
      actions: []
    };
  }

  if (currentVersion) {
    return {
      kind: "skipped",
      title: `${productName} 已安装`,
      message: "后台检查更新中，不影响启动。",
      currentVersion,
      latestVersion: currentVersion,
      productName,
      actions: []
    };
  }

  return {
    kind: "skipped",
    title: `${productName} 已安装`,
    message: "后台检查更新中，不影响启动。",
    currentVersion,
    latestVersion,
    productName,
    actions: []
  };
}

function mergeProgressEvent<
  T extends { kind: string; title: string; detail: string; progress: number | null }
>(
  current: T | null,
  event: T
): T {
  if (current?.kind === "done" || current?.kind === "error") {
    return current;
  }

  if (event.kind !== "progress" || event.detail.trim() !== "" || !current) {
    return event;
  }

  return {
    ...event,
    title: current.title,
    detail: current.detail
  };
}

export function useAppController(bridge: AppBridge) {
  const [status, setStatus] = useState<LoadedAppData["status"] | null>(null);
  const [installerDefaults, setInstallerDefaults] =
    useState<LoadedAppData["installerDefaults"] | null>(null);
  const [proxyStatus, setProxyStatus] = useState<LoadedAppData["proxyStatus"] | null>(null);
  const [updateStatus, setUpdateStatus] =
    useState<LoadedAppData["updateStatus"]>(fallbackUpdateStatus);
  const [updateCheckReady, setUpdateCheckReady] = useState(false);
  const [launcherUpdateStatus, setLauncherUpdateStatus] =
    useState<LoadedAppData["launcherUpdateStatus"]>(fallbackLauncherUpdateStatus);
  const [launcherUpdateCheckReady, setLauncherUpdateCheckReady] = useState(false);
  const [uninstallConfirmation, setUninstallConfirmation] =
    useState<LoadedAppData["uninstallConfirmation"] | null>(null);
  const [uninstallStatus, setUninstallStatus] =
    useState<LoadedAppData["uninstallStatus"] | null>(null);
  const [installForm, setInstallForm] = useState<InstallForm | null>(null);
  const [installerStep, setInstallerStep] = useState<InstallerStep>("welcome");
  const [workspacePanel, setWorkspacePanel] = useState<WorkspacePanel>("home");
  const [forcedWorkspace, setForcedWorkspace] = useState(false);
  const [installState, setInstallState] = useState<
    "idle" | "starting" | "running" | "cancelling"
  >("idle");
  const [installCancellable, setInstallCancellable] = useState(true);
  const [installEvent, setInstallEvent] = useState<InstallEvent | null>(null);
  const [installFailure, setInstallFailure] = useState<string | null>(null);
  const [launchState, setLaunchState] = useState<"idle" | "launching">("idle");
  const [launchMessage, setLaunchMessage] = useState<string | null>(null);
  const [installedProductName, setInstalledProductName] = useState("Codex");
  const [updateState, setUpdateState] = useState<"idle" | "running">("idle");
  const [updateEvent, setUpdateEvent] = useState<UpdateEvent | null>(null);
  const [updateMessage, setUpdateMessage] = useState<string | null>(null);
  const [launcherUpdateState, setLauncherUpdateState] = useState<"idle" | "running">("idle");
  const [launcherUpdateEvent, setLauncherUpdateEvent] = useState<LauncherUpdateEvent | null>(null);
  const [launcherUpdateMessage, setLauncherUpdateMessage] = useState<string | null>(null);
  const [uninstallState, setUninstallState] = useState<"idle" | "running">("idle");
  const [uninstallEvent, setUninstallEvent] = useState<UninstallEvent | null>(null);
  const [uninstallMessage, setUninstallMessage] = useState<string | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  const updateCheckGeneration = useRef(0);
  const launcherCheckGeneration = useRef(0);

  const refreshUpdateStatus = useCallback(async (proxy: LoadedAppData["proxyStatus"]) => {
    const generation = ++updateCheckGeneration.current;
    try {
      const next = proxy.managedInstall ? await bridge.checkUpdateStatus() : fallbackUpdateStatus;
      if (generation === updateCheckGeneration.current) {
        setUpdateStatus(next);
      }
    } catch (cause) {
      if (generation === updateCheckGeneration.current) {
        setUpdateStatus({
          kind: "error",
          title: "检查更新失败",
          message: errorMessage(cause, `无法读取 ${proxy.productName} 更新状态`),
          currentVersion: proxy.currentVersion,
          latestVersion: proxy.knownLatest,
          productName: proxy.productName,
          actions: []
        });
      }
    } finally {
      if (generation === updateCheckGeneration.current) {
        setUpdateCheckReady(true);
      }
    }
  }, [bridge]);

  const refreshLauncherStatus = useCallback(async () => {
    const generation = ++launcherCheckGeneration.current;
    try {
      const next = await bridge.checkLauncherUpdateStatus();
      if (generation === launcherCheckGeneration.current) {
        setLauncherUpdateStatus(next);
      }
    } catch (cause) {
      if (generation === launcherCheckGeneration.current) {
        setLauncherUpdateStatus((current) => ({
          ...current,
          kind: "error",
          title: "检查启动器更新失败",
          message: errorMessage(cause, "无法读取启动器更新状态"),
          actions: []
        }));
      }
    } finally {
      if (generation === launcherCheckGeneration.current) {
        setLauncherUpdateCheckReady(true);
      }
    }
  }, [bridge]);

  useEffect(() => {
    let alive = true;

    Promise.all([
      bridge.getAppStatus(),
      bridge.getInstallerDefaults(),
      bridge.getProxyLaunchStatus().catch(() => fallbackProxyLaunchStatus),
      bridge.getUninstallConfirmation().catch(() => fallbackUninstallConfirmation),
      bridge.getUninstallStatus().catch(() => fallbackUninstallStatus)
    ])
      .then(
        ([
          nextStatus,
          nextInstallerDefaults,
          nextProxyStatus,
          nextUninstallConfirmation,
          nextUninstallStatus
        ]) => {
          if (!alive) {
            return;
          }

          setStatus(nextStatus);
          setInstallerDefaults(nextInstallerDefaults);
          setProxyStatus(nextProxyStatus);
          setUninstallConfirmation(nextUninstallConfirmation);
          setUninstallStatus(nextUninstallStatus);
          setInstallForm(formFromMode(nextInstallerDefaults, nextInstallerDefaults.recommendedMode));

          setUpdateStatus(nextProxyStatus.managedInstall
            ? cachedUpdateStatusFromProxy(nextProxyStatus)
            : fallbackUpdateStatus);
          void refreshUpdateStatus(nextProxyStatus);
          void refreshLauncherStatus();
        }
      )
      .catch((cause: unknown) => {
        if (alive) {
          setLoadError(errorMessage(cause, "无法读取应用状态"));
        }
      });

    return () => {
      alive = false;
      updateCheckGeneration.current += 1;
      launcherCheckGeneration.current += 1;
    };
  }, [bridge, refreshUpdateStatus, refreshLauncherStatus]);

  useEffect(
    () =>
      bridge.onInstallEvent((event) => {
        setInstallEvent((current) => mergeInstallEvent(current, event));
      }),
    [bridge]
  );

  useEffect(() => {
    if (installerStep !== "progress" || installState === "idle") {
      return;
    }

    let alive = true;
    const poll = () => {
      bridge
        .getInstallStatus()
        .then((event) => {
          if (alive && event) {
            setInstallEvent((current) => mergeInstallEvent(current, event));
          }
        })
        .catch(() => {});
    };

    poll();
    const timer = window.setInterval(poll, 500);

    return () => {
      alive = false;
      window.clearInterval(timer);
    };
  }, [bridge, installerStep, installState]);

  useEffect(() => bridge.onUpdateEvent((event) => {
    setUpdateEvent((current) => mergeProgressEvent(current, event));
  }), [bridge]);
  useEffect(
    () =>
      bridge.onLauncherUpdateEvent((event) => {
        setLauncherUpdateEvent((current) => mergeProgressEvent(current, event));
      }),
    [bridge]
  );
  useEffect(() => bridge.onUninstallEvent(setUninstallEvent), [bridge]);

  useEffect(() => {
    if (uninstallState !== "running") {
      return;
    }

    let alive = true;
    const poll = () => {
      bridge
        .getUninstallProgress()
        .then((event) => {
          if (alive && event) {
            setUninstallEvent(event);
          }
        })
        .catch(() => {});
    };

    poll();
    const timer = window.setInterval(poll, 500);

    return () => {
      alive = false;
      window.clearInterval(timer);
    };
  }, [bridge, uninstallState]);

  useEffect(() => {
    if (updateState !== "running") {
      return;
    }

    let alive = true;
    const poll = () => {
      bridge
        .getUpdateStatus()
        .then((event) => {
          if (alive && event) {
            setUpdateEvent((current) => mergeProgressEvent(current, event));
          }
        })
        .catch(() => {});
    };

    poll();
    const timer = window.setInterval(poll, 500);

    return () => {
      alive = false;
      window.clearInterval(timer);
    };
  }, [bridge, updateState]);

  useEffect(() => {
    if (launcherUpdateState !== "running") {
      return;
    }

    let alive = true;
    const poll = () => {
      bridge
        .getLauncherUpdateProgress()
        .then((event) => {
          if (alive && event) {
            setLauncherUpdateEvent((current) => mergeProgressEvent(current, event));
          }
        })
        .catch(() => {});
    };

    poll();
    const timer = window.setInterval(poll, 500);

    return () => {
      alive = false;
      window.clearInterval(timer);
    };
  }, [bridge, launcherUpdateState]);

  useEffect(() => {
    if (installEvent?.kind === "done") {
      setInstallState("idle");
      setInstallerStep("done");
      bridge
        .getVersionInventory()
        .then((inventory) => setInstalledProductName(inventory.productName))
        .catch(() => {});
    }
    if (installEvent?.kind === "error") {
      setInstallState("idle");
      setInstallFailure(installEvent.message ?? installEvent.detail);
      setInstallerStep("error");
    }
  }, [bridge, installEvent]);

  useEffect(() => {
    if (updateEvent?.kind === "done") {
      updateCheckGeneration.current += 1;
      setUpdateState("idle");
      const version = updateEvent.version;
      if (version) {
        const productName =
          updateStatus.productName ?? proxyStatus?.productName ?? installedProductName;
        setUpdateStatus({
          kind: "upToDate",
          title: `${productName} 已更新`,
          message: `当前版本 ${version}`,
          currentVersion: version,
          latestVersion: version,
          productName,
          actions: []
        });
      }
      bridge
        .getProxyLaunchStatus()
        .then((nextProxyStatus) => {
          setProxyStatus(nextProxyStatus);
        })
        .catch(() => {});
    }
    if (updateEvent?.kind === "error") {
      setUpdateState("idle");
    }
  }, [bridge, installedProductName, proxyStatus?.productName, updateEvent, updateStatus.productName]);

  useEffect(() => {
    if (launcherUpdateEvent?.kind === "done" || launcherUpdateEvent?.kind === "error") {
      setLauncherUpdateState("idle");
    }
    if (launcherUpdateEvent?.kind === "done") {
      launcherCheckGeneration.current += 1;
      setLauncherUpdateCheckReady(true);
      setLauncherUpdateStatus((current) => ({
        ...current,
        kind: "skipped",
        title: launcherUpdateEvent.title,
        message: launcherUpdateEvent.message ?? launcherUpdateEvent.detail,
        actions: []
      }));
    }
  }, [launcherUpdateEvent]);

  useEffect(() => {
    if (uninstallEvent?.kind === "done" || uninstallEvent?.kind === "error") {
      setUninstallState("idle");
    }
  }, [uninstallEvent]);

  const chooseMode = (mode: InstallForm["mode"]) => {
    if (!installerDefaults) {
      return;
    }
    setInstallForm(formFromMode(installerDefaults, mode));
  };

  const patchInstallForm = (patch: Partial<InstallForm>) => {
    setInstallForm((current) => (current ? { ...current, ...patch } : current));
  };

  const startInstall = async () => {
    if (!installForm) {
      return;
    }

    setInstallState("starting");
    setInstallCancellable(installForm.mode !== "system");
    setInstallEvent(initialInstallEvent(installForm.fetcher));
    setInstallFailure(null);
    setInstallerStep("progress");

    try {
      const result = await bridge.startInstall({
        mode: installForm.mode,
        root: installForm.root,
        createShortcut: installForm.createShortcut,
        createDesktopShortcut: installForm.createDesktopShortcut,
        createAssistantDesktopShortcut: installForm.createAssistantDesktopShortcut,
        registerUninstall: installForm.registerUninstall,
        keepVersions: installForm.keepVersions,
        keepAllVersions: installForm.keepAllVersions,
        fetcher: installForm.fetcher,
        useCurrentJunction: installForm.useCurrentJunction,
        localMsix: null
      });
      setInstallCancellable(result.cancellable);
      setInstallState("running");
    } catch (cause) {
      setInstallState("idle");
      setInstallFailure(cause instanceof Error ? cause.message : "安装启动失败");
      setInstallerStep("error");
    }
  };

  const cancelInstall = async () => {
    setInstallState("cancelling");
    setInstallEvent((current) =>
      mergeInstallEvent(current, {
        kind: "phase",
        title: "正在取消安装",
        detail: "正在停止当前安装任务。",
        progress: null,
        version: null,
        message: null
      })
    );
    try {
      await bridge.cancelInstall();
    } catch (cause) {
      setInstallFailure(cause instanceof Error ? cause.message : "取消安装失败");
      setInstallerStep("error");
      setInstallState("idle");
    }
  };

  const launchCodex = async () => {
    setLaunchState("launching");
    setLaunchMessage(null);
    try {
      const result = await bridge.launchCodex();
      setLaunchMessage(result.message);
    } catch (cause) {
      setLaunchMessage(cause instanceof Error ? cause.message : "启动应用失败");
    } finally {
      setLaunchState("idle");
    }
  };

  const launchInstalledCodex = async () => {
    if (!installForm) {
      return;
    }

    setLaunchState("launching");
    setLaunchMessage(null);
    try {
      const result = await bridge.launchInstalledCodex({
        root: installForm.root,
        useCurrentJunction: installForm.useCurrentJunction
      });
      if (result.productName) {
        setInstalledProductName(result.productName);
      }
      setLaunchMessage(result.message);
    } catch (cause) {
      setLaunchMessage(cause instanceof Error ? cause.message : "启动应用失败");
    } finally {
      setLaunchState("idle");
    }
  };

  const startUpdate = async () => {
    updateCheckGeneration.current += 1;
    const productName = updateStatus.productName ?? proxyStatus?.productName ?? "Codex";
    setUpdateState("running");
    setUpdateEvent({
      kind: "phase",
      title: "正在下载更新",
      detail: `正在使用已配置的下载方式获取 ${productName} 更新包。`,
      progress: null,
      version: null,
      message: null
    });
    setUpdateMessage(null);
    try {
      await bridge.startUpdate();
    } catch (cause) {
      setUpdateState("idle");
      setUpdateEvent({
        kind: "error",
        title: "更新失败",
        detail: "",
        progress: null,
        version: null,
        message: cause instanceof Error ? cause.message : "启动更新失败"
      });
    }
  };

  const applyUpdateAction = async (action: UpdateAction) => {
    try {
      const result = await bridge.applyUpdateAction(action, updateStatus?.latestVersion ?? "");
      setUpdateMessage(result.message);
      if (proxyStatus) {
        await refreshUpdateStatus(proxyStatus);
      }
    } catch (cause) {
      setUpdateMessage(cause instanceof Error ? cause.message : "保存更新提醒失败");
    }
  };

  const startLauncherUpdate = async () => {
    launcherCheckGeneration.current += 1;
    setLauncherUpdateState("running");
    setLauncherUpdateEvent({
      kind: "phase",
      title: "正在准备自更新",
      detail: "正在启动自更新任务。",
      progress: null,
      message: null
    });
    setLauncherUpdateMessage(null);
    try {
      await bridge.startLauncherUpdate(launcherUpdateStatus?.latestVersion ?? "");
    } catch (cause) {
      setLauncherUpdateState("idle");
      setLauncherUpdateMessage(cause instanceof Error ? cause.message : "启动自更新失败");
    }
  };

  const applyLauncherUpdateAction = async (action: LauncherUpdateAction) => {
    try {
      const result = await bridge.applyLauncherUpdateAction(
        action,
        launcherUpdateStatus?.latestVersion ?? ""
      );
      setLauncherUpdateMessage(result.message);
      await refreshLauncherStatus();
    } catch (cause) {
      setLauncherUpdateMessage(cause instanceof Error ? cause.message : "保存自更新提醒失败");
    }
  };

  const startUninstall = async () => {
    setUninstallState("running");
    setUninstallEvent(null);
    setUninstallMessage(null);
    try {
      await bridge.startUninstall();
    } catch (cause) {
      setUninstallState("idle");
      setUninstallMessage(cause instanceof Error ? cause.message : "启动卸载失败");
    }
  };

  const installed = forcedWorkspace || (proxyStatus?.managedInstall ?? false);
  const installedVersion = updateStatus?.currentVersion ?? proxyStatus?.currentVersion ?? "未知";
  const latestVersion =
    updateStatus?.latestVersion ?? proxyStatus?.knownLatest ?? updateStatus?.currentVersion ?? "未知";
  const activeMode =
    installerDefaults && installForm
      ? installerDefaults.modes.find((mode) => mode.mode === installForm.mode) ??
        installerDefaults.modes[0]
      : null;

  return {
    activeMode,
    applyLauncherUpdateAction,
    applyUpdateAction,
    bridge,
    cancelInstall,
    chooseMode,
    installEvent,
    installFailure,
    installForm,
    installCancellable,
    installProgress: toPercent(installEvent?.progress),
    installState,
    installed,
    installedProductName,
    installedVersion,
    installerDefaults,
    installerStep,
    latestVersion,
    launchCodex,
    launchInstalledCodex,
    launcherUpdateEvent,
    launcherUpdateMessage,
    launcherUpdateCheckReady,
    launcherUpdateProgress: toPercent(launcherUpdateEvent?.progress),
    launcherUpdateState,
    launcherUpdateStatus,
    launchMessage,
    launchState,
    loadError,
    patchInstallForm,
    proxyStatus,
    ready:
      !!status &&
      !!installerDefaults &&
      !!proxyStatus &&
      !!uninstallConfirmation &&
      !!uninstallStatus &&
      !!installForm &&
      updateCheckReady,
    setInstallerStep,
    setForcedWorkspace,
    setWorkspacePanel,
    startInstall,
    startLauncherUpdate,
    startUninstall,
    startUpdate,
    status,
    uninstallConfirmation,
    uninstallEvent,
    uninstallMessage,
    uninstallProgress: toPercent(uninstallEvent?.progress),
    uninstallState,
    uninstallStatus,
    updateEvent,
    updateMessage,
    updateProgress: toPercent(updateEvent?.progress),
    updateState,
    updateStatus,
    workspacePanel
  };
}

export type AppController = ReturnType<typeof useAppController>;
export type ReadyAppController = AppController & LoadedAppData;
