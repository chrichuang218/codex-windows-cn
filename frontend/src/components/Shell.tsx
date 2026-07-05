import type { ReactNode } from "react";
import { ProgressScreen } from "./Progress";

type ProductShellProps = {
  bodyClassName?: string;
  children: ReactNode;
  footer: ReactNode;
  modeLabel: string;
  productName: string;
  stageLabel: string;
  title: string;
};

export function ProductShell({
  bodyClassName,
  children,
  footer,
  modeLabel,
  stageLabel,
  title
}: ProductShellProps) {
  return (
    <main className="app-shell">
      <section className="window-frame">
        <section className="window-surface">
          <header className="window-header">
            <div>
              <h1>Codex Updater</h1>
              <p>{modeLabel || title}</p>
            </div>
            <span aria-label="当前页面">{stageLabel}</span>
          </header>
          <div className="divider" />
          <div className={bodyClassName ? `window-body ${bodyClassName}` : "window-body"}>
            {children}
          </div>
          <footer className="window-footer">{footer}</footer>
        </section>
      </section>
    </main>
  );
}

export function LoadingSplash() {
  return (
    <ProductShell
      footer={<span />}
      modeLabel="启动中"
      productName="Codex Windows 中文助手"
      stageLabel="正在检查"
      title="正在检查更新"
    >
      <ProgressScreen
        detail="正在读取安装、更新和启动器状态。"
        indeterminate
        progress={null}
        title="正在检查更新"
      />
    </ProductShell>
  );
}

export function ErrorShell({ message }: { message: string }) {
  return (
    <main className="app-shell shell-center">
      <section className="window-frame notice notice-error">
        <section className="window-surface notice-surface">
          <p className="eyebrow">启动失败</p>
          <h1>无法加载 Codex Windows 中文助手</h1>
          <p>{message}</p>
        </section>
      </section>
    </main>
  );
}
