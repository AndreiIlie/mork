// Generated from crates/types by cargo xtask web-types.

export type Account = {
  id: string;
  plan: string;
};

export type ActionId = string;

export type AgentKey = string;

export type AgentLight =
  | "working"
  | "asking"
  | "error"
  | "waiting"
  | "idle";

export type AgentList = {
  agent: AgentKey;
  steps: Array<ListStep>;
};

export type AgentRow = {
  parent?: AgentKey;
  agent: AgentKey;
  role: Role;
  topic?: string;
  label: string;
  effort: string;
  light: AgentLight;
  doing: Doing;
  context: [number, number] | null;
  steps: Array<string>;
  setting?: Setting;
  pending?: Setting;
  choices?: Array<ModelChoice>;
  efforts: Array<Effort>;
  turn?: TurnView;
  cache?: CacheState;
};

export type Answer =
  | {
      id: number;
      ok: unknown;
    }
  | {
      id: number;
      err: ErrorBody;
    };

export type AskId = string;

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

export type Author =
  | { is: "operator" }
  | { is: "engine" }
  | { is: "orchestrator" } & {
      project: string;
    }
  | { is: "pa" }
  | { is: "agent" } & {
      label: string;
      role: Role;
    };

export type BoardGroup = {
  name: string;
  count: number;
  cards: Array<BoardRow>;
};

export type BoardRow = {
  id: CardId;
  title: string;
  ran: boolean;
  live: LiveMarks | null;
  status?: string;
  room?: CardId;
};

export type BoardView = {
  auto: boolean;
  project: ProjectId;
  live: BoardGroup;
  merge: MergeQueue;
  areas: Array<BoardGroup>;
  done: BoardGroup;
};

export type BriefView = {
  changes?: Array<string>;
  code_shape?: Array<string>;
  mock?: string;
};

export type BuilderRow = {
  group: QualityGroup;
  counts: FirstPass;
};

export type CacheState =
  | { is: "running" }
  | { is: "idle" } & {
      cold_at_ms: number;
    };

export type CardAbout = {
  preview?: PreviewControl;
  actions?: Array<string>;
  project: ProjectId;
  branch?: string;
  base: string;
  fork?: string;
  pr?: number;
  pr_url?: string;
  run_cost_cents?: number;
  run_cost_unknown?: boolean;
  seat_costs?: Array<SeatCost>;
  delegated?: boolean;
  self_sustained?: boolean;
  switches?: Array<Switch>;
};

export type CardHeader = {
  id: CardId;
  title: string;
  mode: Mode;
  stage: Stage;
  paused: boolean;
  pipeline: Array<StageMark>;
  times: Times;
  facts: Array<Fact>;
  cost_cents: number | null;
  clone: string | null;
  about: CardAbout;
  spec?: PlaceId;
  parent?: CardId;
};

export type CardId = string;

export type CardsLeft = {
  pool: string;
  cards: number;
};

export type ChatTurn = {
  agent: AgentKey;
  after_seq: number;
  running: boolean;
  step_count: number;
  turn: TurnView;
};

export type Choices = {
  ask: string;
  options: Array<string>;
  pick?: number;
  reason?: string;
};

export type Cli =
  | "claude"
  | "codex"
  | "agy";

export type Color =
  | "grey"
  | "green"
  | "yellow"
  | "red";

