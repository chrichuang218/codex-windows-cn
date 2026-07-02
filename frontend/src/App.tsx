import { useEffect, useState } from "react";
import type { AppBridge, AppStatus } from "./bridge";
import { mainPathLabels, tauriBridge } from "./bridge";
import "./styles.css";

type AppProps = {
  bridge?: AppBridge;
};

export function App({ bridge = tauriBridge }: AppProps) {
  const [status, setStatus] = useState<AppStatus | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;

    bridge
      .getAppStatus()
      .then((nextStatus) => {
        if (alive) {
          setStatus(nextStatus);
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

  if (!status) {
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
    </main>
  );
}
