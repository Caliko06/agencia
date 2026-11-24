import { useCallback, useEffect, useState } from "react"

export type Status = "working" | "waiting" | "idle" | "done" | "failed" | "asks" | "queued" | "stalled"
export type LiveStatus = "connecting" | "sse" | "poll" | "off" | "sleep"
export type Kind = "claude" | "codex" | "bec" | "agy"

export interface BusPeer {
  peer_id: string
  provider: string
  pid?: number
  name: string
  repo?: string
  branch?: string
  cwd?: string
  transport?: string
  endpoint?: string
  title?: string
  status?: string
  push_ready?: boolean
  last_seen?: number
}

export interface BusThread {
  thread_id: string
  topic: string
  created_by?: string
  created_at?: number
  hops_left: number
  max_hops: number
  status: "open" | "done" | "exhausted" | string
}

export interface BusMessage {
  msg_id?: string | number
  thread_id: string
  seq: number
  from_peer: string
  to_peer: string
  body: string
  expect_reply?: boolean
  delivered_via?: string
  delivered_ok?: boolean
  delivered_info?: string
  created_at?: number
  read_at?: number | null
}

export interface BusState {
  peers: BusPeer[]
  threads: BusThread[]
  messages: BusMessage[]
}
export interface Usage {
  in: number
  out: number
  cache: number
  turns: number
  usd?: number
  hist?: [number, number][]
}
export type Verdict = "sirve" | "no" | "sin evidencia"
export interface Audit {
  v: Verdict
  claim: "" | "coincide" | "exagera" | "se queda corto"
  why: string
  t: number
  shot: string
}
export interface WLine {
  who: string
  state: string
  line: string
  ev: string
  t: number
}
export interface Task {
  key: string
  who: string
  kind: string
  title: string
  line: string
  ms: string
  done_when: string
  done: string
  last: WLine | null
  audit: Audit | null
  vote: boolean | null
}
export interface Goal {
  id: string
  state: "VERIFIED" | "FAILS" | "UNVERIFIED"
  done_when: string
  sees: string
  ev: string
  gate?: Task
}
export interface XVerdict {
  v: "APPROVE" | "REFUTE" | "GRADE"
  grade?: number
  why?: string
  missing?: string
  t?: number
}
export interface Claim {
  id: string
  claim: string
  need: string
  verdict: XVerdict | null
}
export interface Front {
  id: string
  who: string
  scope: string
  out: string
  done: boolean | null
}
export interface ClaimBoard {
  file: string
  title: string
  why: string
  fronts: Front[]
  claims: Claim[]
  summaries?: { head: string; body: string }[]
  updated: number
}
export interface Run {
  n?: number
  ok: number
  of: number
  start: string
  bad: string
  why: string
  t: number
  live: boolean
  shot: string
  at: string
}
export interface Sweep {
  ok: number
  total: number
  chapters: Run[]
  auto: (Run & { dir: string })[]
  census: { runs: number; last: string; tsv: boolean; md: boolean } | null
}
export interface Avance {
  boards?: ClaimBoard[]
  sweep?: Sweep | null
  goal: Goal[]
  cur: string
  tasks: Task[]
  agree: number
  votes: number
  updated: number
  busy: boolean
  pending: boolean
  err: string
  runs: number
  orq: string
}
export type Meter = { name: string; s?: number; w?: number; sr?: string | number; wr?: string | number }
export type Quota = {
  claude: Meter[]
  codex: Meter[]
  go: { label: string; usd: number; cap: number; pct: number }[]
}
export interface Part {
  t: number
  role: "user" | "assistant"
  kind: "text" | "tool"
  text: string
  name?: string
  arg?: string
  ok?: boolean
  out?: string
}
export interface ActAgent {
  id: string
  title: string
  who: string
  model: string
  account: string
  in: number
  out: number
  updated: number
}
export interface Activity {
  by_model: { model: string; in: number; out: number; cache: number; agents: number }[]
  by_account: { account: string; in: number; out: number; agents: number }[]
  agents: ActAgent[]
}
export interface Step {
  text: string
  ic: string
  n: number
  t: number
  verb?: string
  obj?: string
}
export type Recap = { did: string; next: string }
export interface ReHit {
  t: number
  tool: string
  addrs: string[]
  why: string
}
export interface Shot {
  url: string
  caption: string
  ts: number
}
export interface Card {
  id: string
  kind: "sub" | "cdx" | "bec" | "cla"
  title: string
  who: string
  request: string
  status: Status
  now: string
  steps: Step[]
  errs: number
  updated: number
  model: string
  repo?: string
  cuenta?: string
  reply?: string
  usage?: Usage
  pid?: number
  paused?: boolean
  live?: boolean
  shots?: Shot[]
  shot_count?: number
  recap?: Recap | null
  safe?: string
  re?: ReHit[]
  children?: Card[]
}
export interface Win {
  id: string
  kind: "claude" | "codex" | "opencode" | "agy" | "orphan"
  account: string
  title: string
  repo: string
  cwd: string
  status: Status
  request: string
  now: string
  steps: Step[]
  errs: number
  updated: number
  model: string
  children: Card[]
  re?: ReHit[]
  usage?: Usage
  pid?: number
  paused?: boolean
  linked?: boolean
  shots?: Shot[]
  shot_count?: number
  recap?: Recap | null
  safe?: string
}
export interface Repo {
  name: string
  windows: Win[]
}
export interface State {
  t: number
  repos: Repo[]
  haiku: { used: number; cap: number; asleep?: boolean }
  quota?: Quota
  avance?: Record<string, Avance>
  bus?: BusState
  homes?: { kind: string; id: string; label: string; desc?: string }[]
}