export type Command =
  | { watch: {
      places: Array<string>;
      tail?: number;
      full?: boolean;
    } }
  | { hook: Record<string, never> }
  | { wakes: Record<string, never> }
  | { history: {
      place: PlaceId;
      before: number;
      count: number;
      turns?: boolean;
    } }
  | { "history.search": {
      words: string;
    } }
  | { "history.journal": {
      card: CardId;
      count: number;
      seq?: number;
      at?: number;
      before?: number;
      kinds?: Array<string>;
    } }
  | { status: {
      jobs?: boolean;
      all?: boolean;
      card?: string;
    } }
  | { usage: Record<string, never> }
  | { quality: Record<string, never> }
  | { routing: Record<string, never> }
  | { "claude.account": {
      name: string;
    } }
  | { projects: Record<string, never> }
  | { lookback: {
      project: ProjectId;
      read?: string;
    } }
  | { build: {
      commit: string;
    } }
  | { pin: {
      commit: string;
    } }
  | { reload: {
      target?: string;
    } }
  | { say: {
      place: PlaceId;
      to?: string;
      text: string;
      images?: Array<string>;
      kind?: Intent;
      delivery?: Delivery;
      choice?: number;
    } }
  | { "message.send-now": {
      place: PlaceId;
      seq: number;
    } }
  | { dm: {
      card: CardId;
      role: Role;
    } }
  | { spawn: {
      card: CardId;
      role: Role;
      task: string;
      model?: string;
      effort?: string;
      cli?: string;
      topic?: string;
    } }
  | { ack: {
      ask: AskId;
    } }
  | { start: {
      card: CardId;
      mode?: Mode;
      visual?: boolean;
      model?: string;
      effort?: string;
      delegated?: boolean;
      "self-sustained"?: boolean;
      "no-planner"?: boolean;
      carries?: Array<CardId>;
    } }
  | { approve: {
      card: CardId;
    } }
  | { mock: {
      card: CardId;
    } }
  | { brief: {
      card: CardId;
      words?: string;
    } }
  | { merge: {
      card: CardId;
    } }
  | { stop: {
      card?: CardId;
      agent?: AgentKey;
    } }
  | { resume: {
      card?: CardId;
      agent?: AgentKey;
    } }
  | { fresh: {
      agent: string;
    } }
  | { pick: {
      agent: string;
      model?: string;
      effort?: string;
      cli?: string;
    } }
  | { close: {
      card: CardId;
      why: string;
    } }
  | { done: {
      card: CardId;
    } }
  | { preview: {
      card: CardId;
      action?: string;
    } }
  | { demo: {
      card: CardId;
    } }
  | { prs: {
      project: ProjectId;
    } }
  | { review: {
      project: ProjectId;
      pr: number;
    } }
  | { "review.merge": {
      project: ProjectId;
      pr: number;
    } }
  | { "review.branch": {
      project: ProjectId;
      branch: string;
    } }
  | { post: {
      card: CardId;
    } }
  | { resolve: {
      card: CardId;
      n: number;
      declined?: boolean;
      dropped?: string;
    } }
  | { answer: {
      card: CardId;
      n: number;
      text: string;
    } }
  | { rereview: {
      card: CardId;
    } }
  | { triage: {
      card: CardId;
      fix?: Array<string>;
      decline?: Array<string>;
      foreman?: Array<string>;
    } }
  | { deliverables: {
      card: CardId;
      names: TextList;
    } }
  | { plan: {
      card: CardId;
      deliverable: Pick;
      tasks: TextList;
    } }
  | { "plan.task": {
      card: CardId;
      deliverable: Pick;
      task: Pick;
    } }
  | { "card.show": {
      card: CardId;
    } }
  | { findings: {
      card: CardId;
    } }
  | { "card.create": {
      project: ProjectId;
      title: string;
      description: string;
      id?: CardId;
    } }
  | { "card.change": {
      card: CardId;
      note?: string;
      decision?: string;
      withdraw?: string;
      title?: string;
      status?: string;
      priority?: string;
      labels?: Array<string>;
      parent?: string;
      acceptance?: string;
      description?: string;
    } }
  | { "card.close": {
      card: CardId;
      as: string;
      note: string;
    } }
  | { actions: Record<string, never> }
  | { "action.retry": {
      action: ActionId;
    } }
  | { "action.drop": {
      action: ActionId;
    } }
  | { "action.done": {
      action: ActionId;
      note: string;
    } }
  | { "test.lease": {
      kind: string;
      command: Array<string>;
    } }
  | { "test.resume": {
      lease: LeaseId;
    } }
  | { "test.done": {
      lease: LeaseId;
      exit: number;
    } }
  | { door: {
      target: string;
      op: string;
      params: unknown;
    } }
  | { "door.once": {
      ask: AskId;
    } }
  | { "door.no": {
      ask: AskId;
    } }
  | { "door.help": {
      target?: string;
    } }
  | { give: {
      ask: AskId;
    } }
  | { opinion: {
      ask: AskId;
    } }
  | { delegate: {
      card: CardId;
      off?: boolean;
      "self-sustained"?: boolean;
    } }
  | { visual: {
      card: CardId;
      off?: boolean;
    } }
  | { auto: {
      project: ProjectId;
      off?: boolean;
    } }
  | { "memory.read": {
      memory: string;
      name: string;
    } }
  | { "memory.search": {
      words: string;
    } }
  | { "memory.write": {
      memory: string;
      name: string;
      description: string;
      text: string;
      new?: boolean;
    } }
  | { "memory.delete": {
      memory: string;
      name: string;
    } }
  | { "memory.stats": {
      memory?: string;
    } }
  | { "notes.add": {
      kind: NewNote;
      text: string;
      file?: TextList;
      title?: string;
      line?: number;
      why?: string;
      project?: ProjectId;
    } }
  | { notes: {
      project: ProjectId;
    } }
  | { "notes.delete": {
      project: ProjectId;
      ids: Array<string>;
    } }
  | { "orchestrator.review": {
      section: ReviewSection;
      target: string;
    } }
  | { "debug.sleep": {
      ms: number;
    } };

