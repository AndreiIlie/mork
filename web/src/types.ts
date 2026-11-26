// Wire shapes the engine sends over `/ws`, hand-written after crates/types/src/view.rs and protocol.rs.
// Fields serde skips when empty are optional here.

export interface Hello {
  build: string;
  dirty: boolean;
  protocol: number;
  state_version: number | null;
  home: string;
}

export type Color = "grey" | "green" | "yellow" | "red";

export type Role = "planner" | "foreman" | "reviewer" | "researcher" | "baseline" | "echo";

export type AskKind =
  | "approve-brief"
  | "answer"
  | "merge"
  | "approve-pr"
  | "done"
  | "stuck"
  | "checks"
  | "blocked"
  | "error"
  | "cut"
  | "held"
  | "alarm"
  | "token"
  | "new-pr"
  | "door"
  | "mock"
  | "triage";

export type Phase = { is: "ready" } | { is: "recovering"; doing: string } | { is: "reloading"; short: string };

export type Master = { is: "unknown" } | { is: "equal" } | { is: "ahead"; commits: number };

export type Update =
  | { is: "preparing"; short: string }
  | { is: "build-failed"; short: string; retry: unknown }
  | { is: "not-pinned"; short: string; retry: unknown };

export interface PinLine {
  pin: string | null;
  master: Master;
  ready?: boolean;
  update?: Update;
}

export interface Share {
  cli: string;
  window: string;
  percent: number | null;
  reached: boolean;
  stale: boolean;
  reset_ms?: number | null;
  ends_percent?: number | null;
  signed_out?: boolean;
  switch_account?: string;
  read_ago?: string | null;
}

export type LookbackRead = "pulse" | "deep-sweep";

export interface LastView {
  date: string;
  ended_ms: number;
  commits: number | null;
}

export interface ReadView {
  read: LookbackRead;
  last: LastView | null;
  running: string | null;
}

export interface ProjectView {
  project: string;
  rows: ReadView[];
}

export interface RoleBurn {
  role: string;
  cents: number;
  unknown: boolean;
}

export interface CardsLeft {
  pool: string;
  cards: number;
}

export interface RecentBurn {
  since_ms: number;
  total_cents: number;
  unknown: boolean;
  roles: RoleBurn[];
  ended_cards: number;
  average_cents?: number;
  cache_by_model: { cli: string; model: string; cache_hit_percent?: number }[];
  cards_left?: CardsLeft;
  /** The last 14 local days, oldest first, for the Usage charts. */
  daily?: DayBurn[];
}

/** One local day's spend and landed cards. */
export interface DayBurn {
  /** The day's local midnight, in epoch ms. */
  day_ms: number;
  cents: number;
  unknown: boolean;
  landed: number;
}

export interface UsageAnswer {
  usage?: {
    recent_burn?: RecentBurn;
  };
}

/** The top line: pin state, live usage pools, phase, and optional machine resources. */
export interface TopLine {
  pin: PinLine;
  usage: Share[];
  phase: Phase;
  resources?: unknown;
}

/** A command the engine would accept now, and the word its button shows. */
export interface Offer {
  label: string;
  command: unknown;
}

export interface HandView {
  ask: string;
  place: string;
  card: string | null;
  role: Role | null;
  kind: AskKind;
  color: Color;
  words: string;
  raised_ms: number;
  offers: Offer[];
  given?: boolean;
  design?: boolean;
  cold_at_ms?: number;
  /** An approve-brief hand's standing decisions on its card, oldest first. */
  decisions?: string[];
  /** Its seat's options and pick, when its hand gave them; `words` holds them composed. */
  choices?: Choices;
  /** An approve-brief hand's typed sketch and a visual card's mock link. */
  brief?: BriefView;
}

export interface Choices {
  ask: string;
  options: string[];
  /** The option the seat picks, by its number from 1. */
  pick?: number;
  reason?: string;
}

export interface BriefView {
  changes?: string[];
  code_shape?: string[];
  mock?: string;
}

export interface LiveMarks {
  light: Color | null;
  stage: string;
  working: Role[];
  progress: number;
}

