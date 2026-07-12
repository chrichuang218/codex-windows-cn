import type { Fetcher } from "../../bridge";
import {
  fetcherLabels,
  installerStepLabel,
  modeSubtitle
} from "../../appModel";
import type { ReadyAppController } from "../../useAppController";
import { AssistantMark } from "../../components/AssistantMark";
import { ProductShell } from "../../components/Shell";
import { ProgressScreen } from "../../components/Progress";

export function InstallerWizard({ controller }: { controller: ReadyAppController }) {
  const {
    activeMode,
    cancelInstall,
    chooseMode,
    installEvent,
    installFailure,
    installForm,
    installProgress,
    installerDefaults,
    installerStep,
    installState,
    installedProductName,
    launchMessage,
    patchInstallForm,
    setInstallerStep,
    startInstall,
    status
  } = controller;

  const stepTitle = installerStep === "welcome" ? "安装向导" : activeMode?.label ?? "安装向导";

  return (
    <ProductShell
      bodyClassName="installer-body"
      footer={<InstallerFooter controller={controller} />}
      modeLabel="安装向导"
      productName={status.productName}
      stageLabel={installerStepLabel[installerStep]}
      title={stepTitle}
    >
      {installerStep === "welcome" ? (
        <section className="screen welcome-screen center-screen">
          <AssistantMark label="ChatGPT 标志" />
          <div className="welcome-copy">
            <p className="eyebrow">{status.v1Boundary}</p>
            <h2>欢迎使用 Codex Windows 中文助手</h2>
            <p className="muted">下载并安装官方 Microsoft Store 桌面应用，无需打开 Store。</p>
          </div>
        </section>
      ) : null}

      {installerStep === "mode" ? (
        <section className="screen">
          <h2>选择安装范围</h2>
          <div className="mode-list" aria-label="安装模式">
            {installerDefaults.modes.map((mode) => (
              <button
                aria-pressed={mode.mode === installForm.mode}
                className="mode-card"
                key={mode.mode}
                onClick={() => chooseMode(mode.mode)}
                type="button"
              >
                <strong>{mode.label}</strong>
                <span>{modeSubtitle(mode)}</span>
              </button>
            ))}
          </div>
        </section>
      ) : null}

      {installerStep === "path" ? (
        <section className="screen">
          <h2>安装位置</h2>
          <label className="field">
            <span>目标文件夹</span>
            <input
              onChange={(event) => patchInstallForm({ root: event.target.value })}
              value={installForm.root}
            />
          </label>
          <p className="fine-print">
            versions 子目录会保存不同版本；current 稳定入口用于后续启动和更新。
          </p>
        </section>
      ) : null}

      {installerStep === "options" ? (
        <section className="screen">
          <h2>安装选项</h2>
          <div className="option-stack">
            <label>
              <input
                checked={installForm.createShortcut}
                onChange={(event) => patchInstallForm({ createShortcut: event.target.checked })}
                type="checkbox"
              />
              创建开始菜单快捷方式
            </label>
            <label>
              <input
                checked={installForm.createDesktopShortcut}
                onChange={(event) =>
                  patchInstallForm({ createDesktopShortcut: event.target.checked })
                }
                type="checkbox"
              />
              创建 ChatGPT 桌面快捷方式
            </label>
            <label>
              <input
                checked={installForm.createAssistantDesktopShortcut}
                onChange={(event) =>
                  patchInstallForm({ createAssistantDesktopShortcut: event.target.checked })
                }
                type="checkbox"
              />
              创建中文助手桌面快捷方式
            </label>
            <label>
              <input
                checked={installForm.registerUninstall}
                onChange={(event) => patchInstallForm({ registerUninstall: event.target.checked })}
                type="checkbox"
              />
              写入 Windows 卸载入口
            </label>
            <label>
              <input
                checked={installForm.registerCodexProtocol}
                onChange={(event) =>
                  patchInstallForm({ registerCodexProtocol: event.target.checked })
                }
                type="checkbox"
              />
              支持 CLI /app（注册 codex://）
            </label>
            <label>
              <input
                checked={installForm.useCurrentJunction}
                onChange={(event) => patchInstallForm({ useCurrentJunction: event.target.checked })}
                type="checkbox"
              />
              维护 current 稳定入口
            </label>
          </div>
          <div className="inline-fields">
            <label className="select-field">
              <span>保留版本</span>
              <select
                onChange={(event) => patchInstallForm({ keepVersions: Number(event.target.value) })}
                value={installForm.keepVersions}
              >
                {[1, 2, 3, 5].map((count) => (
                  <option key={count} value={count}>
                    {count}
                  </option>
                ))}
              </select>
            </label>
            <label className="select-field">
              <span>下载方式</span>
              <select
                onChange={(event) => patchInstallForm({ fetcher: event.target.value as Fetcher })}
                value={installForm.fetcher}
              >
                {installerDefaults.fetchers.map((fetcher) => (
                  <option key={fetcher} value={fetcher}>
                    {fetcherLabels[fetcher]}
                  </option>
                ))}
              </select>
            </label>
          </div>
        </section>
      ) : null}

      {installerStep === "progress" ? (
        <ProgressScreen
          detail={installEvent?.detail}
          indeterminate={installProgress === null}
          progress={installProgress}
          title={installEvent?.title ?? (installState === "starting" ? "正在准备安装" : "正在安装")}
        />
      ) : null}

      {installerStep === "done" ? (
        <section className="screen center-screen">
          <p className="success-text">{installedProductName} 已安装</p>
          {installEvent?.version ? <p className="muted">版本 {installEvent.version}</p> : null}
          <code>{installForm.root}</code>
          {launchMessage ? <span className="inline-status">{launchMessage}</span> : null}
        </section>
      ) : null}

      {installerStep === "error" ? (
        <section className="screen">
          <p className="error-text">{installEvent?.title ?? "安装失败"}</p>
          {installFailure && installFailure !== installEvent?.title ? (
            <p className="muted">{installFailure}</p>
          ) : null}
          {!installFailure ? <p className="muted">安装过程中出现错误</p> : null}
        </section>
      ) : null}
    </ProductShell>
  );
}