export const STATUS: Record<Status, string> = {
  working: "Trabajando",
  waiting: "Te espera",
  idle: "En pausa",
  done: "Libre",
  failed: "Falló",
  asks: "Pregunta",
  queued: "En cola",
  stalled: "Sin señal",
}

export const isWork = (x: { status: Status }) => x.status === "working"
export const isFree = (x: { status: Status }) => x.status === "done" || x.status === "failed"
export const usd = (n?: number) => (n ? (n < 0.01 ? "<$0.01" : "$" + n.toFixed(2)) : "")

export function kindOf(x: Card | Win): Kind {
  if ("children" in x) {
    return x.kind === "codex"
      ? "codex"
      : x.kind === "opencode"
        ? "bec"
        : x.kind === "agy"
          ? "agy"
          : "claude"
  }
  return x.kind === "bec"
    ? "bec"
    : x.cuenta === "antigravity"
      ? "agy"
      : x.kind === "cdx" || /codex/i.test(x.who + (x.cuenta ?? ""))
        ? "codex"
        : "claude"
}

export const groupOf = (c: Card) =>
  c.kind === "bec"
    ? "bec"
    : c.cuenta === "antigravity"
      ? "agy"
      : c.kind === "cdx" || /^codex/i.test(c.who)
        ? "codex"
        : "claude"

type Runtime = "codex" | "claude" | "opencode" | "agy"
const RUNTIME: Record<string, Runtime> = {
  codex: "codex",
  cdx: "codex",
  cla: "claude",
  claude: "claude",
  opencode: "opencode",
  bec: "opencode",
  agy: "agy",
}

export function workingBy(repos: Repo[]) {
  const n: Record<Runtime, number> = { codex: 0, claude: 0, opencode: 0, agy: 0 }
  const walk = (x: Win | Card, parent?: Runtime) => {
    const rt = x.kind === "sub" ? parent : RUNTIME[x.kind]
    if (rt && isWork(x)) n[rt]++
    x.children?.forEach((c) => walk(c, rt))
  }
  repos.forEach((r) => r.windows.forEach((w) => walk(w)))
  return n
}

const EMPTY_BUS: BusState = { peers: [], threads: [], messages: [] }

const normalizeBus = (b?: Partial<BusState> | null): BusState => ({
  peers: Array.isArray(b?.peers) ? b.peers : [],
  threads: Array.isArray(b?.threads) ? b.threads : [],
  messages: Array.isArray(b?.messages) ? b.messages : [],
})