export interface BoardRow {
  id: string;
  title: string;
  ran: boolean;
  live: LiveMarks | null;
  status?: string;
  room?: string;
}

export interface BoardGroup {
  name: string;
  count: number;
  cards: BoardRow[];
}

export interface MergeQueue {
  base: string;
  slot: [BoardRow, string] | null;
  waiting: [BoardRow, number][];
  in_line: number;
}

export interface BoardView {
  project: string;
  live: BoardGroup;
  merge: MergeQueue;
  areas: BoardGroup[];
  done: BoardGroup;
}

export type AgentLight = "working" | "asking" | "error" | "waiting" | "idle";

export type PlaceKind =
  | { is: "main" }
  | { is: "card"; card: string }
  | { is: "spec"; card: string }
  | { is: "challenge"; card: string }
  | { is: "dm"; card: string; role: Role }
  | { is: "child"; card: string; role?: Role; agent: string }
  | { is: "other" };

/** A place's row; only a foreman's children's are read here, for their working squares. */
export interface RoomRow {
  place: string;
  kind: PlaceKind;
  light: Color | null;
  stage: string;
  paused: boolean;
  squares: AgentLight[];
  topic?: string;
  under?: unknown;
}

export interface Screen {
  revision: number;
  now_ms: number;
  tz_offset_min: number;
  top: TopLine;
  rooms: RoomRow[];
  hands: HandView[];
  boards: BoardView[];
  memories?: unknown[];
  notes?: unknown[];
  projects?: ProjectView[];
}

export interface ErrorBody {
  code: string;
  message: string;
}

export type Mode = "full" | "light" | "report" | "review";

export type Stage =
  | "starting"
  | "brief"
  | "build"
  | "checks"
  | "review"
  | "second"
  | "ready"
  | "merging"
  | "report"
  | "done"
  | "closed";

export interface CardAbout {
  project: string;
  branch?: string;
  base: string;
  fork?: string;
  pr?: number;
  pr_url?: string;
  delegated?: boolean;
  self_sustained?: boolean;
}

export type StageAt = "passed" | "current" | "to-come";

export type StageKind = "spec" | "build" | "verify" | "land" | "report" | "review" | "done";

export interface StageMark {
  kind: StageKind;
  label: string;
  at: StageAt;
  runs: number;
  count?: string;
  sub?: string;
}

export interface CardHeader {
  id: string;
  title: string;
  mode: Mode;
  stage: Stage;
  paused: boolean;
  pipeline: StageMark[];
  about: CardAbout;
  spec?: string;
  parent?: string;
}

export type CacheState = { is: "running" } | { is: "idle"; cold_at_ms: number };

export type Doing =
  | { is: "working"; step: string }
  | { is: "waiting-for-slot" }
  | { is: "waiting-on-you" }
  | { is: "error"; words: string }
  | { is: "idle" };

export interface ModelChoice {
  cli: string;
  model: string;
  label: string;
}

export interface Setting {
  model: ModelChoice;
  effort: string;
}

export interface AgentRow {
  agent: string;
  role: Role;
  label: string;
  effort: string;
  light: AgentLight;
  doing: Doing;
  context: [number, number] | null;
  setting?: Setting;
  pending?: Setting;
  choices?: ModelChoice[];
  efforts?: string[];
  cache?: CacheState;
}

export type Author =
  | { is: "operator" }
  | { is: "engine" }
  | { is: "orchestrator"; project: string }
  | { is: "pa" }
  | { is: "agent"; label: string; role: Role };

export type LineKind = "marker" | "engine" | "operator" | "agent";

export interface ReplyHand {
  color: Color;
  headline: string;
  message: string;
  ask: string;
  design?: boolean;
  withdraw_ask?: boolean;
  images?: string[];
  options?: string[];
  /** The option the seat would pick, by its number from 1; absent for none. */
  pick?: number;
  reason?: string;
  changes?: string[];
  code_shape?: string[];
}

export interface Finding {
  file: string;
  line: number;
  summary: string;
  proof: string;
}

