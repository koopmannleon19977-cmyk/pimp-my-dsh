import { useCallback, useEffect, useMemo, useRef, useState, type ReactElement, type ReactNode } from "react";
import {
  Badge,
  Button,
  Card,
  CardFooter,
  CardHeader,
  Dialog,
  DialogActions,
  DialogBody,
  DialogContent,
  DialogSurface,
  DialogTitle,
  Dropdown,
  FluentProvider,
  Input,
  Label,
  MessageBar,
  MessageBarBody,
  MessageBarTitle,
  Option,
  Spinner,
  Switch,
  Tab,
  TabList,
  Text,
  Tooltip,
  webDarkTheme,
  webLightTheme,
} from "@fluentui/react-components";
import {
  BroadActivityFeed24Regular,
  ArrowSync24Regular,
  CheckmarkCircle24Regular,
  Copy24Regular,
  ErrorCircle24Regular,
  FolderOpen24Regular,
  Home24Regular,
  Open24Regular,
  Play24Regular,
  Search24Regular,
  Settings24Regular,
  Stop24Regular,
  Warning24Regular,
} from "@fluentui/react-icons";
import { tauriSupervisorBridge, type SupervisorBridge } from "./bridge";
import type {
  CompatibilityView,
  DoctorResult,
  HealthCheck,
  LifecycleState,
  LogEvent,
  LogLevel,
  LogSource,
  RecentRun,
  Snapshot,
  ThemePreference,
} from "./types";

type View = "overview" | "activity" | "settings";
type PendingAction = "start" | "stop" | "doctor" | "open" | "reveal" | "theme" | "port" | "copy" | null;

type StatePresentation = {
  readonly label: string;
  readonly tone: "success" | "warning" | "danger" | "informative";
  readonly action: "start" | "stop" | null;
};

const stateDetails: Record<LifecycleState, StatePresentation> = {
  stopped: { label: "Stopped", tone: "informative", action: "start" },
  preflighting: { label: "Starting", tone: "warning", action: null },
  starting: { label: "Starting", tone: "warning", action: null },
  ready: { label: "Ready", tone: "success", action: "stop" },
  running: { label: "Running", tone: "success", action: "stop" },
  stopping: { label: "Stopping", tone: "warning", action: null },
  "stopped-graceful": { label: "Stopped", tone: "informative", action: "start" },
  "stopped-forced": { label: "Forced stop", tone: "warning", action: "start" },
  "failed-start": { label: "Needs attention", tone: "danger", action: null },
  crashed: { label: "Needs attention", tone: "danger", action: null },
  unmanaged: { label: "Needs attention", tone: "danger", action: null },
  "update-pending": { label: "Needs attention", tone: "warning", action: null },
  updating: { label: "Needs attention", tone: "warning", action: null },
};

export type State = LifecycleState;

export function statePresentation(state: State): { label: string; primaryAction: "start" | "stop" | null; openWebUi: boolean } {
  const details = stateDetails[state];
  return {
    label: details.label,
    primaryAction: details.action,
    openWebUi: state === "ready" || state === "running",
  };
}

const levelOptions: readonly (LogLevel | "all")[] = ["all", "trace", "info", "warning", "error"];
const sourceOptions: readonly (LogSource | "all")[] = ["all", "supervisor", "stdout", "stderr", "lifecycle", "doctor"];

function useSystemPreferences(): { dark: boolean; reducedMotion: boolean } {
  const [preferences, setPreferences] = useState(() => ({
    dark: window.matchMedia("(prefers-color-scheme: dark)").matches,
    reducedMotion: window.matchMedia("(prefers-reduced-motion: reduce)").matches,
  }));

  useEffect(() => {
    const colorScheme = window.matchMedia("(prefers-color-scheme: dark)");
    const motion = window.matchMedia("(prefers-reduced-motion: reduce)");
    const update = () => setPreferences({ dark: colorScheme.matches, reducedMotion: motion.matches });
    colorScheme.addEventListener("change", update);
    motion.addEventListener("change", update);
    return () => {
      colorScheme.removeEventListener("change", update);
      motion.removeEventListener("change", update);
    };
  }, []);

  return preferences;
}


