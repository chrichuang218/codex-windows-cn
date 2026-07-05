import { ErrorShell, LoadingSplash } from "./components/Shell";
import { InstallerWizard } from "./features/installer/InstallerWizard";
import { InstalledWorkspace } from "./features/workspace/InstalledWorkspace";
import type { AppProps } from "./appTypes";
import { tauriBridge } from "./bridge";
import { useAppController, type ReadyAppController } from "./useAppController";
import "./styles.css";

export function App({ bridge = tauriBridge }: AppProps) {
  const controller = useAppController(bridge);

  if (controller.loadError) {
    return <ErrorShell message={controller.loadError} />;
  }

  if (!controller.ready) {
    return <LoadingSplash />;
  }

  const readyController = controller as ReadyAppController;

  if (!readyController.installed) {
    return <InstallerWizard controller={readyController} />;
  }

  return <InstalledWorkspace controller={readyController} />;
}
