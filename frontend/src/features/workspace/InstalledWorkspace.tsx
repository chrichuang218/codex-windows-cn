import {
  ArrowLeft,
  CheckCircle2,
  CircleAlert,
  Download,
  ExternalLink,
  HardDrive,
  Layers3,
  LayoutDashboard,
  LoaderCircle,
  Play,
  RefreshCw,
  Rocket,
  Settings2,
  ShieldCheck,
  Trash2,
  X
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import type { InstalledVersionStatus, VersionInventory } from "../../bridge";
import {
  launcherUpdateActionLabels,
  fetcherLabels,
  updateActionLabels,
  workspacePanelLabel
} from "../../appModel";
import type { UpdateAction } from "../../bridge";
import type { ReadyAppController } from "../../useAppController";
import { ProgressScreen } from "../../components/Progress";

const updateDeferActionOrder: Exclude<UpdateAction, "updateNow">[] = [
  "notNow",
  "snoozeOneDay",
  "snoozeSevenDays",
  "skipThisVersion",
  "never"
];

type PendingSwitch = {
  targetVersion: string | null;
  runningVersions: string[];
};

export function InstalledWorkspace({ controller }: { controller: ReadyAppController }) {
  const {
    bridge,
    proxyStatus,
    setWorkspacePanel,
    updateEvent,
    updateStatus,
    workspacePanel
  } = controller;
  const [inventory, setInventory] = useState<VersionInventory | null>(null);
  const [inventoryError, setInventoryError] = useState<string | null>(null);
  const [actionMessage, setActionMessage] = useState<string | null>(null);
  const [busyAction, setBusyAction] = useState<"launch" | "delete" | "settings" | null>(null);
  const [pendingSwitch, setPendingSwitch] = useState<PendingSwitch | null>(null);
  const [pendingDelete, setPendingDelete] = useState<InstalledVersionStatus | null>(null);

  const refreshInventory = useCallback(async () => {
    try {
      const next = await bridge.getVersionInventory();
      setInventory(next);
      setInventoryError(null);
    } catch (cause) {
      setInventoryError(errorMessage(cause, "无法读取本机版本"));
    }
  }, [bridge]);

  useEffect(() => {
    void refreshInventory();
  }, [refreshInventory]);

  useEffect(() => {
    const refreshVisibleInventory = () => {
      if (document.visibilityState === "visible") {
        void refreshInventory();
      }
    };
    const timer = window.setInterval(refreshVisibleInventory, 1000);
    window.addEventListener("focus", refreshVisibleInventory);
    return () => {
      window.clearInterval(timer);
      window.removeEventListener("focus", refreshVisibleInventory);
    };
  }, [refreshInventory]);

  useEffect(() => {
    if (updateEvent?.kind === "done") {
      void refreshInventory();
    }
  }, [refreshInventory, updateEvent]);

  const productName =
    updateStatus.productName ?? inventory?.productName ?? proxyStatus.productName ?? "Codex";
  const assistantName = "Codex Windows 中文助手";
  const defaultVersion = inventory ? inventory.defaultVersion : proxyStatus.currentVersion;
  const runningVersion = inventory?.runningVersions[0] ?? proxyStatus.runningVersions[0] ?? null;
  const runningIsOld = Boolean(
    runningVersion && defaultVersion && runningVersion !== defaultVersion
  );

  const launchVersion = async (targetVersion: string | null, switchRunning = false) => {
    setBusyAction("launch");
    setActionMessage(null);
    try {
      const result = await bridge.launchCodex({
        version: targetVersion,
        switchRunning
      });
      if (result.switchRequired) {
        setPendingSwitch({
          targetVersion: result.version,
          runningVersions: result.runningVersions
        });
        return;
      }
      setPendingSwitch(null);
      setActionMessage(result.message);
      await refreshInventory();
    } catch (cause) {
      setActionMessage(errorMessage(cause, "启动失败"));
    } finally {
      setBusyAction(null);
    }
  };

  const deleteVersion = async () => {
    if (!pendingDelete) {
      return;
    }
    setBusyAction("delete");
    setActionMessage(null);
    try {
      const result = await bridge.deleteInstalledVersion(pendingDelete.version);
      setInventory(result.inventory);
      setActionMessage(result.message);
      setPendingDelete(null);
    } catch (cause) {
      setActionMessage(errorMessage(cause, "删除版本失败"));
    } finally {
      setBusyAction(null);
    }
  };

  const saveRetention = async (keepVersions: number, keepAllVersions: boolean) => {
    setBusyAction("settings");
    setActionMessage(null);
    try {
      const result = await bridge.saveRetentionSettings({
        keepVersions,
        keepAllVersions
      });
      setInventory(result.inventory);
      setActionMessage(result.message);
    } catch (cause) {
      setActionMessage(errorMessage(cause, "保存保留策略失败"));
    } finally {
      setBusyAction(null);
    }
  };

  return (
    <main className="console-shell">
      <aside className="console-rail" aria-label="主导航">
        <div className="rail-brand" aria-label={assistantName}>
          <span>C</span>
        </div>
        <nav>
          <NavButton
            active={workspacePanel === "home"}
            icon={<LayoutDashboard size={18} />}
            label="概览"
            onClick={() => setWorkspacePanel("home")}
          />
          <NavButton
            active={workspacePanel === "versions"}
            icon={<Layers3 size={18} />}
            label="版本"
            onClick={() => setWorkspacePanel("versions")}
          />
          <NavButton
            active={workspacePanel === "settings"}
            icon={<Settings2 size={18} />}
            label="设置"
            onClick={() => setWorkspacePanel("settings")}
          />
        </nav>
        <div className="rail-status" title="官方 Microsoft Store 分发">
          <ShieldCheck size={16} />
        </div>
      </aside>

      <section className="console-workspace">
        <header className="console-header">
          <div>
            <p className="console-kicker">{workspacePanelLabel[workspacePanel]}</p>
            <h1>{assistantName}</h1>
          </div>
          <div className="header-status">
            <span className={updateStatus.kind === "error" ? "status-dot error" : "status-dot"} />
            {updateStatus.kind === "available" ? "有可用更新" : updateStatus.title}
          </div>
        </header>

        <div className="console-content">
          {workspacePanel === "home" ? (
            <OverviewPanel
              actionMessage={actionMessage}
              busy={busyAction === "launch"}
              controller={controller}
              defaultVersion={defaultVersion}
              inventory={inventory}
              inventoryError={inventoryError}
              onLaunch={() => void launchVersion(null)}
              onSwitchLatest={() => void launchVersion(null)}
              productName={productName}
              runningIsOld={runningIsOld}
              runningVersion={runningVersion}
            />
          ) : null}
          {workspacePanel === "versions" ? (
            <VersionsPanel
              actionMessage={actionMessage}
              busy={busyAction}
              inventory={inventory}
              inventoryError={inventoryError}
              onDelete={setPendingDelete}
              onLaunch={(version) => void launchVersion(version)}
            />
          ) : null}
          {workspacePanel === "settings" ? (
            <SettingsPanel
              actionMessage={actionMessage}
              busy={busyAction === "settings"}
              controller={controller}
              inventory={inventory}
              onSave={(count, keepAll) => void saveRetention(count, keepAll)}
            />
          ) : null}
          {workspacePanel === "uninstall" ? <UninstallPanel controller={controller} /> : null}
          {workspacePanel === "launcherUpdate" ? (
            <LauncherUpdatePanel controller={controller} />
          ) : null}
        </div>
      </section>

      {pendingSwitch ? (
        <ConfirmDialog
          confirmLabel="关闭并切换"
          description={`当前正在运行 ${pendingSwitch.runningVersions.join(
            "、"
          )}。切换会关闭当前应用并启动 ${pendingSwitch.targetVersion ?? "最新版"}。`}
          icon={<RefreshCw size={20} />}
          onCancel={() => setPendingSwitch(null)}
          onConfirm={() =>
            void launchVersion(pendingSwitch.targetVersion, true)
          }
          title="切换运行版本"
        />
      ) : null}

      {pendingDelete ? (
        <ConfirmDialog
          confirmLabel={busyAction === "delete" ? "正在删除" : "删除版本"}
          danger
          description={`将从本机删除 ${pendingDelete.productName} ${pendingDelete.version}，登录和项目数据会保留。`}
          icon={<Trash2 size={20} />}
          onCancel={() => setPendingDelete(null)}
          onConfirm={() => void deleteVersion()}
          title="确认删除"
        />
      ) : null}
    </main>
  );
}

function OverviewPanel({
  actionMessage,
  busy,
  controller,
  defaultVersion,
  inventory,
  inventoryError,
  onLaunch,
  onSwitchLatest,
  productName,
  runningIsOld,
  runningVersion
}: {
  actionMessage: string | null;
  busy: boolean;
  controller: ReadyAppController;
  defaultVersion: string | null;
  inventory: VersionInventory | null;
  inventoryError: string | null;
  onLaunch: () => void;
  onSwitchLatest: () => void;
  productName: string;
  runningIsOld: boolean;
  runningVersion: string | null;
}) {
  const {
    applyUpdateAction,
    startUpdate,
    updateEvent,
    updateProgress,
    updateState,
    updateStatus
  } = controller;

  if (updateState !== "idle" && updateEvent) {
    return (
      <ProgressScreen
        detail={updateEvent.detail}
        indeterminate={updateProgress === null}
        progress={updateProgress}
        title={updateEvent.title}
      />
    );
  }

  const totalSize = inventory?.versions.reduce((sum, item) => sum + item.sizeBytes, 0) ?? 0;

  return (
    <section className="console-view overview-view">
      {runningIsOld ? (
        <div className="version-alert">
          <CircleAlert size={18} />
          <div>
            <strong>当前仍运行历史版本 {runningVersion}</strong>
            <span>最新版 {defaultVersion} 已就绪</span>
          </div>
          <button className="button secondary" onClick={onSwitchLatest} type="button">
            <RefreshCw size={15} />
            切换到最新版
          </button>
        </div>
      ) : null}

      <div className="launch-hero">
        <div className="launch-copy">
          <span className="product-chip">{productName}</span>
          <h2>{defaultVersion ?? "正在读取版本"}</h2>
          <p>{updateStatus.message}</p>
        </div>
        <button
          className="launch-button"
          disabled={!defaultVersion || busy}
          onClick={onLaunch}
          type="button"
        >
          {busy ? <LoaderCircle className="spin" size={20} /> : <Play size={20} fill="currentColor" />}
          {busy ? "启动中" : `启动 ${productName}`}
        </button>
      </div>

      <div className="metric-strip" aria-label="安装摘要">
        <Metric icon={<Layers3 size={17} />} label="已安装" value={`${inventory?.versions.length ?? 0} 个版本`} />
        <Metric icon={<HardDrive size={17} />} label="占用空间" value={formatBytes(totalSize)} />
        <Metric
          icon={<ShieldCheck size={17} />}
          label="保留策略"
          value={inventory?.keepAllVersions ? "全部保留" : `最近 ${inventory?.keepVersions ?? 5} 个`}
        />
      </div>

      <div className="update-band">
        <div>
          <span className="section-label">官方更新</span>
          <strong>{updateStatus.title}</strong>
        </div>
        <div className="update-actions">
          {updateStatus.actions.includes("updateNow") ? (
            <button className="button primary" onClick={startUpdate} type="button">
              <Download size={15} />
              立即更新
            </button>
          ) : (
            <span className="verified-state"><CheckCircle2 size={16} /> 已同步</span>
          )}
        </div>
      </div>

      {updateStatus.actions.length > 1 ? (
        <div className="defer-row">
          {updateDeferActionOrder
            .filter((action) => updateStatus.actions.includes(action))
            .map((action) => (
              <button key={action} onClick={() => applyUpdateAction(action)} type="button">
                {updateActionLabels[action]}
              </button>
            ))}
        </div>
      ) : null}

      {inventoryError ? <p className="inline-message error">{inventoryError}</p> : null}
      {actionMessage ? <p className="inline-message">{actionMessage}</p> : null}
      {updateEvent?.kind === "done" ? <p className="inline-message success">{updateEvent.detail}</p> : null}
      {updateEvent?.kind === "error" ? (
        <p className="inline-message error">{updateEvent.message ?? updateEvent.detail}</p>
      ) : null}
    </section>
  );
}

function VersionsPanel({
  actionMessage,
  busy,
  inventory,
  inventoryError,
  onDelete,
  onLaunch
}: {
  actionMessage: string | null;
  busy: "launch" | "delete" | "settings" | null;
  inventory: VersionInventory | null;
  inventoryError: string | null;
  onDelete: (version: InstalledVersionStatus) => void;
  onLaunch: (version: string) => void;
}) {
  return (
    <section className="console-view versions-view">
      <div className="view-heading">
        <div>
          <span className="section-label">本机库存</span>
          <h2>已安装版本</h2>
        </div>
        <span>{inventory?.versions.length ?? 0} 个</span>
      </div>

      <div className="version-list" role="list">
        {!inventory && !inventoryError ? (
          <div className="empty-state"><LoaderCircle className="spin" size={22} />正在扫描版本</div>
        ) : null}
        {inventoryError ? <div className="empty-state error">{inventoryError}</div> : null}
        {inventory && inventory.versions.length === 0 ? (
          <div className="empty-state">未找到可启动版本</div>
        ) : null}
        {inventory?.versions.map((item) => (
          <div className="version-row" key={item.version} role="listitem">
            <div className={`version-mark ${item.appKind === "chatGpt" ? "chatgpt" : "codex"}`}>
              {item.appKind === "chatGpt" ? "G" : "C"}
            </div>
            <div className="version-identity">
              <strong>{item.version}</strong>
              <span>{item.productName}</span>
            </div>
            <div className="version-meta">
              <span>{formatBytes(item.sizeBytes)}</span>
              <span>{formatDate(item.installedAtUnix)}</span>
            </div>
            <div className="version-flags">
              {item.isDefault ? <span className="flag latest">最新版</span> : null}
              {item.isRunning ? <span className="flag running">运行中</span> : null}
            </div>
            <div className="row-actions">
              <button
                aria-label={`启动 ${item.productName} ${item.version}`}
                disabled={busy !== null}
                onClick={() => onLaunch(item.version)}
                title="启动此版本"
                type="button"
              >
                <Play size={16} />
              </button>
              <button
                aria-label={`删除 ${item.productName} ${item.version}`}
                className="danger-icon"
                disabled={!item.canDelete || busy !== null}
                onClick={() => onDelete(item)}
                title={item.canDelete ? "删除此版本" : "当前版本不可删除"}
                type="button"
              >
                <Trash2 size={16} />
              </button>
            </div>
          </div>
        ))}
      </div>
      {actionMessage ? <p className="inline-message">{actionMessage}</p> : null}
    </section>
  );
}

function SettingsPanel({
  actionMessage,
  busy,
  controller,
  inventory,
  onSave
}: {
  actionMessage: string | null;
  busy: boolean;
  controller: ReadyAppController;
  inventory: VersionInventory | null;
  onSave: (count: number, keepAll: boolean) => void;
}) {
  const { setWorkspacePanel } = controller;
  const [keepAll, setKeepAll] = useState(inventory?.keepAllVersions ?? false);
  const [count, setCount] = useState(inventory?.keepVersions ?? 5);

  useEffect(() => {
    if (inventory) {
      setKeepAll(inventory.keepAllVersions);
      setCount(inventory.keepVersions);
    }
  }, [inventory]);

  return (
    <section className="console-view settings-view">
      <div className="view-heading">
        <div>
          <span className="section-label">版本策略</span>
          <h2>保留与维护</h2>
        </div>
      </div>

      <section className="settings-section">
        <div className="settings-copy">
          <strong>自动保留</strong>
          <span>更新完成后清理超出策略且未运行的版本</span>
        </div>
        <div className="segmented" aria-label="版本保留模式">
          <button aria-pressed={!keepAll} onClick={() => setKeepAll(false)} type="button">
            最近版本
          </button>
          <button aria-pressed={keepAll} onClick={() => setKeepAll(true)} type="button">
            全部保留
          </button>
        </div>
        {!keepAll ? (
          <label className="count-control">
            <span>数量</span>
            <input
              aria-label="自动保留版本数量"
              max={20}
              min={1}
              onChange={(event) => setCount(clamp(Number(event.target.value), 1, 20))}
              type="number"
              value={count}
            />
          </label>
        ) : null}
        <button
          className="button primary save-settings"
          disabled={busy || !inventory}
          onClick={() => onSave(count, keepAll)}
          type="button"
        >
          {busy ? <LoaderCircle className="spin" size={15} /> : <CheckCircle2 size={15} />}
          保存设置
        </button>
      </section>

      <section className="settings-section install-details">
        <div className="settings-copy">
          <strong>安装位置</strong>
          <span>{inventory?.root || "正在读取"}</span>
        </div>
        <div className="settings-copy">
          <strong>下载方式</strong>
          <span>{inventory ? fetcherLabels[inventory.fetcher] : "正在读取"}</span>
        </div>
        <div className="settings-copy">
          <strong>稳定入口</strong>
          <span>{inventory?.useCurrentJunction ? "versions\\current" : "未启用"}</span>
        </div>
      </section>

      <section className="maintenance-strip">
        <button onClick={() => setWorkspacePanel("launcherUpdate")} type="button">
          <Rocket size={17} />
          <span><strong>启动器更新</strong><small>检查中文助手新版本</small></span>
        </button>
        <button className="maintenance-danger" onClick={() => setWorkspacePanel("uninstall")} type="button">
          <Trash2 size={17} />
          <span><strong>卸载助手</strong><small>保留登录和项目数据</small></span>
        </button>
      </section>

      {actionMessage ? <p className="inline-message">{actionMessage}</p> : null}
    </section>
  );
}

function UninstallPanel({ controller }: { controller: ReadyAppController }) {
  const {
    setWorkspacePanel,
    startUninstall,
    uninstallConfirmation,
    uninstallEvent,
    uninstallMessage,
    uninstallProgress,
    uninstallState,
    uninstallStatus
  } = controller;

  if (uninstallState !== "idle" || uninstallEvent) {
    if (uninstallEvent?.kind === "done" || uninstallEvent?.kind === "error") {
      return (
        <ResultState
          error={uninstallEvent.kind === "error"}
          message={uninstallEvent.message ?? uninstallEvent.detail}
          title={uninstallEvent.title}
        />
      );
    }
    return (
      <ProgressScreen
        detail={uninstallEvent?.detail}
        indeterminate={uninstallProgress === null}
        progress={uninstallProgress}
        title={uninstallEvent?.title ?? "正在卸载"}
      />
    );
  }

  return (
    <section className="console-view maintenance-view">
      <button className="back-button" onClick={() => setWorkspacePanel("settings")} type="button"><ArrowLeft size={15} />返回设置</button>
      <span className="section-label">卸载</span>
      <h2>{uninstallConfirmation.title}</h2>
      <p>{uninstallStatus.message}</p>
      <div className="uninstall-grid">
        <div><strong>将删除</strong>{uninstallConfirmation.deleteItems.map((item) => <span key={item}>{item}</span>)}</div>
        <div><strong>将保留</strong>{uninstallConfirmation.preserveItems.map((item) => <span key={item}>{item}</span>)}</div>
      </div>
      <button className="button danger" disabled={uninstallStatus.kind !== "ready"} onClick={startUninstall} type="button">
        <Trash2 size={15} />确认卸载
      </button>
      {uninstallMessage ? <p className="inline-message error">{uninstallMessage}</p> : null}
    </section>
  );
}

function LauncherUpdatePanel({ controller }: { controller: ReadyAppController }) {
  const {
    applyLauncherUpdateAction,
    launcherUpdateEvent,
    launcherUpdateMessage,
    launcherUpdateProgress,
    launcherUpdateState,
    launcherUpdateStatus,
    setWorkspacePanel,
    startLauncherUpdate
  } = controller;

  if (launcherUpdateState !== "idle" || launcherUpdateEvent) {
    if (launcherUpdateEvent?.kind === "done" || launcherUpdateEvent?.kind === "error") {
      return (
        <ResultState
          error={launcherUpdateEvent.kind === "error"}
          message={launcherUpdateEvent.message ?? launcherUpdateEvent.detail}
          onBack={() => setWorkspacePanel("settings")}
          title={launcherUpdateEvent.title}
        />
      );
    }
    return (
      <ProgressScreen
        detail={launcherUpdateEvent?.detail}
        indeterminate={launcherUpdateProgress === null}
        progress={launcherUpdateProgress}
        title={launcherUpdateEvent?.title ?? "正在自更新"}
      />
    );
  }

  return (
    <section className="console-view maintenance-view">
      <button className="back-button" onClick={() => setWorkspacePanel("settings")} type="button"><ArrowLeft size={15} />返回设置</button>
      <span className="section-label">启动器自更新</span>
      <h2>{launcherUpdateStatus.title}</h2>
      <p>{launcherUpdateStatus.message}</p>
      <div className="maintenance-actions">
        {launcherUpdateStatus.actions.includes("updateNow") ? (
          <button className="button primary" onClick={startLauncherUpdate} type="button"><Download size={15} />应用更新</button>
        ) : null}
        {launcherUpdateStatus.actions
          .filter((action) => action !== "updateNow" && action !== "viewRelease")
          .map((action) => (
            <button className="button secondary" key={action} onClick={() => applyLauncherUpdateAction(action)} type="button">
              {launcherUpdateActionLabels[action]}
            </button>
          ))}
        {launcherUpdateStatus.releaseUrl && launcherUpdateStatus.actions.includes("viewRelease") ? (
          <a className="button secondary" href={launcherUpdateStatus.releaseUrl} rel="noreferrer" target="_blank">
            <ExternalLink size={15} />查看发布页
          </a>
        ) : null}
      </div>
      {launcherUpdateMessage ? <p className="inline-message">{launcherUpdateMessage}</p> : null}
    </section>
  );
}

function NavButton({
  active,
  icon,
  label,
  onClick
}: {
  active: boolean;
  icon: React.ReactNode;
  label: string;
  onClick: () => void;
}) {
  return (
    <button aria-current={active ? "page" : undefined} onClick={onClick} title={label} type="button">
      {icon}
      <span>{label}</span>
    </button>
  );
}

function Metric({ icon, label, value }: { icon: React.ReactNode; label: string; value: string }) {
  return <div className="metric-item">{icon}<span>{label}<strong>{value}</strong></span></div>;
}

function ConfirmDialog({
  confirmLabel,
  danger = false,
  description,
  icon,
  onCancel,
  onConfirm,
  title
}: {
  confirmLabel: string;
  danger?: boolean;
  description: string;
  icon: React.ReactNode;
  onCancel: () => void;
  onConfirm: () => void;
  title: string;
}) {
  return (
    <div className="modal-backdrop">
      <section aria-modal="true" className="confirm-dialog" role="dialog">
        <button aria-label="关闭" className="modal-close" onClick={onCancel} type="button"><X size={17} /></button>
        <div className={danger ? "dialog-icon danger" : "dialog-icon"}>{icon}</div>
        <h2>{title}</h2>
        <p>{description}</p>
        <div className="dialog-actions">
          <button className="button secondary" onClick={onCancel} type="button">取消</button>
          <button className={danger ? "button danger" : "button primary"} onClick={onConfirm} type="button">{confirmLabel}</button>
        </div>
      </section>
    </div>
  );
}

function ResultState({
  error,
  message,
  onBack,
  title
}: {
  error: boolean;
  message: string;
  onBack?: () => void;
  title: string;
}) {
  return (
    <section className="result-state">
      {error ? <CircleAlert size={28} /> : <CheckCircle2 size={28} />}
      <h2>{title}</h2>
      <p>{message}</p>
      {onBack ? <button className="button secondary" onClick={onBack} type="button"><ArrowLeft size={15} />返回设置</button> : null}
    </section>
  );
}

function formatBytes(bytes: number) {
  if (!bytes) {
    return "0 B";
  }
  const units = ["B", "KB", "MB", "GB"];
  const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  return `${(bytes / 1024 ** index).toFixed(index >= 3 ? 2 : 1)} ${units[index]}`;
}

function formatDate(unix: number) {
  if (!unix) {
    return "未知时间";
  }
  return new Intl.DateTimeFormat("zh-CN", { month: "2-digit", day: "2-digit" }).format(
    new Date(unix * 1000)
  );
}

function clamp(value: number, min: number, max: number) {
  if (!Number.isFinite(value)) {
    return min;
  }
  return Math.min(max, Math.max(min, Math.round(value)));
}

function errorMessage(cause: unknown, fallback: string) {
  return cause instanceof Error ? cause.message : fallback;
}