/** A message's image: its path, and where the bridge serves a job's published copy. */
export interface ImageView {
  path: string;
  url?: string;
}

export interface MessageView {
  seq: number;
  time_ms: number;
  from: Author;
  to: string | null;
  kind: LineKind;
  headline: string;
  text: string;
  cut: boolean;
  number: number | null;
  hand: ReplyHand | null;
  findings: Finding[];
  images?: ImageView[];
  ask_open?: boolean;
  /** The option number an answer chose, when it chose one. */
  choice?: number;
}

export interface Deliverable {
  name: string;
  tasks: string[];
  current: number;
}

export interface Plan {
  deliverables: Deliverable[];
  current: number;
}

export type FindingState = "open" | "resolved" | "declined" | "dropped";

export interface FindingView {
  n: number;
  round: number;
  by: string;
  file: string;
  line: number;
  summary: string;
  state: FindingState;
  proof?: string;
}

export interface FoldedReview {
  card: string;
  round: number;
  /** Its foreman, the parent of its reviewers, with its own state. */
  foreman?: AgentRow | null;
  reviewers: AgentRow[];
  /** Its other seats, its foreman's DM among them. */
  others?: AgentRow[];
}

export interface RoundView {
  n: number;
  verdict: string;
  commit: string;
  findings: FindingView[];
}

export interface TriageFindingView {
  pos: number;
  n: number;
  by: string;
  file: string;
  line: number;
  summary: string;
  proof: string;
}

export interface TriageView {
  ask: string;
  round: number;
  stalemate?: boolean;
  findings: TriageFindingView[];
}

export type OverviewStageKind = "spec" | "build" | "verify" | "land" | "report" | "review";

export type OverviewStageAt = "passed" | "failed" | "current";

export interface OverviewStage {
  kind: OverviewStageKind;
  label: string;
  at: OverviewStageAt;
  start_ms: number;
  end_ms?: number;
  summary?: string;
  round?: RoundView;
}

export interface OverviewView {
  stages: OverviewStage[];
  live?: LiveRow[];
}

/** One live row under the current stage: a verification step or a working seat. */
export interface LiveRow {
  name: string;
  topic?: string;
  words: string;
  working?: boolean;
  under?: boolean;
  seat?: boolean;
  start_ms?: number;
  end_ms?: number;
  progress?: SuiteProgress;
}

/** A running suite's counts so far (fac-697): tests, or a Jest suite's test files, with no total. */
export interface SuiteProgress {
  passed: number;
  failed: number;
  total?: number;
  files?: boolean;
  latest?: string;
}

export interface Detail {
  id: string;
  kind: "text" | "thinking";
  text: string;
  cut?: boolean;
  sub?: boolean;
}

export interface StepView {
  id: string;
  words: string;
  start_ms?: number;
  end_ms?: number;
  thought_ms?: number;
  details?: Detail[];
}

export interface TurnView {
  job: string;
  steps: StepView[];
  cut?: boolean;
  thought_ms?: number;
  thinking_since?: number;
  after?: Detail[];
}

export interface ChatTurn {
  agent: string;
  after_seq: number;
  running: boolean;
  step_count: number;
  turn: TurnView;
}

export interface PlaceView {
  place: string;
  kind: PlaceKind;
  revision: number;
  card: CardHeader | null;
  agents: AgentRow[];
  messages: MessageView[];
  earlier: number;
  turns?: ChatTurn[];
  pinned?: MessageView[];
  offers: Offer[];
  input_to: string;
  plan?: Plan;
  sessions?: AgentRow[];
  review?: FoldedReview;
  triage?: TriageView;
  overview?: OverviewView;
}

export interface HistoryPage {
  place: string;
  messages: MessageView[];
  earlier: number;
  turns?: ChatTurn[];
}

/** One line from the engine: its hello, a watched view, or an answer by id. */
export type Frame =
  | { hello: Hello }
  | { event: { screen?: Screen; view?: PlaceView } }
  | { id: number; ok: unknown }
  | { id: number; err: ErrorBody };
