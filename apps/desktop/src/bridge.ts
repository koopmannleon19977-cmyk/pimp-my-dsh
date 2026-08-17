import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  SUPERVISOR_PROTOCOL_VERSION,
  type Snapshot,
  type RestartPolicy,
  type ThemePreference,
} from "./types";

const commands = {
  getSnapshot: "get_snapshot",
  startHarness: "start_harness",
  stopHarness: "stop_harness",
  runDoctor: "run_doctor",
  openHarness: "open_harness",
  revealLogFolder: "reveal_log_folder",
  setTheme: "set_theme",
  setFixedPort: "set_fixed_port",
  setRestartPolicy: "set_restart_policy",
} as const;

const snapshotEvent = "supervisor://snapshot";

export interface SupervisorBridge {
  getSnapshot(): Promise<Snapshot>;
  startHarness(): Promise<void>;
  stopHarness(): Promise<void>;
  runDoctor(): Promise<void>;
  openHarness(): Promise<void>;
  revealLogFolder(): Promise<void>;
  setTheme(theme: ThemePreference): Promise<void>;
  setFixedPort(port: number | null): Promise<void>;
  setRestartPolicy(policy: RestartPolicy): Promise<void>;
  subscribe(onSnapshot: (snapshot: Snapshot) => void): Promise<() => void>;
}

function assertFixedPort(port: number | null): void {
  if (port !== null && (!Number.isInteger(port) || port < 1 || port > 65_535)) {
    throw new RangeError("Fixed port must be an integer from 1 through 65535.");
  }
}

function assertTheme(theme: ThemePreference): void {
  if (theme !== "system" && theme !== "light" && theme !== "dark") {
    throw new TypeError("Theme must be system, light, or dark.");
  }
}

function assertSnapshot(value: Snapshot): Snapshot {
  if (value.protocolVersion !== SUPERVISOR_PROTOCOL_VERSION) {
    throw new Error(`Unsupported supervisor protocol version: ${String(value.protocolVersion)}.`);
  }

  return value;
}
export const tauriSupervisorBridge: SupervisorBridge = {
  async getSnapshot() {
    return invoke<Snapshot>(commands.getSnapshot);
  },
  async startHarness() {
    await invoke<void>(commands.startHarness);
  },
  async stopHarness() {
    await invoke<void>(commands.stopHarness);
  },
  async runDoctor() {
    await invoke<void>(commands.runDoctor);
  },
  async openHarness() {
    await invoke<void>(commands.openHarness);
  },
  async revealLogFolder() {
    await invoke<void>(commands.revealLogFolder);
  },
  async setTheme(theme) {
    assertTheme(theme);
    await invoke<void>(commands.setTheme, { theme });
  },
  async setFixedPort(port) {
    assertFixedPort(port);
    await invoke<void>(commands.setFixedPort, { port });
  },
  async setRestartPolicy(policy) {
    if (policy !== "never" && policy !== "always") {
      throw new TypeError("Restart policy must be never or always.");
    }
    await invoke<void>(commands.setRestartPolicy, { policy });
  },
  async subscribe(onSnapshot) {
    return listen<Snapshot>(snapshotEvent, (event) => {
      onSnapshot(assertSnapshot(event.payload));
    });
  },
};

export { assertFixedPort };
