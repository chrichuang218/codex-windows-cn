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
  productName,
  stageLabel,
  title
}: ProductShellProps) {
  return (
    <main className="app-shell">
      <section className="window-frame">
        <section className="window-surface">
          <header className="window-header">
            <div>
              <h1>{productName}</h1>
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
      bodyClassName="boot-body"
      footer={<span className="footer-note">官方 Microsoft Store 分发</span>}
      modeLabel="安装向导"
      productName="Codex Windows 中文助手"
      stageLabel="正在加载"
      title="正在启动 Codex Windows 中文助手"
    >
      <ProgressScreen
        brandMark
        compact
        detail="正在读取安装、版本和更新状态。"
        indeterminate
        progress={null}
        title="正在启动 Codex Windows 中文助手"
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