const rank = new Map<string, number>()
const stable = <T,>(xs: T[], key: (x: T) => string) => {
  for (const x of xs) if (!rank.has(key(x))) rank.set(key(x), rank.size)
  return [...xs].sort((a, b) => rank.get(key(a))! - rank.get(key(b))!)
}
const settle = (s: State): State => ({
  ...s,
  repos: stable(s.repos ?? [], (r) => "r:" + r.name).map((r) => ({
    ...r,
    windows: stable(r.windows ?? [], (w) => w.id).map((w) => ({
      ...w,
      children: stable(w.children ?? [], (c) => w.id + c.id),
    })),
  })),
  ...(s.bus ? { bus: normalizeBus(s.bus) } : {}),
})

function share<T>(a: unknown, b: T): T {
  if (a === b || typeof a !== "object" || typeof b !== "object" || !a || !b || Array.isArray(a) !== Array.isArray(b)) {
    return b
  }
  if (Array.isArray(b)) {
    const p = a as unknown[]
    const n = b.map((x, i) => share(p[i], x))
    return n.length === p.length && n.every((x, i) => x === p[i]) ? (a as T) : (n as T)
  }
  const p = a as Record<string, unknown>
  const q = b as Record<string, unknown>
  const n: Record<string, unknown> = {}
  let same = Object.keys(p).length === Object.keys(q).length
  for (const k of Object.keys(q)) {
    n[k] = share(p[k], q[k])
    if (n[k] !== p[k]) same = false
  }
  return same ? (a as T) : (n as T)
}
const merge = (prev: State | null, raw: State) => share(prev, settle(raw))

const NAP_MS = 10 * 60_000
const OFFLINE_GRACE_MS = 3000
export const page = { asleep: false }

export function useAgencia(demoState: State | null) {
  const [state, setState] = useState<State | null>(null)
  const [live, setLive] = useState<LiveStatus>("connecting")

  useEffect(() => {
    if (demoState) return
    let es: EventSource | null = null
    let timer = 0
    let retry = 0
    let grace = 0
    let delay = 1
    let dead = false
    let sseActive = false

    const clearGrace = () => {
      if (grace) {
        clearTimeout(grace)
        grace = 0
      }
    }

    const scheduleOfflineGrace = () => {
      if (grace || dead || page.asleep) return
      grace = window.setTimeout(() => {
        grace = 0
        if (!dead && !page.asleep && !sseActive) setLive("off")
      }, OFFLINE_GRACE_MS)
    }

    const markHealthy = (mode: "sse" | "poll") => {
      clearGrace()
      if (!dead) setLive(mode)
    }

    const poll = async () => {
      try {
        const r = await fetch("/api/state", { cache: "no-store" })
        if (!r.ok) throw new Error(`HTTP ${r.status}`)
        const j: State = await r.json()
        if (dead) return
        setState((p) => merge(p, j))
        markHealthy(sseActive ? "sse" : "poll")
      } catch {
        if (!dead && !sseActive) scheduleOfflineGrace()
      }
    }

    const startPoll = () => {
      if (timer) return
      void poll()
      timer = window.setInterval(() => !document.hidden && void poll(), 3000)
    }

    const reconnect = () => {
      sseActive = false
      scheduleOfflineGrace()
      startPoll()
      if (retry || page.asleep) return
      retry = window.setTimeout(() => {
        retry = 0
        start()
      }, delay * 1000)
      delay = Math.min(delay * 2, 15)
    }

    const start = () => {
      try {
        es = new EventSource("/api/events")
        es.addEventListener("state", (e) => {
          const j: State = JSON.parse((e as MessageEvent).data)
          clearInterval(timer)
          timer = 0
          delay = 1
          sseActive = true
          // After the first full snapshot the server sends only the top-level keys that changed.
          setState((p) => merge(p, p ? { ...p, ...j } : j))
          markHealthy("sse")
        })
        es.onerror = () => {
          es?.close()
          es = null
          reconnect()
        }
      } catch {
        reconnect()
      }
    }

    let touched = Date.now()
    const touch = () => {
      if (document.hidden) return
      touched = Date.now()
      if (page.asleep) {
        page.asleep = false
        sseActive = false
        setLive("connecting")
        scheduleOfflineGrace()
        void poll()
        start()
      }
    }
    const nap = window.setInterval(() => {
      if (page.asleep || Date.now() - touched < NAP_MS) return
      page.asleep = true
      sseActive = false
      es?.close()
      es = null
      clearInterval(timer)
      timer = 0
      clearTimeout(retry)
      retry = 0
      clearGrace()
      setLive("sleep")
    }, 30_000)

    const events = ["pointermove", "pointerdown", "keydown", "wheel"]
    events.forEach((e) => addEventListener(e, touch, { passive: true }))
    document.addEventListener("visibilitychange", touch)
    scheduleOfflineGrace()
    void poll()
    start()

    return () => {
      dead = true
      es?.close()
      clearInterval(timer)
      clearTimeout(retry)
      clearGrace()
      clearInterval(nap)
      events.forEach((e) => removeEventListener(e, touch))
      document.removeEventListener("visibilitychange", touch)
    }
  }, [demoState])

  if (demoState) {
    return { state: demoState, live: "sse" as LiveStatus }
  }
  return { state, live }
}