function formatUptime(uptimeMs: number | null): string {
  if (uptimeMs === null) {
    return "Not running";
  }

  const totalSeconds = Math.floor(uptimeMs / 1_000);
  const hours = Math.floor(totalSeconds / 3_600);
  const minutes = Math.floor((totalSeconds % 3_600) / 60);
  const seconds = totalSeconds % 60;
  return `${hours}h ${minutes}m ${seconds}s`;
}

function formatTimestamp(timestamp: string): string {
  const date = new Date(timestamp);
  return Number.isNaN(date.getTime()) ? timestamp : date.toLocaleTimeString();
}

function statusIcon(tone: StatePresentation["tone"]): ReactElement {
  if (tone === "success") return <CheckmarkCircle24Regular aria-hidden="true" />;
  if (tone === "danger") return <ErrorCircle24Regular aria-hidden="true" />;
  if (tone === "warning") return <Warning24Regular aria-hidden="true" />;
  return <BroadActivityFeed24Regular aria-hidden="true" />;
}

function HealthRow({ check }: { readonly check: HealthCheck }): ReactNode {
  const icon = check.status === "ok" ? <CheckmarkCircle24Regular aria-hidden="true" /> : check.status === "error" ? <ErrorCircle24Regular aria-hidden="true" /> : <Warning24Regular aria-hidden="true" />;
  return (
    <li className="health-row">
      <span className={`health-icon health-${check.status}`}>{icon}</span>
      <span>
        <Text weight="semibold">{check.id}</Text>
        <Text className="secondary-text" block>{check.message}</Text>
      </span>
    </li>
  );
}

function DoctorPanel({ doctor }: { readonly doctor: DoctorResult | null }): ReactNode {
  if (doctor === null) {
    return <Text className="secondary-text">Run a diagnostic check to record compatibility and launch evidence.</Text>;
  }

  const evidence = [
    ["Node", doctor.node],
    ["Platform", doctor.platform],
    ["Architecture", doctor.architecture],
    ["DSH CLI", doctor.dshAvailable === null ? null : doctor.dshAvailable ? "Available" : "Unavailable"],
    ["Profile", doctor.profileReady === null ? null : doctor.profileReady ? "Ready" : "Not ready"],
    ["API key", doctor.apiKeyConfigured === null ? null : doctor.apiKeyConfigured ? "Configured" : "Not configured"],
    ["Base URL", doctor.baseUrlConfigured === null ? null : doctor.baseUrlConfigured ? "Configured" : "Not configured"],
    ["Model", doctor.modelConfigured === null ? null : doctor.modelConfigured ? "Configured" : "Not configured"],
    ["LSP", doctor.lspEnabled === null ? null : doctor.lspEnabled ? "Enabled" : "Disabled"],
    ["Telemetry", doctor.telemetryEnabled === null ? null : doctor.telemetryEnabled ? "Enabled" : "Disabled"],
  ] as const;
  const recovery = doctor.ok ? null : doctor.error ?? doctor.dshError ?? "Resolve the failed diagnostic evidence, then run diagnostics again.";

  return (
    <div className="doctor-result" aria-label="Diagnostic result">
      <Badge appearance="tint" color={doctor.ok ? "success" : "danger"}>{doctor.ok ? "Passed" : "Action needed"}</Badge>
      {doctor.error !== null ? <Text weight="semibold" block>{doctor.error}</Text> : null}
      {doctor.dshError !== null ? <Text className="field-error" block>{doctor.dshError}</Text> : null}
      <dl className="evidence-list">
        {evidence.map(([label, value]) => value === null ? null : <div key={label}><dt>{label}</dt><dd>{value}</dd></div>)}
      </dl>
      {recovery !== null ? <section aria-labelledby="remediation-heading"><Text id="remediation-heading" weight="semibold" block>Recommended recovery</Text><Text className="secondary-text" block>{recovery}</Text></section> : null}
    </div>
  );
}

