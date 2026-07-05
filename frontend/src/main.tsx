import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { App } from "./App";
import { ErrorBoundary } from "./ErrorBoundary";

const root = document.getElementById("root");

if (!root) {
  throw new Error("Missing root element");
}

const appRoot = root;
const minPrebootMs = 2600;

async function showPrebootWindow() {
  if (!isTauri()) {
    return;
  }

  const currentWindow = getCurrentWindow();
  await currentWindow.show();
  await currentWindow.setFocus().catch(() => undefined);
}

function wait(ms: number) {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

async function boot() {
  const started = performance.now();

  await showPrebootWindow().catch((cause) => {
    console.error("Failed to show Codex Windows preboot window", cause);
  });

  const remaining = minPrebootMs - (performance.now() - started);
  if (remaining > 0) {
    await wait(remaining);
  }

  createRoot(appRoot).render(
    <StrictMode>
      <ErrorBoundary>
        <App />
      </ErrorBoundary>
    </StrictMode>
  );
}

void boot();
