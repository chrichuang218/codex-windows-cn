import {
  launcherUpdateActionLabels,
  updateActionLabels,
  updateStatusLabel,
  workspacePanelLabel
} from "../../appModel";
import type { UpdateAction } from "../../bridge";
import type { ReadyAppController } from "../../useAppController";
import { ProductShell } from "../../components/Shell";
import { ProgressScreen } from "../../components/Progress";

const updateDeferActionOrder: Exclude<UpdateAction, "updateNow">[] = [
  "notNow",
  "snoozeOneDay",
  "snoozeSevenDays",
  "skipThisVersion",
  "never"
];

export function InstalledWorkspace({ controller }: { controller: ReadyAppController }) {
  const { status, updateEvent, updateState, updateStatus, workspacePanel } = controller;
  const updateFlowActive = workspacePanel === "home" && (updateState !== "idle" || updateEvent);
  const shellTitle = updateFlowActive
    ? updateEvent?.title ?? "正在更新"
    : workspacePanel === "home"
      ? updateStatus.title
      : workspacePanelLabel[workspacePanel];
  const stageLabel = updateFlowActive
    ? updateEvent?.kind === "done"
      ? "更新完成"
      : updateEvent?.kind === "error"
        ? "更新失败"
        : "正在更新"
    : workspacePanel === "home"
      ? updateStatusLabel(updateStatus)
      : workspacePanelLabel[workspacePanel];

  return (
    <ProductShell
      bodyClassName="workspace-body"
      footer={<WorkspaceFooter controller={controller} />}
      modeLabel="已安装模式"
      productName={status.productName}
      stageLabel={stageLabel}
      title={shellTitle}
    >
      {workspacePanel === "home" ? <WorkspaceHome controller={controller} /> : null}
      {workspacePanel === "uninstall" ? <UninstallPanel controller={controller} /> : null}
      {workspacePanel === "launcherUpdate" ? <LauncherUpdatePanel controller={controller} /> : null}
    </ProductShell>
  );
}

