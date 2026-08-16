import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { App } from "./app";
import type { SupervisorBridge } from "./bridge";
import type { LifecycleState, Snapshot } from "./types";

function createSnapshot(state: LifecycleState = "stopped", overrides: Partial<Snapshot> = {}): Snapshot {
  return {
    protocolVersion: 1,
    revision: 1,
    state,
    reason: "initial state",
    runId: null,
    endpoint: state === "ready" || state === "running" ? "http://127.0.0.1:4310" : null,
    profile: "web",
    uptimeMs: state === "running" ? 61_000 : null,
    lastTransitionAt: "2026-08-16T08:00:00.000Z",
    busy: false,
    health: [{ id: "Pinned runtime", status: "ok", message: "Node and pnpm match the manifest." }],
    doctor: null,
    logs: [
      { runId: "run-1", revision: 1, sequence: 1, timestamp: "2026-08-16T08:00:00.000Z", source: "lifecycle", level: "info", message: "Harness started" },
      { runId: "run-1", revision: 1, sequence: 2, timestamp: "2026-08-16T08:01:00.000Z", source: "stderr", level: "error", message: "Example failure" },
    ],
    settings: { theme: "system", fixedPort: null },
    compatibility: { controllerVersion: "0.1.0", distributionVersion: "0.1.0", nodeVersion: "24.19.0", pnpmVersion: "11.7.0", dshVersion: "0.1.0-rc.6", target: "x86_64-pc-windows-msvc", verified: true },
    loggingFault: null,
    ...overrides,
  };
}

function createBridge(initial: Snapshot) {
  let listener: ((snapshot: Snapshot) => void) | undefined;
  const bridge: SupervisorBridge = {
    getSnapshot: vi.fn().mockResolvedValue(initial),
    subscribe: vi.fn().mockImplementation(async (next: (snapshot: Snapshot) => void) => {
      listener = next;
      return () => { listener = undefined; };
    }),
    startHarness: vi.fn().mockResolvedValue(undefined),
    stopHarness: vi.fn().mockResolvedValue(undefined),
    runDoctor: vi.fn().mockResolvedValue(undefined),
    openHarness: vi.fn().mockResolvedValue(undefined),
    revealLogFolder: vi.fn().mockResolvedValue(undefined),
    setTheme: vi.fn().mockResolvedValue(undefined),
    setFixedPort: vi.fn().mockResolvedValue(undefined),
  };
  return { bridge, publish: (snapshot: Snapshot) => listener?.(snapshot) };
}

async function renderLoaded(snapshot = createSnapshot()) {
  const control = createBridge(snapshot);
  const user = userEvent.setup();
  const clipboardWrite = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: clipboardWrite },
  });
  render(<App bridge={control.bridge} />);
  await screen.findByRole("heading", { name: "Overview" });
  return { ...control, clipboardWrite, user };
}