function CompatibilityPanel({ compatibility }: { readonly compatibility: CompatibilityView }): ReactNode {
  return (
    <dl className="compatibility-list" aria-label="Runtime compatibility">
      <div><dt>Controller</dt><dd>{compatibility.controllerVersion}</dd></div>
      <div><dt>Distribution</dt><dd>{compatibility.distributionVersion}</dd></div>
      <div><dt>Node</dt><dd>{compatibility.nodeVersion}</dd></div>
      <div><dt>pnpm</dt><dd>{compatibility.pnpmVersion}</dd></div>
      <div><dt>DSH</dt><dd>{compatibility.dshVersion}</dd></div>
      <div><dt>Target</dt><dd>{compatibility.target}</dd></div>
    </dl>
  );
}

export function App({ bridge = tauriSupervisorBridge }: { readonly bridge?: SupervisorBridge }): ReactNode {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [view, setView] = useState<View>("overview");
  const [pending, setPending] = useState<PendingAction>(null);
  const [error, setError] = useState<string | null>(null);
  const [announcement, setAnnouncement] = useState("Loading supervisor state.");
  const [logLevel, setLogLevel] = useState<LogLevel | "all">("all");
  const [logSource, setLogSource] = useState<LogSource | "all">("all");
  const [query, setQuery] = useState("");
  const [paused, setPaused] = useState(false);
  const [frozenLogs, setFrozenLogs] = useState<readonly LogEvent[]>([]);
  const [quitConfirmation, setQuitConfirmation] = useState(false);
  const [onboardingOpen, setOnboardingOpen] = useState(false);
  const stopButtonRef = useRef<HTMLSpanElement>(null);
  const onboardingButtonRef = useRef<HTMLSpanElement>(null);
  const overviewHeadingRef = useRef<HTMLSpanElement>(null);
  const lifecycleStatusRef = useRef<HTMLDivElement>(null);
  const latestRevision = useRef<number | null>(null);
  const preferences = useSystemPreferences();

  const acceptSnapshot = useCallback((next: Snapshot) => {
    if (latestRevision.current !== null && next.revision < latestRevision.current) return;
    latestRevision.current = next.revision;
    setSnapshot(next);
    setAnnouncement(`${stateDetails[next.state].label}. ${next.reason}`);
  }, []);

  useEffect(() => {
    let active = true;
    let unsubscribe: (() => void) | undefined;
    void (async () => {
      try {
        const [removeListener, initial] = await Promise.all([bridge.subscribe(acceptSnapshot), bridge.getSnapshot()]);
        unsubscribe = removeListener;
        if (active) acceptSnapshot(initial);
      } catch (cause) {
        if (active) {
          const message = cause instanceof Error ? cause.message : "The supervisor could not be reached.";
          setError(message);
          setAnnouncement(`Unable to load supervisor state. ${message}`);
        }
      }
    })();
    return () => {
      active = false;
      unsubscribe?.();
    };
  }, [acceptSnapshot, bridge]);

  useEffect(() => {
    if (!paused && snapshot !== null) setFrozenLogs(snapshot.logs);
  }, [paused, snapshot]);

  const displayedLogs = paused ? frozenLogs : snapshot?.logs ?? [];
  const filteredLogs = useMemo(() => {
    const normalizedQuery = query.trim().toLocaleLowerCase();
    return displayedLogs.filter((event) =>
      (logLevel === "all" || event.level === logLevel)
      && (logSource === "all" || event.source === logSource)
      && (normalizedQuery.length === 0 || event.message.toLocaleLowerCase().includes(normalizedQuery)),
    );
  }, [displayedLogs, logLevel, logSource, query]);

  const theme = snapshot?.settings.theme ?? "system";
  const isDark = theme === "dark" || (theme === "system" && preferences.dark);

  useEffect(() => {
    document.documentElement.style.colorScheme = isDark ? "dark" : "light";
  }, [isDark]);


  const run = useCallback(async (action: Exclude<PendingAction, null>, operation: () => Promise<void>, success: string) => {
    setPending(action);
    setError(null);
    try {
      await operation();
      setAnnouncement(success);
    } catch (cause) {
      const message = cause instanceof Error ? cause.message : "The requested operation failed.";
      setError(message);
      setAnnouncement(`Operation failed. ${message}`);
    } finally {
      setPending(null);
    }
  }, []);

  const setTheme = useCallback((nextTheme: ThemePreference) => {
    void run("theme", () => bridge.setTheme(nextTheme), `Theme preference set to ${nextTheme}.`);
  }, [bridge, run]);

  const copyLogs = useCallback(() => {
    if (filteredLogs.length === 0 || !navigator.clipboard?.writeText) {
      setAnnouncement("No visible logs are available to copy.");
      return;
    }
    const content = filteredLogs.map((event) => `${event.timestamp} ${event.level.toUpperCase()} ${event.source}: ${event.message}`).join("\n");
    void run("copy", () => navigator.clipboard.writeText(content), "Visible logs copied to the clipboard.");
  }, [filteredLogs, run]);

  const closeConfirmation = useCallback((restoreFocus = true) => {
    setQuitConfirmation(false);
    if (restoreFocus) {
      window.setTimeout(() => stopButtonRef.current?.querySelector<HTMLButtonElement>("button")?.focus(), 0);
    }
  }, []);

  const closeOnboarding = useCallback((restoreFocus = true) => {
    setOnboardingOpen(false);
    if (restoreFocus) {
      window.setTimeout(() => onboardingButtonRef.current?.querySelector<HTMLButtonElement>("button")?.focus(), 0);
    }
  }, []);

  const focusLifecycleStatus = useCallback(() => {
    window.setTimeout(() => lifecycleStatusRef.current?.focus(), 0);
  }, []);

  const focusOverviewHeading = useCallback(() => {
    window.setTimeout(() => overviewHeadingRef.current?.querySelector<HTMLElement>("[tabindex]")?.focus(), 0);
  }, []);

  if (snapshot === null) {
    return (
      <FluentProvider theme={isDark ? webDarkTheme : webLightTheme} className="app-shell" data-reduced-motion={preferences.reducedMotion ? "true" : "false"}>
        <main className="loading-view" aria-busy="true"><Spinner label="Loading supervisor state" /></main>
        <div className="live-region" role="status" aria-label="Supervisor updates" aria-live="polite">{announcement}</div>
        {error !== null ? <MessageBar intent="error"><MessageBarBody><MessageBarTitle>Connection unavailable</MessageBarTitle>{error}</MessageBarBody></MessageBar> : null}
      </FluentProvider>
    );
  }

  const presentation = stateDetails[snapshot.state];
  const lifecycleBusy = snapshot.busy || pending === "start" || pending === "stop";
  const primaryDisabled = lifecycleBusy || presentation.action === null;
  const canOpen = (snapshot.state === "ready" || snapshot.state === "running") && snapshot.endpoint !== null && !snapshot.busy && pending === null;

  return (
    <FluentProvider theme={isDark ? webDarkTheme : webLightTheme} className="app-shell" data-reduced-motion={preferences.reducedMotion ? "true" : "false"}>
      <a className="skip-link" href="#main-content">Skip to main content</a>
      <header className="topbar">
        <div><Text as="h1" size={500} weight="semibold">DSH Supervisor</Text><Text className="secondary-text" block>Local web profile</Text></div>
        <Badge appearance="tint" color={presentation.tone} icon={statusIcon(presentation.tone)}>{presentation.label}</Badge>
      </header>
      <div className="app-layout">
        <nav aria-label="Supervisor sections" className="sidebar">
          <TabList selectedValue={view} onTabSelect={(_, data) => setView(data.value as View)} vertical>
            <Tab id="overview-tab" value="overview" aria-controls="overview-panel" icon={<Home24Regular aria-hidden="true" />}>Overview</Tab>
            <Tab id="activity-tab" value="activity" aria-controls="activity-panel" icon={<BroadActivityFeed24Regular aria-hidden="true" />}>Activity</Tab>
            <Tab id="settings-tab" value="settings" aria-controls="settings-panel" icon={<Settings24Regular aria-hidden="true" />}>Settings</Tab>
          </TabList>
        </nav>
        <main id="main-content" className="main-content" tabIndex={-1}>
          {error !== null ? <MessageBar intent="error" className="message"><MessageBarBody><MessageBarTitle>Action failed</MessageBarTitle>{error}</MessageBarBody></MessageBar> : null}
          {view === "overview" ? (
            <section id="overview-panel" role="tabpanel" aria-labelledby="overview-tab" className="content-stack">
              <div className="section-heading"><div><span ref={overviewHeadingRef}><Text as="h2" id="overview-heading" size={600} weight="semibold" tabIndex={-1}>Overview</Text></span><Text className="secondary-text" block>Supervise the local DSH web harness.</Text></div></div>
              <Card><CardHeader header={<Text weight="semibold">Harness lifecycle</Text>} description={<Text className="secondary-text">{snapshot.reason}</Text>} />
                <div ref={lifecycleStatusRef} className="lifecycle-summary" role="group" aria-label="Harness lifecycle status" tabIndex={-1}><span className={`state-icon state-${presentation.tone}`}>{statusIcon(presentation.tone)}</span><div><Text size={500} weight="semibold">{presentation.label}</Text><Text className="secondary-text" block>{formatUptime(snapshot.uptimeMs)}</Text></div></div>
                <CardFooter>
                  {presentation.action === "start" ? <Button appearance="primary" icon={<Play24Regular />} disabled={primaryDisabled} onClick={() => { focusLifecycleStatus(); void run("start", () => bridge.startHarness(), "Starting the web harness."); }}>{pending === "start" ? "Starting…" : "Start"}</Button> : null}
                  {presentation.action === "stop" ? <span ref={stopButtonRef}><Button appearance="primary" icon={<Stop24Regular />} disabled={primaryDisabled} onClick={() => setQuitConfirmation(true)}>{pending === "stop" ? "Stopping…" : "Stop"}</Button></span> : null}
                  <Button icon={<Open24Regular />} disabled={!canOpen} onClick={() => void run("open", () => bridge.openHarness(), "Opening the local Web UI.")}>Open Web UI</Button>
                </CardFooter>
              </Card>
              <Card><CardHeader header={<Text weight="semibold">Recent runs</Text>} />
                {snapshot.recentRuns.length > 0 ? (
                  <ul className="health-list">
                    {snapshot.recentRuns.map((run) => (
                      <li key={run.runId}>
                        <Text size={300} weight="semibold">{run.outcome}</Text>
                        <Text className="secondary-text" block>{run.reason}</Text>
                      </li>
                    ))}
                  </ul>
                ) : (
                  <Text className="secondary-text">No completed runs yet.</Text>
                )}
                <CardFooter>
                  {presentation.action === "start" && snapshot.recentRuns.length > 0 ? (
                    <Button appearance="secondary" icon={<Play24Regular />} disabled={primaryDisabled} onClick={() => { focusLifecycleStatus(); void run("start", () => bridge.startHarness(), "Resuming the web harness."); }}>{pending === "start" ? "Resuming…" : "Resume"}</Button>
                  ) : null}
                </CardFooter>
              </Card>
              <div className="two-column-grid">
                <Card><CardHeader header={<Text weight="semibold">Health checks</Text>} />
                  {snapshot.health.length > 0 ? <ul className="health-list">{snapshot.health.map((check) => <HealthRow check={check} key={check.id} />)}</ul> : <Text className="secondary-text">No health checks have been reported yet.</Text>}
                </Card>
                <Card><CardHeader header={<Text weight="semibold">Compatibility</Text>} description={<Text className="secondary-text">{snapshot.compatibility.verified ? "Pinned runtime verified" : "Review runtime requirements"}</Text>} /><CompatibilityPanel compatibility={snapshot.compatibility} /></Card>
              </div>
              <Card><CardHeader header={<Text weight="semibold">Diagnostics</Text>} />
                <DoctorPanel doctor={snapshot.doctor} />
                <CardFooter><Button icon={<ArrowSync24Regular />} disabled={snapshot.busy || pending !== null} onClick={() => void run("doctor", () => bridge.runDoctor(), "Diagnostics completed; review the result above.")}>{pending === "doctor" ? "Checking…" : "Run diagnostics"}</Button></CardFooter>
              </Card>
              {snapshot.loggingFault !== null ? <MessageBar intent="warning"><MessageBarBody><MessageBarTitle>Log persistence unavailable</MessageBarTitle>{snapshot.loggingFault}</MessageBarBody></MessageBar> : null}
            </section>
          ) : null}
          {view === "activity" ? (
            <section id="activity-panel" role="tabpanel" aria-labelledby="activity-tab" className="content-stack">
              <div className="section-heading"><div><Text as="h2" id="activity-heading" size={600} weight="semibold">Activity</Text><Text className="secondary-text" block>Lifecycle and process output from this supervisor session.</Text></div><Button icon={<FolderOpen24Regular />} disabled={pending !== null} onClick={() => void run("reveal", () => bridge.revealLogFolder(), "Opened the log folder.")}>Show log folder</Button></div>
              <Card>
                <div className="log-toolbar">
                  <div><Label htmlFor="log-search">Search logs</Label><Input id="log-search" type="search" value={query} onChange={(_, data) => setQuery(data.value)} contentBefore={<Search24Regular aria-hidden="true" />} /></div>
                  <div><Label htmlFor="log-level">Level</Label><Dropdown id="log-level" value={logLevel} selectedOptions={[logLevel]} onOptionSelect={(_, data) => setLogLevel(data.optionValue as LogLevel | "all")}>{levelOptions.map((level) => <Option key={level} value={level}>{level}</Option>)}</Dropdown></div>
                  <div><Label htmlFor="log-source">Source</Label><Dropdown id="log-source" value={logSource} selectedOptions={[logSource]} onOptionSelect={(_, data) => setLogSource(data.optionValue as LogSource | "all")}>{sourceOptions.map((source) => <Option key={source} value={source}>{source}</Option>)}</Dropdown></div>
                  <Switch label="Pause updates" checked={paused} onChange={(_, data) => setPaused(data.checked)} />
                  <Tooltip content="Copy visible log entries" relationship="description"><Button aria-label="Copy visible log entries" icon={<Copy24Regular />} disabled={pending === "copy" || filteredLogs.length === 0} onClick={copyLogs}>Copy</Button></Tooltip>
                </div>
                <Text className="secondary-text" block aria-live="polite">{filteredLogs.length} visible {filteredLogs.length === 1 ? "entry" : "entries"}{paused ? ", updates paused" : ""}.</Text>
                {filteredLogs.length > 0 ? <ol className="log-list" aria-label="Supervisor logs">{filteredLogs.map((event) => <li key={`${event.revision}:${event.sequence}`}><time dateTime={event.timestamp}>{formatTimestamp(event.timestamp)}</time><Badge appearance="outline" color={event.level === "error" ? "danger" : event.level === "warning" ? "warning" : "informative"}>{event.level}</Badge><Text className="log-source">{event.source}</Text><Text className="log-message">{event.message}</Text></li>)}</ol> : <Text className="empty-state">No entries match the current filters.</Text>}
              </Card>
            </section>
          ) : null}
          {view === "settings" ? (
            <section id="settings-panel" role="tabpanel" aria-labelledby="settings-tab" className="content-stack">
              <div className="section-heading"><div><Text as="h2" id="settings-heading" size={600} weight="semibold">Settings</Text><Text className="secondary-text" block>Preferences are applied by the supervisor.</Text></div></div>
              <Card><CardHeader header={<Text weight="semibold">Appearance</Text>} description={<Text className="secondary-text">Use the system setting or choose a fixed application theme.</Text>} />
                <div className="settings-field"><Label htmlFor="theme-preference">Theme</Label><Dropdown id="theme-preference" value={theme} selectedOptions={[theme]} disabled={pending === "theme"} onOptionSelect={(_, data) => setTheme(data.optionValue as ThemePreference)}><Option value="system">System</Option><Option value="light">Light</Option><Option value="dark">Dark</Option></Dropdown><Text className="secondary-text">High contrast and reduced-motion preferences remain controlled by Windows.</Text></div>
              </Card>
              <Card><CardHeader header={<Text weight="semibold">Network</Text>} description={<Text className="secondary-text">Leave the port automatic unless a local policy requires a fixed value.</Text>} />
                <FixedPortField value={snapshot.settings.fixedPort} disabled={pending === "port" || snapshot.busy} onSave={(port) => void run("port", () => bridge.setFixedPort(port), port === null ? "Automatic port selection restored." : `Fixed port set to ${port}.`)} />
              </Card>
              <Card><CardHeader header={<Text weight="semibold">Onboarding</Text>} description={<Text className="secondary-text">The supervisor controls one local web profile. Start it from Overview, wait for Ready or Running, then open the Web UI.</Text>} /><CardFooter><span ref={onboardingButtonRef}><Button onClick={() => setOnboardingOpen(true)}>Review onboarding</Button></span></CardFooter></Card>
            </section>
          ) : null}
        </main>
      </div>
      {quitConfirmation ? <QuitConfirmation onCancel={closeConfirmation} onConfirm={() => { closeConfirmation(false); focusLifecycleStatus(); void run("stop", () => bridge.stopHarness(), "Stopping the web harness."); }} busy={pending === "stop"} /> : null}
      {onboardingOpen ? <OnboardingDialog onClose={closeOnboarding} onReviewDiagnostics={() => { closeOnboarding(false); setView("overview"); focusOverviewHeading(); }} /> : null}
      <div className="live-region" role="status" aria-label="Supervisor updates" aria-live="polite" aria-atomic="true">{announcement}</div>
    </FluentProvider>
  );
}