export type CommandView = {
  text: string;
  cut?: boolean;
};

export type ComposerAction =
  | { action: "help" } & {
      commands: Array<[string, string]>;
    }
  | { action: "open" } & {
      card: string;
    }
  | { action: "usage" };

export type ComposerDraft = {
  context: Context;
  text: string;
  images: Array<string>;
  now: boolean;
};

export type ComposerRequest = {
  id: number;
  composer: ComposerDraft;
};

export type Context = {
  place: string;
  card: string | null;
  to: string;
};

export type ContextView = {
  agent: AgentKey;
  used: number;
  window: number | null;
  model: string | null;
};

export type DayBurn = {
  day_ms: number;
  cents: number;
  unknown: boolean;
  landed: number;
};

export type Deliverable = {
  name: string;
  tasks: Array<string>;
  current: number;
};

export type Delivery =
  | "queued"
  | "now";

export type Detail = {
  id: string;
  kind: DetailKind;
  text: string;
  cut?: boolean;
  sub?: boolean;
};

export type DetailKind =
  | "text"
  | "thinking";

export type Doing =
  | { is: "working" } & {
      step: string;
    }
  | { is: "waiting-for-slot" }
  | { is: "waiting-on-you" }
  | { is: "error" } & {
      words: string;
    }
  | { is: "idle" };

export type Effort =
  | "low"
  | "medium"
  | "high"
  | "xhigh"
  | "max";

export type ErrorBody = {
  code: ErrorCode;
  message: string;
};

export type ErrorCode =
  | "invalid"
  | "refused"
  | "buzz"
  | "leak"
  | "internals"
  | "recovering"
  | "reloading"
  | "replaced"
  | "too_large";

export type Fact =
  | { fact: "head" } & {
      head: string;
    }
  | { fact: "join-owed" } & {
      base: string;
      tip: string;
      asked: boolean;
    }
  | { fact: "base-ahead" } & {
      base: string;
      commits: number;
    }
  | { fact: "overlap" } & {
      card: CardId;
      files: number;
    }
  | { fact: "mode" } & {
      mode: Mode;
    }
  | { fact: "run" } & {
      run: number;
    }
  | { fact: "carries" } & {
      cards: Array<CardId>;
    };

export type FailureRate = {
  turns: number;
  calls: number;
  failed: number;
  percent: number | null;
};

export type FileRow = {
  name: string;
  description: string;
  reads: number;
  last_read_ms: number | null;
};

export type Finding = {
  file: string;
  line: number;
  summary: string;
  proof: string;
};

export type FindingRow = {
  group: QualityGroup;
  lens: string;
  counts: Precision;
};

export type FindingState =
  | "open"
  | "resolved"
  | "declined"
  | "dropped";

export type FindingView = {
  n: number;
  round: number;
  by: string;
  file: string;
  line: number;
  summary: string;
  state: FindingState;
  proof?: string;
};

export type FirstPass = {
  verified: number;
  first_pass: number;
  first_pass_percent: number | null;
  landed: number;
  median_rounds: number | null;
};

export type FoldedReview = {
  card: CardId;
  round: number;
  foreman?: AgentRow;
  reviewers: Array<AgentRow>;
  others?: Array<AgentRow>;
};

export type Frame =
  | HelloLine
  | {
      event: WatchEvent;
    }
  | Answer;

