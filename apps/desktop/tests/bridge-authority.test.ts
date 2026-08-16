import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
const listen = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: (...args: unknown[]) => listen(...args),
}));

import { assertFixedPort, tauriSupervisorBridge } from "../src/bridge";

describe("assertFixedPort (closed range, renderer-side guard)", () => {
  it("accepts the closed 1..65535 integer range and the null sentinel", () => {
    expect(() => assertFixedPort(1)).not.toThrow();
    expect(() => assertFixedPort(8080)).not.toThrow();
    expect(() => assertFixedPort(65_535)).not.toThrow();
    expect(() => assertFixedPort(null)).not.toThrow();
  });

  it("rejects out-of-range, non-integer, and non-finite ports", () => {
    for (const bad of [0, -1, 65_536, 1.5, Number.NaN, Number.POSITIVE_INFINITY, Number.NEGATIVE_INFINITY]) {
      expect(() => assertFixedPort(bad), `port=${String(bad)}`).toThrow(RangeError);
    }
  });
});

describe("tauriSupervisorBridge (parameterless command surface)", () => {
  beforeEach(() => {
    invoke.mockReset();
    invoke.mockResolvedValue(undefined);
    listen.mockReset();
    listen.mockResolvedValue(() => {});
  });
  afterEach(() => {
    vi.clearAllMocks();
  });

  it("never forwards a URL, path, executable, argv, or lifecycle target on the no-arg commands", async () => {
    const noArgCalls: Array<[() => Promise<unknown>, string]> = [
      [() => tauriSupervisorBridge.getSnapshot(), "get_snapshot"],
      [() => tauriSupervisorBridge.startHarness(), "start_harness"],
      [() => tauriSupervisorBridge.stopHarness(), "stop_harness"],
      [() => tauriSupervisorBridge.runDoctor(), "run_doctor"],
      [() => tauriSupervisorBridge.openHarness(), "open_harness"],
      [() => tauriSupervisorBridge.revealLogFolder(), "reveal_log_folder"],
    ];
    for (const [call, command] of noArgCalls) {
      invoke.mockClear();
      await call();
      expect(invoke, command).toHaveBeenCalledTimes(1);
      expect(invoke, command).toHaveBeenCalledWith(command);
    }
  });

  it("opens the harness without supplying any authority (backend constructs the URL)", async () => {
    await tauriSupervisorBridge.openHarness();
    expect(invoke).toHaveBeenCalledWith("open_harness");
    expect(invoke.mock.calls[0]).toHaveLength(1);
  });

  it("forwards only the closed theme enum", async () => {
    for (const theme of ["system", "light", "dark"] as const) {
      invoke.mockClear();
      await tauriSupervisorBridge.setTheme(theme);
      expect(invoke).toHaveBeenCalledWith("set_theme", { theme });
    }
  });

  it("rejects an invalid fixed port before any IPC is issued", async () => {
    await expect(tauriSupervisorBridge.setFixedPort(0)).rejects.toThrow(RangeError);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("forwards a valid fixed port as the only settings argument", async () => {
    await tauriSupervisorBridge.setFixedPort(8080);
    expect(invoke).toHaveBeenCalledWith("set_fixed_port", { port: 8080 });
  });

  it("clears a fixed port with the null sentinel", async () => {
    await tauriSupervisorBridge.setFixedPort(null);
    expect(invoke).toHaveBeenCalledWith("set_fixed_port", { port: null });
  });

  it("subscribes to the supervisor snapshot event channel", async () => {
    const handler = () => {};
    const unlisten = () => {};
    listen.mockResolvedValue(unlisten);
    const unsubscribe = await tauriSupervisorBridge.subscribe(handler);
    expect(listen).toHaveBeenCalledWith("supervisor://snapshot", expect.any(Function));
    expect(unsubscribe).toBe(unlisten);
  });
});