function FixedPortField({ value, disabled, onSave }: { readonly value: number | null; readonly disabled: boolean; readonly onSave: (port: number | null) => void }): ReactNode {
  const [mode, setMode] = useState<"automatic" | "fixed">(value === null ? "automatic" : "fixed");
  const [port, setPort] = useState(value?.toString() ?? "");
  const [validation, setValidation] = useState<string | null>(null);

  useEffect(() => {
    setMode(value === null ? "automatic" : "fixed");
    setPort(value?.toString() ?? "");
  }, [value]);

  const save = () => {
    if (mode === "automatic") {
      setValidation(null);
      onSave(null);
      return;
    }
    const parsed = Number(port);
    if (!Number.isInteger(parsed) || parsed < 1 || parsed > 65_535) {
      setValidation("Enter a whole-number port from 1 through 65535.");
      return;
    }
    setValidation(null);
    onSave(parsed);
  };

  return <div className="settings-field"><Switch label="Use a fixed port" checked={mode === "fixed"} disabled={disabled} onChange={(_, data) => setMode(data.checked ? "fixed" : "automatic")} /><Label htmlFor="fixed-port">Port</Label><Input id="fixed-port" type="number" inputMode="numeric" value={port} disabled={disabled || mode === "automatic"} aria-invalid={validation !== null} aria-describedby={validation === null ? undefined : "port-error"} onChange={(_, data) => setPort(data.value)} /><Text id="port-error" role={validation === null ? undefined : "alert"} className="field-error">{validation}</Text><Button disabled={disabled} onClick={save}>Save port</Button></div>;
}