describe("DSH Supervisor control surface", () => {
  it("renders lifecycle health and safe primary action from a snapshot", async () => {
    await renderLoaded(createSnapshot("stopped-graceful"));
    expect(screen.getAllByText("Stopped")[0]).toBeVisible();
    expect(screen.getByRole("button", { name: "Start" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Open Web UI" })).toBeDisabled();
    expect(screen.getByText("Pinned runtime")).toBeVisible();
  });

  it("invokes the typed start action and announces progress", async () => {
    const delayedStart = vi.fn(() => new Promise<void>(() => undefined));
    const { bridge, user } = await renderLoaded();
    bridge.startHarness = delayedStart;
    await user.click(screen.getByRole("button", { name: "Start" }));
    expect(delayedStart).toHaveBeenCalledOnce();
    expect(screen.getByRole("button", { name: "Starting…" })).toBeDisabled();
  });

  it("moves focus to the lifecycle status when a backend transition removes Start", async () => {
    const { bridge, publish, user } = await renderLoaded();
    bridge.startHarness = vi.fn(async () => {
      publish(createSnapshot("preflighting", { revision: 2, reason: "checking runtime" }));
    });
    await user.click(screen.getByRole("button", { name: "Start" }));
    await waitFor(() => expect(screen.getByRole("group", { name: "Harness lifecycle status" })).toHaveFocus());
    expect(screen.queryByRole("button", { name: "Start" })).not.toBeInTheDocument();
  });

  it("requires confirmation before stopping and returns focus to its trigger", async () => {
    const { bridge, user } = await renderLoaded(createSnapshot("running"));
    const stop = screen.getByRole("button", { name: "Stop" });
    await user.click(stop);
    expect(screen.getByRole("dialog", { name: "Quit confirmation" })).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(stop).toHaveFocus());
    expect(bridge.stopHarness).not.toHaveBeenCalled();
  });

  it("stops the harness only after confirmation and moves focus to its lifecycle view", async () => {
    const { bridge, user } = await renderLoaded(createSnapshot("running"));
    await user.click(screen.getByRole("button", { name: "Stop" }));
    await user.click(await screen.findByRole("button", { name: "Stop harness" }));
    expect(bridge.stopHarness).toHaveBeenCalledOnce();
    await waitFor(() => expect(screen.getByRole("group", { name: "Harness lifecycle status" })).toHaveFocus());
  });

  it("keeps lifecycle commands disabled when the backend marks the snapshot busy", async () => {
    await renderLoaded(createSnapshot("running", { busy: true }));
    expect(screen.getByRole("button", { name: "Stop" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Open Web UI" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Run diagnostics" })).toBeDisabled();
  });

  it("recovers from a failed action after receiving a newer snapshot", async () => {
    const { bridge, publish, user } = await renderLoaded();
    bridge.startHarness = vi.fn().mockRejectedValue(new Error("runtime verification failed"));
    await user.click(screen.getByRole("button", { name: "Start" }));
    expect(await screen.findByText("runtime verification failed")).toBeVisible();
    publish(createSnapshot("ready", { revision: 2, reason: "runtime verified" }));
    expect((await screen.findAllByText("Ready"))[0]).toBeVisible();
    expect(screen.getByRole("button", { name: "Open Web UI" })).toBeEnabled();
  });

  it("supports keyboard tab activation after roving focus changes the active tab", async () => {
    const { user } = await renderLoaded();
    const activity = screen.getByRole("tab", { name: "Activity" });
    activity.focus();
    await user.keyboard("{Enter}");
    expect(await screen.findByRole("heading", { name: "Activity" })).toBeVisible();
    expect(screen.getByRole("searchbox", { name: "Search logs" })).toBeVisible();
  });

  it("filters, pauses, and copies only visible log events", async () => {
    const { clipboardWrite, user } = await renderLoaded();
    await user.click(screen.getByRole("tab", { name: "Activity" }));
    const search = await screen.findByRole("searchbox", { name: "Search logs" });
    await user.type(search, "failure");
    expect(screen.getByText("Example failure")).toBeVisible();
    expect(screen.queryByText("Harness started")).not.toBeInTheDocument();
    await user.click(screen.getByRole("switch", { name: "Pause updates" }));
    expect(screen.getByText(/updates paused/)).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Copy visible log entries" }));
    await waitFor(() => expect(clipboardWrite).toHaveBeenCalledWith(expect.stringContaining("Example failure")));
  });

  it("shows diagnostic evidence and remediation without inventing a recovery action", async () => {
    await renderLoaded(createSnapshot("failed-start", {
      doctor: {
        ok: false,
        error: "Pinned Node hash did not match.",
        node: "24.19.0",
        platform: "windows",
        architecture: "x86_64",
        dshAvailable: false,
        dshError: "DSH CLI is unavailable.",
        profileReady: false,
        apiKeyConfigured: true,
        baseUrlConfigured: true,
        modelConfigured: true,
        lspEnabled: null,
        telemetryEnabled: null,
      },
    }));
    expect(screen.getAllByText("Pinned Node hash did not match.")[0]).toBeVisible();
    expect(screen.getAllByText("Node")[0]).toBeVisible();
    expect(screen.getByText("Recommended recovery")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Start" })).not.toBeInTheDocument();
  });

  it("validates fixed ports before issuing the only permitted settings command", async () => {
    const { bridge, user } = await renderLoaded();
    await user.click(screen.getByRole("tab", { name: "Settings" }));
    await user.click(screen.getByRole("switch", { name: "Use a fixed port" }));
    const port = screen.getByRole("spinbutton", { name: "Port" });
    await user.type(port, "70000");
    await user.click(screen.getByRole("button", { name: "Save port" }));
    expect(screen.getByRole("alert")).toHaveTextContent("Enter a whole-number port from 1 through 65535.");
    expect(bridge.setFixedPort).not.toHaveBeenCalled();
  });

  it("uses system, light, and dark themes without bypassing the settings bridge", async () => {
    const { bridge, user } = await renderLoaded();
    await user.click(screen.getByRole("tab", { name: "Settings" }));
    const theme = screen.getByRole("combobox", { name: "Theme" });
    await user.click(theme);
    await user.click(await screen.findByRole("option", { name: "Dark" }));
    await user.click(theme);
    await user.click(await screen.findByRole("option", { name: "Light" }));
    await user.click(theme);
    await user.click(await screen.findByRole("option", { name: "System" }));
    expect(bridge.setTheme).toHaveBeenNthCalledWith(1, "dark");
    expect(bridge.setTheme).toHaveBeenNthCalledWith(2, "light");
    expect(bridge.setTheme).toHaveBeenNthCalledWith(3, "system");
  });
  it("provides a keyboard-accessible onboarding review without persisting renderer state", async () => {
    const { user } = await renderLoaded();
    await user.click(screen.getByRole("tab", { name: "Settings" }));
    await user.click(screen.getByRole("button", { name: "Review onboarding" }));
    expect(screen.getByRole("dialog", { name: "Welcome to DSH Supervisor" })).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Welcome to DSH Supervisor" })).not.toBeInTheDocument());
  });

  it("moves focus to Overview when onboarding opens diagnostics", async () => {
    const { user } = await renderLoaded();
    await user.click(screen.getByRole("tab", { name: "Settings" }));
    await user.click(screen.getByRole("button", { name: "Review onboarding" }));
    await user.click(screen.getByRole("button", { name: "Review diagnostics" }));
    await waitFor(() => expect(screen.getByRole("heading", { name: "Overview" })).toHaveFocus());
  });

  it("announces a completed doctor run after the backend publishes diagnostic evidence", async () => {
    const { bridge, publish, user } = await renderLoaded();
    bridge.runDoctor = vi.fn(async () => {
      publish(createSnapshot("stopped", {
        revision: 2,
        reason: "doctor result recorded",
        doctor: {
          ok: true,
          error: null,
          node: "24.19.0",
          platform: "windows",
          architecture: "x86_64",
          dshAvailable: true,
          dshError: null,
          profileReady: true,
          apiKeyConfigured: true,
          baseUrlConfigured: true,
          modelConfigured: true,
          lspEnabled: null,
          telemetryEnabled: null,
        },
      }));
    });
    await user.click(screen.getByRole("button", { name: "Run diagnostics" }));
    expect(await screen.findByText("Passed")).toBeVisible();
    await waitFor(() => expect(screen.getByRole("status", { name: "Supervisor updates" })).toHaveTextContent("Diagnostics completed; review the result above."));
  });

  it("rejects a late lower-revision snapshot and its announcement", async () => {
    let resolveInitial: ((snapshot: Snapshot) => void) | undefined;
    const initial = new Promise<Snapshot>((resolve) => { resolveInitial = resolve; });
    const control = createBridge(createSnapshot("stopped", { revision: 1, reason: "initial state" }));
    control.bridge.getSnapshot = vi.fn(() => initial);
    render(<App bridge={control.bridge} />);
    await waitFor(() => expect(control.bridge.subscribe).toHaveBeenCalledOnce());
    control.publish(createSnapshot("running", { revision: 2, reason: "newer state" }));
    expect(await screen.findByRole("heading", { name: "Overview" })).toBeVisible();
    resolveInitial?.(createSnapshot("stopped", { revision: 1, reason: "stale state" }));
    await waitFor(() => expect(screen.getByRole("status", { name: "Supervisor updates" })).toHaveTextContent("Running. newer state"));
    expect(screen.queryByText("stale state")).not.toBeInTheDocument();
  });

  it("announces state changes from complete snapshots and ignores no browser authority", async () => {
    const { publish } = await renderLoaded();
    publish(createSnapshot("running", { revision: 2, reason: "health check passed" }));
    expect(await screen.findByRole("status", { name: "Supervisor updates" })).toHaveTextContent("Running. health check passed");
  });
});