function WorkspaceHome({ controller }: { controller: ReadyAppController }) {
  const {
    applyUpdateAction,
    installedVersion,
    latestVersion,
    launchMessage,
    proxyStatus,
    updateEvent,
    updateMessage,
    updateProgress,
    updateState,
    updateStatus
  } = controller;

  if (updateEvent?.kind === "done") {
    return (
      <section className="screen center-screen">
        <p className="success-text">{updateEvent.title}</p>
        {updateEvent.detail ? <p className="muted">{updateEvent.detail}</p> : null}
      </section>
    );
  }

  if (updateEvent?.kind === "error") {
    return (
      <section className="screen center-screen">
        <p className="error-text">{updateEvent.title}</p>
        <p className="muted">{updateEvent.message ?? updateEvent.detail}</p>
      </section>
    );
  }

  if (updateState !== "idle" || updateEvent) {
    return (
      <ProgressScreen
        detail={updateEvent?.detail}
        indeterminate={updateProgress === null}
        progress={updateProgress}
        title={updateEvent?.title ?? "正在更新"}
      />
    );
  }

  return (
    <section className="screen update-screen">
      <div className="update-copy">
        <p className="eyebrow">版本状态</p>
        <h2>{updateStatus.title}</h2>
        {updateStatus.kind === "error" && updateStatus.message ? (
          <p className="muted">{updateStatus.message}</p>
        ) : null}
      </div>

      <div className="version-compare" aria-label="Codex 版本比较">
        <div>
          <span>已安装</span>
          <strong>{installedVersion}</strong>
        </div>
        <div>
          <span>最新版本</span>
          <strong>{latestVersion}</strong>
        </div>
      </div>

      {!proxyStatus.canLaunch ? <p className="launch-note">{proxyStatus.message}</p> : null}

      {updateStatus.actions.length > 0 ? (
        <div className="defer-area">
          <span>不想现在更新？</span>
          <div className="defer-grid">
            {updateDeferActionOrder
              .filter((action) => updateStatus.actions.includes(action))
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
        </div>
      ) : null}

      {updateMessage ? <span className="inline-status">{updateMessage}</span> : null}
      {launchMessage ? <span className="inline-status">{launchMessage}</span> : null}
    </section>
  );
}

function UninstallPanel({ controller }: { controller: ReadyAppController }) {
  const {
    startUninstall,
    uninstallConfirmation,
    uninstallEvent,
    uninstallMessage,
    uninstallProgress,
    uninstallState,
    uninstallStatus
  } = controller;

  if (uninstallEvent?.kind === "done") {
    return (
      <section className="screen center-screen">
        <p className="success-text">{uninstallEvent.title}</p>
        {uninstallEvent.detail ? <p className="muted">{uninstallEvent.detail}</p> : null}
      </section>
    );
  }

  if (uninstallEvent?.kind === "error") {
    return (
      <section className="screen center-screen">
        <p className="error-text">{uninstallEvent.title}</p>
        <p className="muted">{uninstallEvent.message ?? uninstallEvent.detail}</p>
      </section>
    );
  }

  if (uninstallState !== "idle" || uninstallEvent) {
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
    <section className="screen workspace-screen">
      <div>
        <p className="eyebrow">卸载</p>
        <h2>{uninstallConfirmation.title}</h2>
        <p className="muted">{uninstallStatus.message}</p>
        <code>{uninstallConfirmation.root}</code>
      </div>
      <div className="uninstall-columns">
        <div>
          <strong>将删除</strong>
          {uninstallConfirmation.deleteItems.map((item) => (
            <span key={item}>{item}</span>
          ))}
        </div>
        <div>
          <strong>将保留</strong>
          {uninstallConfirmation.preserveItems.map((item) => (
            <span key={item}>{item}</span>
          ))}
        </div>
      </div>
      <div className="action-row">
        <button
          className="danger-button"
          disabled={uninstallStatus.kind !== "ready" || uninstallState !== "idle"}
          onClick={startUninstall}
          type="button"
        >
          确认卸载
        </button>
        {uninstallMessage ? <span className="inline-status">{uninstallMessage}</span> : null}
      </div>
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
    startLauncherUpdate
  } = controller;

  if (launcherUpdateEvent?.kind === "done") {
    return (
      <section className="screen center-screen">
        <p className="success-text">{launcherUpdateEvent.title}</p>
        {launcherUpdateEvent.detail ? <p className="muted">{launcherUpdateEvent.detail}</p> : null}
      </section>
    );
  }

  if (launcherUpdateEvent?.kind === "error") {
    return (
      <section className="screen center-screen">
        <p className="error-text">{launcherUpdateEvent.title}</p>
        <p className="muted">{launcherUpdateEvent.message ?? launcherUpdateEvent.detail}</p>
      </section>
    );
  }

  if (launcherUpdateState !== "idle" || launcherUpdateEvent) {
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
    <section className="screen workspace-screen">
      <div>
        <p className="eyebrow">启动器自更新</p>
        <h2>{launcherUpdateStatus.title}</h2>
        <p className="muted">{launcherUpdateStatus.message}</p>
        {launcherUpdateStatus.releaseUrl ? (
          <a href={launcherUpdateStatus.releaseUrl} rel="noreferrer" target="_blank">
            查看发布页
          </a>
        ) : null}
      </div>
      {launcherUpdateStatus.actions.length > 0 ? (
        <div className="button-strip">
          {launcherUpdateStatus.actions.includes("updateNow") ? (
            <button
              className="primary-button"
              disabled={launcherUpdateState !== "idle"}
              onClick={startLauncherUpdate}
              type="button"
            >
              应用更新
            </button>
          ) : null}
          {launcherUpdateStatus.actions
            .filter((action) => action !== "updateNow" && action !== "viewRelease")
            .map((action) => (
              <button
                className="secondary-button"
                key={action}
                onClick={() => applyLauncherUpdateAction(action)}
                type="button"
              >
                {launcherUpdateActionLabels[action]}
              </button>
            ))}
        </div>
      ) : null}
      {launcherUpdateMessage ? <span className="inline-status">{launcherUpdateMessage}</span> : null}
    </section>
  );
}

function WorkspaceFooter({ controller }: { controller: ReadyAppController }) {
  const {
    launchCodex,
    launchState,
    proxyStatus,
    setWorkspacePanel,
    startUpdate,
    updateEvent,
    updateState,
    updateStatus,
    workspacePanel
  } = controller;

  if (workspacePanel !== "home") {
    return (
      <>
        <button className="secondary-button" onClick={() => setWorkspacePanel("home")} type="button">
          返回
        </button>
        <span />
      </>
    );
  }

  return (
    <>
      <div className="footer-left compact-footer-actions">
        <button className="link-button" onClick={() => setWorkspacePanel("uninstall")} type="button">
          卸载
        </button>
        <button
          className="link-button"
          onClick={() => setWorkspacePanel("launcherUpdate")}
          type="button"
        >
          启动器更新
        </button>
        <button
          className="link-button"
          disabled={!proxyStatus.canLaunch || launchState !== "idle"}
          onClick={launchCodex}
          type="button"
        >
          {launchState === "idle" ? "启动 Codex" : "启动中"}
        </button>
      </div>
      {updateStatus.actions.includes("updateNow") && !updateEvent ? (
        <button
          className="primary-button footer-primary"
          disabled={updateState !== "idle"}
          onClick={startUpdate}
          type="button"
        >
          立即更新
        </button>
      ) : (
        <span />
      )}
    </>
  );
}