function QuitConfirmation({ onCancel, onConfirm, busy }: { readonly onCancel: () => void; readonly onConfirm: () => void; readonly busy: boolean }): ReactNode {
  return (
    <Dialog open onOpenChange={(_, data) => { if (!data.open) onCancel(); }}>
      <DialogSurface aria-describedby="quit-confirmation-description">
        <DialogBody>
          <DialogTitle>Quit confirmation</DialogTitle>
          <DialogContent id="quit-confirmation-description">Stop the running web harness before closing the supervisor?</DialogContent>
          <DialogActions>
            <Button autoFocus onClick={onCancel}>Cancel</Button>
            <Button appearance="primary" icon={<Stop24Regular />} disabled={busy} onClick={onConfirm}>Stop harness</Button>
          </DialogActions>
        </DialogBody>
      </DialogSurface>
    </Dialog>
  );
}

function OnboardingDialog({ onClose, onReviewDiagnostics }: { readonly onClose: () => void; readonly onReviewDiagnostics: () => void }): ReactNode {
  return (
    <Dialog open onOpenChange={(_, data) => { if (!data.open) onClose(); }}>
      <DialogSurface aria-describedby="onboarding-description">
        <DialogBody>
          <DialogTitle>Welcome to DSH Supervisor</DialogTitle>
          <DialogContent id="onboarding-description">
            This app supervises one local DSH web profile. Start it from Overview, wait for Ready or Running, then use Open Web UI. If the state needs attention, run diagnostics before trying again.
          </DialogContent>
          <DialogActions>
            <Button onClick={onReviewDiagnostics}>Review diagnostics</Button>
            <Button appearance="primary" onClick={onClose}>Continue</Button>
          </DialogActions>
        </DialogBody>
      </DialogSurface>
    </Dialog>
  );
}