export type HandView = {
  seq: number;
  message_place: PlaceId;
  headline: string;
  ask: AskId;
  place: PlaceId;
  card: CardId | null;
  role: Role | null;
  kind: AskKind;
  color: Color;
  words: string;
  raised_ms: number;
  offers: Array<Offer>;
  given?: boolean;
  design?: boolean;
  cold_at_ms?: number;
  decisions?: Array<string>;
  choices?: Choices;
  brief?: BriefView;
};

export type HandoffFile = {
  path: string;
  bytes: number;
};

export type Hello = {
  build: string;
  dirty: boolean;
  protocol: number;
  state_version: number | null;
  home: string;
};

export type HelloLine = {
  hello: Hello;
};

export type HistoryAnswer = {
  history: HistoryPage;
};

export type HistoryPage = {
  place: PlaceId;
  messages: Array<MessageView>;
  earlier: number;
  turns?: Array<ChatTurn>;
};

export type ImageView = {
  path: string;
  url?: string;
};

export type Intent =
  | "request"
  | "response"
  | "update";

export type JobId = string;

export type LastView = {
  date: string;
  ended_ms: number;
  commits: number | null;
};

export type LeaseId = string;

export type LimitView = {
  cli: Cli;
  account: Account | null;
  bucket: string;
  window: string;
  name: string;
  percent: number | null;
  resets_at: number | null;
  reached: boolean;
  seen_ms: number;
  length_ms: number | null;
  source: string;
  age_ms: number;
  unknown: string | null;
};

export type LineKind =
  | "marker"
  | "engine"
  | "operator"
  | "agent";

export type ListStep = {
  id: string;
  words: string;
  status: StepStatus;
};

export type LiveMarks = {
  light: Color | null;
  stage: string;
  working: Array<Role>;
  progress: number;
};

export type LiveRow = {
  name: string;
  topic?: string;
  words: string;
  working?: boolean;
  under?: boolean;
  seat?: boolean;
  start_ms?: number;
  end_ms?: number;
  progress?: SuiteProgress;
};

export type LookbackRead =
  | "pulse"
  | "deep-sweep"
  | "dream";

export type Master =
  | { is: "unknown" }
  | { is: "equal" }
  | { is: "ahead" } & {
      commits: number;
    };

export type MemoryView = {
  tally: Tally;
  reviewed: string | null;
  files: Array<FileRow>;
};

export type MergeQueue = {
  base: string;
  slot: [BoardRow, string] | null;
  waiting: Array<[BoardRow, number]>;
  in_line: number;
};

export type MessageView = {
  pending?: boolean;
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
  findings: Array<Finding>;
  images?: Array<ImageView>;
  open_ask?: AskId;
  choice?: number;
};

export type MessagesView = {
  place: PlaceId;
  revision: number;
  messages: Array<MessageView>;
  earlier: number;
  pinned?: Array<MessageView>;
};

export type Mode =
  | "full"
  | "light"
  | "report"
  | "review";

export type ModelCache = {
  cli: string;
  model: string;
  cache_hit_percent?: number;
};

export type ModelChoice = {
  cli: Cli;
  model: string;
  label: string;
};

export type NewNote =
  | "feedback"
  | "lesson"
  | "smell"
  | "proposal";

export type Note = {
  id: number;
  project: ProjectId | null;
  card: CardId | null;
  seat: AgentKey | null;
  time: number;
} & NoteWhat;

export type NoteWhat =
  | { kind: "survey" } & {
      from: string;
      text: string;
    }
  | { kind: "smell" } & {
      file: string;
      line: number;
      what: string;
      why: string;
    }
  | { kind: "feedback" } & {
      text: string;
    }
  | { kind: "lesson" } & {
      text: string;
    }
  | { kind: "proposal" } & {
      title: string;
      text: string;
      files: Array<string>;
    }
  | { kind: "handoff" } & {
      run: number;
      dir: string;
      files: Array<HandoffFile>;
      skipped: Array<string>;
    };

export type Offer = {
  label: string;
  command: Command;
};

export type OutputFile = {
  job: JobId;
  agent: AgentKey;
  name: string;
  size: number;
  modified_ms: number;
};

export type OverviewStage = {
  kind: OverviewStageKind;
  label: string;
  at: OverviewStageAt;
  start_ms: number;
  end_ms?: number;
  summary?: string;
  round?: RoundView;
};

export type OverviewStageAt =
  | "passed"
  | "failed"
  | "current";

export type OverviewStageKind =
  | "spec"
  | "build"
  | "verify"
  | "land"
  | "report"
  | "review";

