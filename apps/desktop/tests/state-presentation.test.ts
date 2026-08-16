import { describe, expect, it } from "vitest";
import { statePresentation, type State } from "../src/app";

// Mirrors the closed lifecycle vocabulary in local://desktop-contracts.md §UI vocabulary.
// Any drift between this table and the shipped mapping is a production defect the test catches.
const EXPECTED: Record<State, { label: string; primaryAction: "start" | "stop" | null; openWebUi: boolean }> = {
  stopped: { label: "Stopped", primaryAction: "start", openWebUi: false },
  "stopped-graceful": { label: "Stopped", primaryAction: "start", openWebUi: false },
  preflighting: { label: "Starting", primaryAction: null, openWebUi: false },
  starting: { label: "Starting", primaryAction: null, openWebUi: false },
  ready: { label: "Ready", primaryAction: "stop", openWebUi: true },
  running: { label: "Running", primaryAction: "stop", openWebUi: true },
  stopping: { label: "Stopping", primaryAction: null, openWebUi: false },
  "stopped-forced": { label: "Forced stop", primaryAction: "start", openWebUi: false },
  "failed-start": { label: "Needs attention", primaryAction: null, openWebUi: false },
  crashed: { label: "Needs attention", primaryAction: null, openWebUi: false },
  unmanaged: { label: "Needs attention", primaryAction: null, openWebUi: false },
  "update-pending": { label: "Needs attention", primaryAction: null, openWebUi: false },
  updating: { label: "Needs attention", primaryAction: null, openWebUi: false },
};

const ALL_STATES = Object.keys(EXPECTED) as State[];

describe("statePresentation (lifecycle vocabulary)", () => {
  it("covers the complete closed state enum with the exact UI text mapping", () => {
    // 13 states: stopped | preflighting | starting | ready | running | stopping |
    // stopped-graceful | stopped-forced | failed-start | crashed | unmanaged |
    // update-pending | updating
    expect(ALL_STATES).toHaveLength(13);
  });

  it("maps every state to its label, single primary action, and open-UI eligibility", () => {
    for (const state of ALL_STATES) {
      expect(statePresentation(state), `state=${state}`).toEqual(EXPECTED[state]);
    }
  });

  it("enables Open Web UI only for ready and running (validated endpoint)", () => {
    const open = ALL_STATES.filter((state) => statePresentation(state).openWebUi);
    expect(open).toEqual(["ready", "running"]);
  });

  it("offers Start only from safe stopped terminal states", () => {
    const starts = ALL_STATES.filter((state) => statePresentation(state).primaryAction === "start");
    expect(starts.sort()).toEqual(["stopped", "stopped-forced", "stopped-graceful"].sort());
  });

  it("offers Stop only while ready or running", () => {
    const stops = ALL_STATES.filter((state) => statePresentation(state).primaryAction === "stop");
    expect(stops.sort()).toEqual(["ready", "running"].sort());
  });

  it("never offers a primary action from transitional or needs-attention states", () => {
    const disabled = ALL_STATES.filter((state) => statePresentation(state).primaryAction === null);
    expect(disabled.sort()).toEqual(
      [
        "preflighting",
        "starting",
        "stopping",
        "failed-start",
        "crashed",
        "unmanaged",
        "update-pending",
        "updating",
      ].sort(),
    );
  });
});
