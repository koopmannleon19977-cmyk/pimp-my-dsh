export const SUPERVISOR_PROTOCOL_VERSION = 1 as const;

export type LifecycleState =
  | "stopped"
  | "preflighting"
  | "starting"
  | "ready"
  | "running"
  | "stopping"
  | "stopped-graceful"
  | "stopped-forced"
  | "failed-start"
  | "crashed"
  | "unmanaged"
  | "update-pending"
  | "updating";

export type ThemePreference = "system" | "light" | "dark";
export type RestartPolicy = "never" | "always";
export type LogLevel = "trace" | "info" | "warning" | "error";
export type LogSource = "supervisor" | "stdout" | "stderr" | "lifecycle" | "doctor";

export interface HealthCheck {
  readonly id: string;
  readonly status: "ok" | "warning" | "error";
  readonly message: string;
}

export type RunOutcome = "graceful" | "forced" | "crashed" | "failed-start";

export interface RecentRun {
  readonly runId: string;
  readonly startedAt: string;
  readonly endedAt: string;
  readonly outcome: RunOutcome;
  readonly reason: string;
}

export interface DoctorResult {
  readonly ok: boolean;
  readonly error: string | null;
  readonly node: string | null;
  readonly platform: string | null;
  readonly architecture: string | null;
  readonly dshAvailable: boolean | null;
  readonly dshError: string | null;
  readonly profileReady: boolean | null;
  readonly apiKeyConfigured: boolean | null;
  readonly baseUrlConfigured: boolean | null;
  readonly modelConfigured: boolean | null;
  readonly lspEnabled: boolean | null;
  readonly telemetryEnabled: boolean | null;
}

export interface LogEvent {
  readonly runId: string | null;
  readonly revision: number;
  readonly sequence: number;
  readonly timestamp: string;
  readonly source: LogSource;
  readonly level: LogLevel;
  readonly message: string;
}

export interface Settings {
  readonly theme: ThemePreference;
  readonly fixedPort: number | null;
  readonly restartPolicy: RestartPolicy;
}

export interface CompatibilityView {
  readonly controllerVersion: string;
  readonly distributionVersion: string;
  readonly dshVersion: string;
  readonly nodeVersion: string;
  readonly pnpmVersion: string;
  readonly target: string;
  readonly verified: boolean;
}

export interface Snapshot {
  readonly protocolVersion: typeof SUPERVISOR_PROTOCOL_VERSION;
  readonly revision: number;
  readonly state: LifecycleState;
  readonly reason: string;
  readonly runId: string | null;
  readonly endpoint: string | null;
  readonly profile: "web";
  readonly uptimeMs: number | null;
  readonly lastTransitionAt: string;
  readonly busy: boolean;
  readonly health: readonly HealthCheck[];
  readonly recentRuns: readonly RecentRun[];
  readonly doctor: DoctorResult | null;
  readonly logs: readonly LogEvent[];
  readonly settings: Settings;
  readonly compatibility: CompatibilityView;
  readonly loggingFault: string | null;
}