export type OverviewView = {
  stages: Array<OverviewStage>;
  live?: Array<LiveRow>;
};

export type Phase =
  | { is: "ready" }
  | { is: "recovering" } & {
      doing: string;
    }
  | { is: "reloading" } & {
      short: string;
    };

export type Pick =
  | number
  | string;

export type PinLine = {
  pin: string | null;
  master: Master;
  ready: boolean;
  update?: Update;
};

export type PlaceId = string;

export type PlaceKind =
  | { is: "main" }
  | { is: "card" } & {
      card: CardId;
    }
  | { is: "spec" } & {
      card: CardId;
    }
  | { is: "challenge" } & {
      card: CardId;
    }
  | { is: "dm" } & {
      card: CardId;
      role: Role;
    }
  | { is: "child" } & {
      card: CardId;
      role?: Role;
      agent: AgentKey;
    }
  | { is: "other" };

export type PlaceView = {
  unread?: Array<[number, boolean]>;
  decisions?: Array<string>;
  start?: StartDefaults;
  place: PlaceId;
  kind: PlaceKind;
  revision: number;
  card: CardHeader | null;
  agents: Array<AgentRow>;
  messages: Array<MessageView>;
  earlier: number;
  turns?: Array<ChatTurn>;
  pinned?: Array<MessageView>;
  offers: Array<Offer>;
  input_to: string;
  plan?: Plan;
  notes?: Array<string>;
  sessions?: Array<AgentRow>;
  outputs?: Array<OutputFile>;
  lists?: Array<AgentList>;
  summaries?: Array<Summary>;
  review?: FoldedReview;
  triage?: TriageView;
  overview?: OverviewView;
};

export type Plan = {
  deliverables: Array<Deliverable>;
  current: number;
};

export type Precision = {
  findings: number;
  fixed: number;
  declined: number;
  covered: number;
  stalled: number;
  fixed_percent: number | null;
  declined_percent: number | null;
  stalemate_percent: number | null;
};

export type PreviewControl = {
  on: boolean;
  url: string;
  enabled: boolean;
};

export type ProjectId = string;

export type ProjectNotes = {
  project: ProjectId;
  notes: Array<Note>;
};

export type ProjectView = {
  project: ProjectId;
  rows: Array<ReadView>;
};

export type Quality = {
  since_ms: number;
  builders: Array<BuilderRow>;
  findings: Array<FindingRow>;
  spend: Array<SpendRow>;
  tools: Array<ToolRow>;
};

export type QualityAnswer = {
  quality: Quality;
};

export type QualityGroup = {
  project: string;
  role: string;
  model: string;
  effort: string;
};

export type QualitySpend = {
  cards: number;
  average_dollars: number;
  unknown: boolean;
};

export type Ram = {
  used_mib: number;
  total_mib: number;
};

export type ReadPoint = {
  seq: number;
  step: number;
};

export type ReadView = {
  mode: Trigger;
  read: LookbackRead;
  last: LastView | null;
  running: CardId | null;
};

export type RecentBurn = {
  since_ms: number;
  total_cents: number;
  unknown: boolean;
  roles: Array<RoleBurn>;
  ended_cards: number;
  average_cents?: number;
  cache_by_model: Array<ModelCache>;
  cards_left?: CardsLeft;
  daily?: Array<DayBurn>;
};

export type ReplyHand = {
  color: Color;
  headline: string;
  message: string;
  ask: boolean;
  design?: boolean;
  withdraw_ask?: boolean;
  images?: Array<string>;
  options?: Array<string>;
  pick?: number;
  reason?: string;
  changes?: Array<string>;
  code_shape?: Array<string>;
};

export type Request = {
  id: number;
  cmd: unknown;
  as?: string;
};

export type Resources = {
  cpu_percent: number | null;
  ram: Ram | null;
};

export type ReviewSection =
  | "memories"
  | "notes";

export type ReviewerCounts = {
  reported: number;
  kept?: number;
};

export type Role =
  | "planner"
  | "foreman"
  | "reviewer"
  | "researcher"
  | "baseline"
  | "echo";

export type RoleBurn = {
  role: string;
  cents: number;
  unknown: boolean;
};

export type RoomRow = {
  place: PlaceId;
  kind: PlaceKind;
  light: Color | null;
  stage: string;
  paused: boolean;
  squares: Array<AgentLight>;
  topic?: string;
  under?: Under;
};