function InstallerFooter({ controller }: { controller: ReadyAppController }) {
  const {
    cancelInstall,
    installCancellable,
    installForm,
    installerStep,
    installState,
    installedProductName,
    launchInstalledCodex,
    launchState,
    setInstallerStep,
    startInstall
  } = controller;

  if (installerStep === "welcome") {
    return (
      <>
        <span className="footer-note">官方 Microsoft Store 分发</span>
        <button className="primary-button" onClick={() => setInstallerStep("mode")} type="button">
          开始安装
        </button>
      </>
    );
  }

  if (installerStep === "mode") {
    return (
      <>
        <button className="secondary-button" onClick={() => setInstallerStep("welcome")} type="button">
          返回
        </button>
        <button className="primary-button" onClick={() => setInstallerStep("path")} type="button">
          下一步
        </button>
      </>
    );
  }

  if (installerStep === "path") {
    return (
      <>
        <button className="secondary-button" onClick={() => setInstallerStep("mode")} type="button">
          返回
        </button>
        <button
          className="primary-button"
          disabled={installForm.root.trim() === ""}
          onClick={() => setInstallerStep("options")}
          type="button"
        >
          下一步
        </button>
      </>
    );
  }

  if (installerStep === "options") {
    return (
      <>
        <button className="secondary-button" onClick={() => setInstallerStep("path")} type="button">
          返回
        </button>
        <button
          className="primary-button"
          disabled={installState !== "idle"}
          onClick={startInstall}
          type="button"
        >
          安装
        </button>
      </>
    );
  }

  if (installerStep === "progress") {
    if (!installCancellable) {
      return <span className="footer-note">管理员安装进行中，完成前请勿关闭窗口</span>;
    }
    return (
      <>
        <span />
        <button
          className="secondary-button"
          disabled={installState === "cancelling"}
          onClick={cancelInstall}
          type="button"
        >
          {installState === "cancelling" ? "取消中" : "取消安装"}
        </button>
      </>
    );
  }

  if (installerStep === "done") {
    return (
      <>
        <span />
        <button
          className="primary-button"
          disabled={launchState !== "idle"}
          onClick={launchInstalledCodex}
          type="button"
        >
          {launchState === "idle" ? `启动 ${installedProductName}` : "启动中"}
        </button>
      </>
    );
  }

  return (
    <>
      <button className="secondary-button" onClick={() => setInstallerStep("options")} type="button">
        返回
      </button>
      <button className="primary-button" onClick={startInstall} type="button">
        重试
      </button>
    </>
  );
}
