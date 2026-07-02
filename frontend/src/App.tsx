import { useEffect, useState } from "react";
import type {
  AppBridge,
  AppStatus,
  InstallEvent,
  InstallerDefaults,
  InstallMode,
  ProxyLaunchStatus,
  UpdateAction,
  UpdateEvent,
  UpdateStatus
} from "./bridge";
import { mainPathLabels, tauriBridge } from "./bridge";
import "./styles.css";

type AppProps = {
  bridge?: AppBridge;
};

export function App({ bridge = tauriBridge }: AppProps) {
  const [status, setStatus] = useState<AppStatus | null>(null);
  const [installerDefaults, setInstallerDefaults] = useState<InstallerDefaults | null>(null);
  const [proxyStatus, setProxyStatus] = useState<ProxyLaunchStatus | null>(null);
  const [updateStatus, setUpdateStatus] = useState<UpdateStatus | null>(null);
  const [selectedMode, setSelectedMode] = useState<InstallMode | null>(null);
  const [installState, setInstallState] = useState<"idle" | "starting" | "running">("idle");
  const [installEvent, setInstallEvent] = useState<InstallEvent | null>(null);
  const [launchState, setLaunchState] = useState<"idle" | "launching">("idle");
  const [launchMessage, setLaunchMessage] = useState<string | null>(null);
  const [updateState, setUpdateState] = useState<"idle" | "running">("idle");
  const [updateEvent, setUpdateEvent] = useState<UpdateEvent | null>(null);
  const [updateMessage, setUpdateMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;

    Promise.all([
      bridge.getAppStatus(),
      bridge.getInstallerDefaults(),
      bridge.getProxyLaunchStatus(),
      bridge.checkUpdateStatus()
    ])
      .then(([nextStatus, nextInstallerDefaults, nextProxyStatus, nextUpdateStatus]) => {
        if (alive) {
          setStatus(nextStatus);
          setInstallerDefaults(nextInstallerDefaults);
          setProxyStatus(nextProxyStatus);
          setUpdateStatus(nextUpdateStatus);
          setSelectedMode(nextInstallerDefaults.recommendedMode);
        }
      })
      .catch((cause: unknown) => {
        if (alive) {
          setError(cause instanceof Error ? cause.message : "无法读取应用状态");
        }
      });

    return () => {
      alive = false;
    };
  }, [bridge]);

  useEffect(() => bridge.onInstallEvent(setInstallEvent), [bridge]);
  useEffect(() => bridge.onUpdateEvent(setUpdateEvent), [bridge]);

  if (error) {
    return (
      <main className="shell shell-center">
        <section className="notice notice-error">
          <p className="eyebrow">启动失败</p>
          <h1>无法加载 Codex Windows 中文助手</h1>
          <p>{error}</p>
        </section>
      </main>
    );
  }

  if (!status || !installerDefaults || !proxyStatus || !updateStatus || !selectedMode) {
    return (
      <main className="shell shell-center">
        <section className="notice">
          <p className="eyebrow">正在启动</p>
          <h1>正在启动中文助手</h1>
          <p>正在读取本机启动器状态...</p>
        </section>
      </main>
    );
  }

  const selectedModeDefaults =
    installerDefaults.modes.find((mode) => mode.mode === selectedMode) ?? installerDefaults.modes[0];

  const startInstall = async () => {
    setInstallState("starting");
    try {
      setInstallEvent(null);
      await bridge.startInstall({
        mode: selectedModeDefaults.mode,
        root: selectedModeDefaults.defaultRoot,
        createShortcut: selectedModeDefaults.createShortcut,
        registerUninstall: selectedModeDefaults.registerUninstall,
        keepVersions: selectedModeDefaults.keepVersions,
        fetcher: installerDefaults.recommendedFetcher,
        useCurrentJunction: selectedModeDefaults.useCurrentJunction,
        localMsix: null
      });
      setInstallState("running");
    } catch (cause) {
      setInstallState("idle");
      setError(cause instanceof Error ? cause.message : "安装启动失败");
    }
  };

  const installProgress =
    installEvent?.progress === null || installEvent?.progress === undefined
      ? null
      : Math.round(installEvent.progress * 100);

  const launchCodex = async () => {
    setLaunchState("launching");
    setLaunchMessage(null);
    try {
      const result = await bridge.launchCodex();
      setLaunchMessage(result.message);
    } catch (cause) {
      setLaunchMessage(cause instanceof Error ? cause.message : "启动 Codex 失败");
    } finally {
      setLaunchState("idle");
    }
  };

  const startUpdate = async () => {
    setUpdateState("running");
    setUpdateEvent(null);
    setUpdateMessage(null);
    try {
      await bridge.startUpdate();
    } catch (cause) {
      setUpdateState("idle");
      setUpdateMessage(cause instanceof Error ? cause.message : "启动更新失败");
    }
  };

  const applyUpdateAction = async (action: UpdateAction) => {
    const latestVersion = updateStatus.latestVersion ?? "";
    const result = await bridge.applyUpdateAction(action, latestVersion);
    setUpdateMessage(result.message);
  };

  const updateProgress =
    updateEvent?.progress === null || updateEvent?.progress === undefined
      ? null
      : Math.round(updateEvent.progress * 100);

  return (
    <main className="shell">
      <section className="hero">
        <div className="brand-mark" aria-hidden="true">
          C
        </div>
        <div>
          <p className="eyebrow">{status.v1Boundary}</p>
          <h1>{status.productName}</h1>
          <p className="summary">
            面向中文 Windows 用户的 Codex 安装、更新、启动与卸载助手。v1
            先稳定交付五条主路径，后续再扩展诊断和修复能力。
          </p>
        </div>
      </section>

      <section className="main-paths" aria-label="v1 五条主路径">
        {status.mainPaths.map((path) => (
          <div className="path-row" key={path}>
            <span>{mainPathLabels[path]}</span>
            <small>v1 保留</small>
          </div>
        ))}
      </section>

      <section className="install-panel" aria-label="安装向导">
        <div className="section-heading">
          <p className="eyebrow">主路径 1</p>
          <h2>安装 Codex</h2>
        </div>

        <div className="mode-grid" aria-label="安装模式">
          {installerDefaults.modes.map((mode) => (
            <button
              aria-pressed={mode.mode === selectedMode}
              className="mode-button"
              key={mode.mode}
              onClick={() => setSelectedMode(mode.mode)}
              type="button"
            >
              <span>{mode.label}</span>
              {mode.mode === installerDefaults.recommendedMode ? <small>推荐</small> : null}
            </button>
          ))}
        </div>

        <label className="field">
          <span>安装位置</span>
          <input readOnly value={selectedModeDefaults.defaultRoot} />
        </label>

        <div className="option-list" aria-label="默认安装选项">
          <span>下载方式：{fetcherLabels[installerDefaults.recommendedFetcher]}</span>
          <span>保留 {selectedModeDefaults.keepVersions} 个版本</span>
          {selectedModeDefaults.createShortcut ? <span>创建开始菜单快捷方式</span> : null}
          {selectedModeDefaults.registerUninstall ? <span>写入 Windows 卸载入口</span> : null}
          {selectedModeDefaults.useCurrentJunction ? <span>维护 current 稳定入口</span> : null}
        </div>

        <div className="install-actions">
          <button
            className="primary-button"
            disabled={installState !== "idle"}
            onClick={startInstall}
            type="button"
          >
            {installState === "idle" ? "开始安装" : "安装中"}
          </button>
          {installState !== "idle" ? <span>正在安装</span> : null}
        </div>

        {installState !== "idle" && installEvent ? (
          <div className="install-progress">
            <strong>{installEvent.title}</strong>
            {installEvent.detail ? <span>{installEvent.detail}</span> : null}
            {installProgress !== null ? (
              <div
                aria-valuemax={100}
                aria-valuemin={0}
                aria-valuenow={installProgress}
                className="progress-track"
                role="progressbar"
              >
                <div style={{ width: `${installProgress}%` }} />
              </div>
            ) : null}
          </div>
        ) : null}
      </section>

      <section className="install-panel launch-panel" aria-label="代理启动">
        <div className="section-heading">
          <p className="eyebrow">主路径 2</p>
          <h2>启动 Codex</h2>
        </div>

        <div className="launch-status">
          <strong>{proxyStatus.message}</strong>
          {proxyStatus.codexExe ? <code>{proxyStatus.codexExe}</code> : null}
        </div>

        <div className="install-actions">
          <button
            className="primary-button"
            disabled={!proxyStatus.canLaunch || launchState !== "idle"}
            onClick={launchCodex}
            type="button"
          >
            {launchState === "idle" ? "启动" : "启动中"}
          </button>
          {launchMessage ? <span>{launchMessage}</span> : null}
        </div>
      </section>

      <section className="install-panel launch-panel" aria-label="检查更新">
        <div className="section-heading">
          <p className="eyebrow">主路径 3</p>
          <h2>检查更新</h2>
        </div>

        <div className="launch-status">
          <strong>{updateStatus.title}</strong>
          <span>{updateStatus.message}</span>
        </div>

        {updateStatus.actions.length > 0 ? (
          <div className="update-actions">
            {updateStatus.actions.includes("updateNow") ? (
              <button
                className="primary-button"
                disabled={updateState !== "idle"}
                onClick={startUpdate}
                type="button"
              >
                立即更新
              </button>
            ) : null}
            {updateStatus.actions
              .filter((action) => action !== "updateNow")
              .map((action) => (
                <button
                  className="secondary-button"
                  key={action}
                  onClick={() => applyUpdateAction(action)}
                  type="button"
                >
                  {updateActionLabels[action]}
                </button>
              ))}
          </div>
        ) : null}

        {updateState !== "idle" ? <span className="inline-status">正在更新</span> : null}
        {updateMessage ? <span className="inline-status">{updateMessage}</span> : null}

        {updateState !== "idle" && updateEvent ? (
          <div className="install-progress">
            <strong>{updateEvent.title}</strong>
            {updateEvent.detail ? <span>{updateEvent.detail}</span> : null}
            {updateProgress !== null ? (
              <div
                aria-valuemax={100}
                aria-valuemin={0}
                aria-valuenow={updateProgress}
                className="progress-track"
                role="progressbar"
              >
                <div style={{ width: `${updateProgress}%` }} />
              </div>
            ) : null}
          </div>
        ) : null}
      </section>
    </main>
  );
}

const fetcherLabels = {
  direct: "直连 Microsoft Store",
  winget: "winget",
  localFile: "本地 MSIX"
};

const updateActionLabels: Record<Exclude<UpdateAction, "updateNow">, string> = {
  notNow: "稍后提醒",
  skipThisVersion: "跳过此版本",
  snoozeOneDay: "1 天后提醒",
  snoozeSevenDays: "7 天后提醒",
  never: "关闭提醒"
};