export type RoundView = {
  n: number;
  commit: string;
  verdict: string;
  findings: Array<FindingView>;
  reviewers?: Record<string, ReviewerCounts>;
};

export type Screen = {
  revision: number;
  now_ms: number;
  tz_offset_min: number;
  top: TopLine;
  rooms: Array<RoomRow>;
  hands: Array<HandView>;
  boards: Array<BoardView>;
  memories: Array<MemoryView>;
  notes?: Array<ProjectNotes>;
  projects?: Array<ProjectView>;
};

export type SeatCost = {
  agent: AgentKey;
  cost_cents: number;
  unknown: boolean;
  tokens: Tokens;
};

export type Setting = {
  model: ModelChoice;
  effort: Effort;
};

export type Share = {
  cli: string;
  window: string;
  percent: number | null;
  reached: boolean;
  stale: boolean;
  reset_ms: number | null;
  ends_percent: number | null;
  signed_out: boolean;
  switch_account?: string;
  read_ago: string | null;
};

export type SpendRow = {
  group: QualityGroup;
  spend: QualitySpend;
};

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

export type StageAt =
  | "passed"
  | "current"
  | "to-come";

export type StageKind =
  | "spec"
  | "build"
  | "verify"
  | "land"
  | "report"
  | "review"
  | "done";

export type StageMark = {
  kind: StageKind;
  label: string;
  at: StageAt;
  runs: number;
  count?: string;
  sub?: string;
};

export type StartDefaults = {
  level: number;
  from: string;
  visual: boolean;
  carries: Array<CardId>;
  modes: StartModes;
};

export type StartModes = {
  full: boolean;
  report: boolean;
};

export type StepStatus =
  | "pending"
  | "in-progress"
  | "completed";

export type StepView = {
  id: string;
  words: string;
  command?: CommandView;
  details?: Array<Detail>;
  start_ms?: number;
  end_ms?: number;
  thought_ms?: number;
};

export type SuiteProgress = {
  passed: number;
  failed: number;
  total?: number;
  files?: boolean;
  latest?: string;
};

export type Summary = {
  card: CardId;
  title: string;
  text: string;
};

export type Switch = {
  name: string;
  position: number;
  positions: Array<SwitchPosition>;
};

export type SwitchPosition = {
  label: string;
  command?: Command;
};

export type Tally = {
  memory: string;
  written: number;
  read: number;
  deleted: number;
  reviewed_ms: number | null;
};

export type TextList =
  | string
  | Array<string>;

export type Times = {
  stages: Array<[string, number]>;
  waiting_on_you: number;
  paused: number;
};

export type Tokens = {
  input: number;
  cache_read: number;
  cache_write: number;
  output: number;
};

export type ToolRow = {
  group: QualityGroup;
  counts: FailureRate;
};

export type TopLine = {
  pin: PinLine;
  usage: Array<Share>;
  phase: Phase;
  resources?: Resources;
};

export type TriageFindingView = {
  n: number;
  by: string;
  file: string;
  line: number;
  summary: string;
  proof: string;
};

export type TriageView = {
  ask: AskId;
  review: CardId;
  round: number;
  stalemate?: boolean;
  findings: Array<TriageFindingView>;
};

export type Trigger =
  | "manual"
  | "scheduled"
  | "auto";

export type TurnView = {
  job: JobId;
  steps: Array<StepView>;
  read?: Array<ReadPoint>;
  after?: Array<Detail>;
  cut?: boolean;
  thought_ms?: number;
  thinking_since?: number;
};

export type Under = {
  project: string;
  card: CardId;
  review?: boolean;
  detail?: string;
  mode?: Mode;
};

export type Update =
  | { is: "preparing" } & {
      short: string;
    }
  | { is: "build-failed" } & {
      short: string;
      retry: Command;
    }
  | { is: "not-pinned" } & {
      short: string;
      retry: Command;
    };

export type UsageAnswer = {
  usage: UsageView;
};

export type UsageView = {
  by_pool: Array<string>;
  limits: Array<LimitView>;
  contexts: Array<ContextView>;
  recent_burn: RecentBurn;
};

export type WatchEvent =
  | { screen: Screen }
  | { view: WatchedPlace };

export type WatchedPlace =
  | PlaceView
  | MessagesView;