export function useBus(stateBus?: BusState, demoMode?: boolean): { bus: BusState; loading: boolean; refresh: () => void } {
  const [res, setRes] = useState<{ bus: BusState; nonce: number }>({ bus: EMPTY_BUS, nonce: -1 })
  const [nonce, setNonce] = useState(0)

  const refresh = useCallback(() => {
    setNonce((n) => n + 1)
  }, [])

  useEffect(() => {
    if (stateBus || demoMode) return
    let dead = false
    fetch("/api/bus", { cache: "no-store" })
      .then((r) => {
        if (!r.ok) throw new Error(`HTTP ${r.status}`)
        return r.json()
      })
      .then((j) => {
        if (!dead) setRes({ bus: normalizeBus(j), nonce })
      })
      .catch(() => {
        if (!dead) setRes({ bus: EMPTY_BUS, nonce })
      })
    return () => {
      dead = true
    }
  }, [stateBus, demoMode, nonce])

  if (stateBus) {
    return { bus: normalizeBus(stateBus), loading: false, refresh }
  }
  if (demoMode) {
    return { bus: EMPTY_BUS, loading: false, refresh }
  }
  return { bus: res.bus, loading: res.nonce !== nonce, refresh }
}

export function useReducedMotion(): boolean {
  const [reduced, setReduced] = useState(() =>
    typeof window !== "undefined" && typeof window.matchMedia === "function"
      ? window.matchMedia("(prefers-reduced-motion: reduce)").matches
      : false,
  )
  useEffect(() => {
    if (typeof window === "undefined" || typeof window.matchMedia !== "function") return
    const mq = window.matchMedia("(prefers-reduced-motion: reduce)")
    const onChange = () => setReduced(mq.matches)
    mq.addEventListener("change", onChange)
    return () => mq.removeEventListener("change", onChange)
  }, [])
  return reduced
}

export function useNow() {
  const [n, setN] = useState(() => Date.now() / 1000)
  useEffect(() => {
    const i = setInterval(() => setN(Date.now() / 1000), 1000)
    return () => clearInterval(i)
  }, [])
  return n
}

export function ago(t: number, now: number) {
  const s = Math.max(0, Math.round(now - t))
  return s < 60 ? `${s}s` : s < 3600 ? `${Math.floor(s / 60)}m` : `${Math.floor(s / 3600)}h`
}

export async function post(path: string, body: unknown): Promise<{ ok: boolean; info?: string; id?: string }> {
  try {
    const r = await fetch(path, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    })
    const j = await r.json().catch(() => ({}))
    return { ok: r.ok, ...j }
  } catch {
    return { ok: false, info: "Sin conexión con el servidor" }
  }
}

export const tok = (n: number) =>
  n >= 1e6 ? (n / 1e6).toFixed(1) + "M" : n >= 1e3 ? Math.round(n / 1e3) + "k" : String(n)
export const used = (u?: Usage) => (u ? u.in + u.out : 0)
