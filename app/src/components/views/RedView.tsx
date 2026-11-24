import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react"
import {
  BaseEdge,
  ControlButton,
  Controls,
  EdgeLabelRenderer,
  Handle,
  Position,
  ReactFlow,
  ReactFlowProvider,
  getBezierPath,
  useNodesState,
  useReactFlow,
  useStore,
  type Edge,
  type EdgeProps,
  type FitViewOptions,
  type Node,
  type NodeChange,
  type NodeProps,
} from "@xyflow/react"
import {
  ArrowRight,
  ArrowUp,
  Check,
  FolderGit2,
  GripVertical,
  History,
  Link2,
  MessageSquare,
  MessageSquarePlus,
  PanelLeft,
  Plus,
  Radio,
  RefreshCw,
  Terminal,
  Undo2,
  Unlink,
  X,
} from "lucide-react"
import { toast } from "sonner"
import { Age, SignalBar, STEP_ICON } from "@/components/bits"
import { Brand } from "@/components/brand"
import { Shots } from "@/components/shots"
import {
  groupOf,
  isFree,
  isWork,
  kindOf,
  post,
  tok,
  useBus,
  useReducedMotion,
  used,
  type BusMessage,
  type BusPeer,
  type BusState,
  type BusThread,
  type Card,
  type Kind,
  type Repo,
  type Usage,
  type Win,
} from "@/lib/agencia"
import { flat, hhmm, isWin, labelOf, lineOf, modelOf, nameOf, subOf, toneOf, type Item } from "@/lib/tone"
import { cn } from "@/lib/utils"

export const RUNTIME_FILTERS = [
  ["all", "Todos"],
  ["claude", "Claude"],
  ["codex", "Codex"],
  ["bec", "OpenCode"],
  ["agy", "Antigravity"],
] as const

const ROWS = 3
const LINE = 26
const liveTone = (it: Item) => toneOf(it) === "work" || toneOf(it) === "need"
const recapOf = (it: Item) => {
  const r = toneOf(it) === "work" ? it.recap : null
  const did = r?.did && !/^nada/i.test(r.did) ? r.did : ""
  return r && (did || r.next) ? { did, next: r.next } : null
}

const hasResponder = (it: Item) => toneOf(it) === "need" && isWin(it) && it.kind === "claude"
const hasMetaRow = (it: Item, repoTag?: string, busCount?: number) =>
  Boolean(repoTag || modelOf(it.model) || (busCount ?? 0) > 0)

const CARD = {
  w: 340,
  hero: 460,
  h: (it: Item, rows: number, repoTag?: string, busCount?: number) =>
    liveTone(it)
      ? 106 +
        (hasMetaRow(it, repoTag, busCount) ? 24 : 0) +
        (hasResponder(it) ? 34 : 0) +
        ((r) => (r ? 6 + 22 * (+!!r.did + +!!r.next) : 0))(recapOf(it)) +
        (rows ? rows * LINE + 14 : 0)
      : 68 + (hasMetaRow(it, repoTag, busCount) ? 24 : 0),
}
const ROOT = { w: 184, h: 38 }
const GAP = 36
const RANK = 58

type NodeData = {
  it: Item
  rows: number
  hero?: boolean
  repoTag?: string
  busCount?: number
  onOpenBus?: () => void
  onDetach?: () => void
}
type RootData = { need: number; work: number }
type LabelData = { text: string }
type ED = { kind: "own" | "bec"; live: boolean }
type BusED = { topic: string; hops: string; threadId: string }

const handles = (
  <>
    <Handle type="target" position={Position.Top} isConnectable={false} className="!size-1.5 !min-h-0 !min-w-0 !border-0 !bg-transparent" />
    <Handle type="source" position={Position.Bottom} isConnectable={false} className="!size-1.5 !min-h-0 !min-w-0 !border-0 !bg-transparent" />
  </>
)

const useFar = () => useStore((s) => s.transform[2] < 0.35)
const rowsOf = (it: Item) => (toneOf(it) === "work" ? Math.min(it.steps.length, ROWS) : 0)

const useCompact = (onEnterCompact?: () => void) => {
  const q = "(max-width: 1023px)"
  const [m, setM] = useState(() => (typeof window !== "undefined" ? matchMedia(q).matches : false))
  const cbRef = useRef(onEnterCompact)
  useEffect(() => {
    cbRef.current = onEnterCompact
  }, [onEnterCompact])
  useEffect(() => {
    const l = matchMedia(q)
    const f = () => {
      setM(l.matches)
      if (l.matches) cbRef.current?.()
    }
    l.addEventListener("change", f)
    return () => l.removeEventListener("change", f)
  }, [])
  return m
}

const providerKind = (provider: string): Kind | null => {
  const p = provider.toLowerCase()
  if (p === "claude") return "claude"
  if (p === "codex" || p === "cdx") return "codex"
  if (p === "opencode" || p === "bec") return "bec"
  if (p === "agy" || p === "antigravity") return "agy"
  return null
}

const transportLabel = (tr?: string) =>
  tr === "claude-uds" ? "UDS" : tr === "agy-agentapi" ? "AGY" : tr === "bus-inbox" ? "Inbox" : tr || "Inbox"

const AgentNode = memo(
  ({ data, selected }: NodeProps<Node<NodeData>>) => {
    const { it, rows } = data
    const far = useFar()
    const tone = toneOf(it)
    const line = lineOf(it)
    const steps = rows ? it.steps.slice(-rows).reverse() : []
    const rc = recapOf(it)
    const model = modelOf(it.model)
    const showMeta = !far && hasMetaRow(it, data.repoTag, data.busCount)

    return (
      <div
        style={{ width: data.hero ? CARD.hero : CARD.w }}
        className={cn(
          "flex flex-col gap-2 rounded-xl border bg-surface px-3.5 py-3 shadow-[var(--shadow-sm)] transition-colors",
          selected
            ? "border-ink-1 ring-1 ring-ink-1"
            : tone === "need"
              ? "border-[var(--sig-need-border)]"
              : tone === "work"
                ? "border-[var(--sig-work-border)]"
                : tone === "bad"
                  ? "border-[var(--sig-bad-border)]"
                  : "border-line-strong",
        )}
      >
        {handles}
        <div className="flex items-center gap-2">
          <SignalBar tone={tone} paused={it.paused} />
          <Brand kind={kindOf(it)} model={it.model} className={far ? "size-7" : "size-4"} />
          <span
            className={cn("min-w-0 flex-1 truncate font-semibold text-ink-1", far ? "text-[22px]" : "text-[14px]")}
            title={`${nameOf(it)} — ${subOf(it)}`}
          >
            {nameOf(it)}
          </span>
          <Shots shots={it.shots} name={nameOf(it)} />
          {!far && (
            <span className={cn("shrink-0 font-mono text-[12px] tabular", tone === "work" ? "text-work" : tone === "need" ? "text-need" : "text-ink-3")}>
              {tone === "work" ? (steps[0] ? <Age t={steps[0].t} /> : "ahora") : tone === "need" ? "te espera" : <Age t={it.updated} />}
            </span>
          )}
          {!far && data.onDetach && (
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation()
                data.onDetach?.()
              }}
              title="Quitar de esta red unida"
              aria-label="Quitar de esta red unida"
              className="pressable grid size-5 shrink-0 place-items-center rounded text-ink-3 hover:bg-hover hover:text-ink-1"
            >
              <X className="size-3.5" />
            </button>
          )}
        </div>

        {showMeta && (
          <div className="flex flex-wrap items-center gap-1.5">
            {data.repoTag && (
              <span className="max-w-[160px] truncate rounded border border-line-strong bg-elevated px-1.5 py-0.5 font-mono text-[12px] font-medium leading-none text-ink-2">
                {data.repoTag}
              </span>
            )}
            {model && (
              <span className="rounded border border-line bg-inset px-1.5 py-0.5 font-mono text-[12px] leading-none text-ink-3">
                {model}
              </span>
            )}
            {(data.busCount ?? 0) > 0 && (
              <button
                type="button"
                onClick={(e) => {
                  e.stopPropagation()
                  data.onOpenBus?.()
                }}
                title="Abrir hilos del bus de sesiones"
                className="pressable inline-flex items-center gap-1 rounded border border-[var(--sig-work-border)] bg-[var(--sig-work-soft)] px-1.5 py-0.5 font-mono text-[12px] font-medium leading-none text-work hover:opacity-90"
              >
                <MessageSquare className="size-3" />
                {data.busCount}
              </button>
            )}
          </div>
        )}

        {!far && (
          <p className={cn("line-clamp-2 text-[13px] leading-snug", tone === "need" ? "font-medium text-ink-1" : "text-ink-2")} title={line}>
            {tone === "bad" ? "Falló: " : ""}
            {line || subOf(it)}
          </p>
        )}

        {!far && hasResponder(it) && (
          <button
            type="button"
            onClick={(e) => {
              e.stopPropagation()
              setTimeout(() => dispatchEvent(new Event("ag:reply")))
            }}
            className="pressable self-start rounded-md bg-ink-1 px-2.5 py-1 text-[12px] font-medium text-ink-inv hover:opacity-90"
          >
            Responder
          </button>
        )}

        {!far && rc && (
          <div className="flex flex-col gap-0.5 rounded-md border border-line bg-inset px-2.5 py-1.5 text-[12px]">
            {rc.did && (
              <p className="flex items-center gap-1.5 truncate text-ink-1" title={rc.did}>
                <Check className="size-3 shrink-0 text-ok" />
                <span className="truncate">{rc.did}</span>
              </p>
            )}
            {rc.next && (
              <p className="flex items-center gap-1.5 truncate text-ink-3" title={rc.next}>
                <ArrowRight className="size-3 shrink-0" />
                <span className="truncate">{rc.next}</span>
              </p>
            )}
          </div>
        )}

        {!far && steps.length > 0 && (
          <ol className="flex flex-col gap-1 border-t border-line pt-2 font-mono text-[12px]">
            {steps.map((s, i) => {
              const Icon = STEP_ICON[s.ic] ?? STEP_ICON.tool
              return (
                <li key={i} className={cn("flex items-center gap-1.5", i === 0 ? "text-ink-1" : "text-ink-3")} title={s.text}>
                  <Icon className="size-3 shrink-0 text-ink-3" />
                  <span className="min-w-0 flex-1 truncate">
                    {s.verb ? (
                      <>
                        <b className="font-semibold">{s.verb}</b>
                        {s.obj ? " " + s.obj : ""}
                      </>
                    ) : (
                      s.text
                    )}
                  </span>
                  {s.n > 1 && <span className="text-[12px] text-ink-3">×{s.n}</span>}
                  <span className="shrink-0 tabular text-ink-3">
                    <Age t={s.t} />
                  </span>
                </li>
              )
            })}
          </ol>
        )}
      </div>
    )
  },
  (p, n) =>
    p.selected === n.selected &&
    p.data.it === n.data.it &&
    p.data.rows === n.data.rows &&
    p.data.repoTag === n.data.repoTag &&
    p.data.busCount === n.data.busCount &&
    Boolean(p.data.onDetach) === Boolean(n.data.onDetach),
)

const RootNode = memo(({ data }: NodeProps<Node<RootData>>) => {
  const { need, work } = data
  const tone = need > 0 ? "need" : work > 0 ? "work" : "idle"
  const txt = need ? `${need} te ${need === 1 ? "espera" : "esperan"}` : work ? `${work} trabajando` : "en calma"
  return (
    <div
      style={{ width: ROOT.w, height: ROOT.h }}
      className="flex items-center justify-center gap-2.5 rounded-lg border border-line-strong bg-elevated px-3 text-[13px] font-semibold text-ink-1 shadow-[var(--shadow-sm)]"
    >
      {handles}
      <span>OWNER</span>
      <span className="h-3.5 w-px bg-line" />
      <SignalBar tone={tone} className="h-3" />
      <span className="text-[12px] font-medium text-ink-2">{txt}</span>
    </div>
  )
})

function treePath(sx: number, sy: number, tx: number, ty: number, r = 10) {
  const down = ty - sy
  const my = down > RANK / 2 + 8 ? sy + RANK / 2 : sy + down / 2
  let pts: [number, number][] = [
    [sx, sy],
    [sx, my],
    [tx, my],
    [tx, ty],
  ]
  pts = pts.filter((p, i) => !i || p[0] !== pts[i - 1][0] || p[1] !== pts[i - 1][1])
  let d = `M${pts[0][0]},${pts[0][1]}`
  for (let i = 1; i < pts.length - 1; i++) {
    const [p0, p, p1] = [pts[i - 1], pts[i], pts[i + 1]]
    const l0 = Math.hypot(p[0] - p0[0], p[1] - p0[1])
    const l1 = Math.hypot(p1[0] - p[0], p1[1] - p[1])
    const k = Math.min(r, l0 / 2, l1 / 2)
    d += `L${p[0] + ((p0[0] - p[0]) / l0) * k},${p[1] + ((p0[1] - p[1]) / l0) * k}Q${p[0]},${p[1]} ${p[0] + ((p1[0] - p[0]) / l1) * k},${p[1] + ((p1[1] - p[1]) / l1) * k}`
  }
  return d + `L${pts[pts.length - 1][0]},${pts[pts.length - 1][1]}`
}

function FlowEdge(p: EdgeProps<Edge<ED>>) {
  const { live } = p.data!
  const reduced = useReducedMotion()
  const path = treePath(p.sourceX, p.sourceY, p.targetX, p.targetY)
  return (
    <>
      <BaseEdge
        id={p.id}
        path={path}
        style={{
          stroke: live ? "var(--edge-live)" : "var(--edge-idle)",
          strokeWidth: live ? 1.6 : 1.2,
          strokeDasharray: live ? undefined : "none",
        }}
      />
      {live && !reduced && (
        <circle r={2.8} fill="var(--sig-work)">
          <animateMotion dur="2s" repeatCount="indefinite" path={path} />
        </circle>
      )}
    </>
  )
}

function BusEdge(p: EdgeProps<Edge<BusED>>) {
  const [path, lx, ly] = getBezierPath({
    sourceX: p.sourceX,
    sourceY: p.sourceY,
    targetX: p.targetX,
    targetY: p.targetY,
    curvature: 0.25,
  })
  return (
    <>
      <BaseEdge
        id={p.id}
        path={path}
        style={{
          stroke: "var(--sig-work)",
          strokeWidth: 1.4,
          strokeDasharray: "5 4",
          opacity: 0.75,
        }}
      />
      {p.data?.topic && (
        <EdgeLabelRenderer>
          <div
            style={{ transform: `translate(-50%, -50%) translate(${lx}px, ${ly}px)` }}
            className="pointer-events-none nodrag nopan absolute rounded border border-[var(--sig-work-border)] bg-surface px-1.5 py-0.5 font-mono text-[11px] text-work shadow-[var(--shadow-sm)]"
          >
            {p.data.topic}
          </div>
        </EdgeLabelRenderer>
      )}
    </>
  )
}

const LabelNode = ({ data }: NodeProps<Node<LabelData>>) => (
  <div className="font-mono text-[12px] font-medium text-ink-3">{data.text}</div>
)

const nodeTypes = { agent: AgentNode, root: RootNode, label: LabelNode }
const edgeTypes = { flow: FlowEdge, bus: BusEdge }
const fitOf = (n: number): FitViewOptions => ({
  padding: { top: "76px", bottom: "100px", left: "5%", right: "5%" },
  maxZoom: n <= 2 ? 1.2 : 1.05,
  minZoom: 0.2,
})

type P = { x: number; y: number }
const PAD = 24

function relax(pos: Record<string, P>, size: (id: string) => [number, number], held: Set<string>, parent: Record<string, string>) {
  const p = { ...pos }
  const ids = Object.keys(p)
  for (let it = 0; it < 16; it++) {
    let moved = false
    for (let i = 0; i < ids.length; i++) {
      for (let j = i + 1; j < ids.length; j++) {
        const a = ids[i]
        const b = ids[j]
        const ha = held.has(a)
        const hb = held.has(b)
        if (ha && hb) continue
        const [aw, ah] = size(a)
        const [bw, bh] = size(b)
        const A = p[a]
        const B = p[b]
        const ox = Math.min(A.x + aw, B.x + bw) - Math.max(A.x, B.x) + PAD
        const oy = Math.min(A.y + ah, B.y + bh) - Math.max(A.y, B.y) + PAD
        if (ox <= 0 || oy <= 0) continue
        const kb = ha ? 1 : hb ? 0 : 0.5
        const ka = 1 - kb
        if (ox < oy) {
          const d = A.x + aw / 2 < B.x + bw / 2 ? 1 : -1
          p[a] = { ...A, x: A.x - d * ox * ka }
          p[b] = { ...B, x: B.x + d * ox * kb }
        } else {
          const d = A.y + ah / 2 < B.y + bh / 2 ? 1 : -1
          p[a] = { ...A, y: A.y - d * oy * ka }
          p[b] = { ...B, y: B.y + d * oy * kb }
        }
        moved = true
      }
    }
    for (const id of ids) {
      const up = parent[id]
      if (!up || !p[up] || held.has(id)) continue
      const min = p[up].y + size(up)[1] + PAD
      if (p[id].y < min - 0.5) {
        p[id] = { ...p[id], y: min }
        moved = true
      }
    }
    if (!moved) break
  }
  return p
}

type CanvasEntry = { win: Win; repoName: string; kids: Card[]; primary: boolean }
const SUBTREE_GAP = 84
const DRAG_MIME = "application/x-agencia-win"

function matchPeerToWin(p: BusPeer, wins: Win[]): string | null {
  for (const w of wins) {
    if (p.pid && w.pid && p.pid === w.pid) return w.id
    if (p.endpoint && (w.id === p.endpoint || w.id.endsWith(":" + p.endpoint))) return w.id
    if (p.cwd && w.cwd && p.cwd === w.cwd && (!p.provider || w.kind === p.provider || (p.provider === "opencode" && w.kind === "opencode"))) {
      return w.id
    }
  }
  return null
}

function GraphCanvas({
  entries,
  need,
  work,
  sel,
  dark,
  busState,
  onOpen,
  onOpenBusThread,
  onDetach,
  onDropWin,
}: {
  entries: CanvasEntry[]
  need: number
  work: number
  sel: string | null
  dark: boolean
  busState: BusState
  onOpen: (id: string) => void
  onOpenBusThread: (threadId?: string) => void
  onDetach: (winId: string) => void
  onDropWin: (winId: string) => void
}) {
  const pkey = "agbench.pos." + entries.map((e) => e.win.id).sort().join("+")
  const [saved, setSaved] = useState<Record<string, P>>(() => {
    try {
      return JSON.parse(localStorage.getItem(pkey) || "{}")
    } catch {
      return {}
    }
  })
  const [moving, setMoving] = useState(false)
  const [dropOver, setDropOver] = useState(false)
  const [hs, setHs] = useState<Record<string, number>>({})

  const canvasWins = useMemo(() => entries.map((e) => e.win), [entries])
  const { winBusCounts, busEdgeList } = useMemo(() => {
    const peerToWin = new Map<string, string>()
    for (const p of busState.peers) {
      const wid = matchPeerToWin(p, canvasWins)
      if (wid) peerToWin.set(p.peer_id, wid)
    }
    const counts = new Map<string, number>()
    const openThreads = new Map(busState.threads.filter((t) => t.status === "open").map((t) => [t.thread_id, t]))
    const pairs = new Map<string, { source: string; target: string; thread: BusThread }>()
    for (const m of busState.messages) {
      const th = openThreads.get(m.thread_id)
      if (!th) continue
      const sw = peerToWin.get(m.from_peer)
      const tw = peerToWin.get(m.to_peer)
      if (sw) counts.set(sw, (counts.get(sw) ?? 0) + (m.read_at ? 0 : 1))
      if (tw) counts.set(tw, (counts.get(tw) ?? 0) + (m.read_at ? 0 : 1))
      if (sw && tw && sw !== tw) {
        const key = [sw, tw].sort().join("--") + ":" + th.thread_id
        if (!pairs.has(key)) pairs.set(key, { source: sw, target: tw, thread: th })
      }
    }
    return { winBusCounts: counts, busEdgeList: Array.from(pairs.values()) }
  }, [busState, canvasWins])

  const H = useCallback(
    (it: Item, repoTag?: string, busCount?: number) => hs[it.id] ?? CARD.h(it, rowsOf(it), repoTag, busCount),
    [hs],
  )

  const { nodes: built, edges, freeGroups, treeEdges } = useMemo(() => {
    const sub = (c: Card) => c.children ?? []
    const multi = entries.length > 1
    const multiRepo = new Set(entries.map((e) => e.repoName)).size > 1
    const hy = ROOT.h + RANK
    const plans = entries.map((entry) => {
      const liveKids = entry.kids.filter((c) => sub(c).length || isWin(c as Item) || toneOf(c) === "work" || toneOf(c) === "need")
      const freeKids = entry.kids.filter((c) => !liveKids.includes(c))
      const maxCols = multi ? 2 : 3
      const cols = Math.min(maxCols, Math.max(1, liveKids.length, freeKids.length ? Math.min(maxCols, freeKids.length) : 0))
      const width = Math.max(cols * CARD.w + (cols - 1) * GAP, CARD.hero)
      return { entry, liveKids, freeKids, cols, width }
    })
    const totalW = plans.reduce((s, p) => s + p.width, 0) + Math.max(0, plans.length - 1) * SUBTREE_GAP
    const rootCx = totalW / 2
    const ns: Node[] = [
      { id: "root", type: "root", position: { x: rootCx - ROOT.w / 2, y: 0 }, data: { need, work } satisfies RootData, draggable: false, selectable: false },
    ]
    const es: Edge[] = []
    const treeEs: Edge[] = []
    const fGroups: { labelId: string; ids: string[] }[] = []

    let xOff = 0
    for (const plan of plans) {
      const { entry, liveKids, freeKids, cols, width } = plan
      const { win, repoName, primary } = entry
      const cx = xOff + width / 2
      const winItems = flat(win)
      const winLive = winItems.some((c) => toneOf(c) === "need" || isWork(c))
      const repoTag = multiRepo || !primary ? repoName : undefined
      const busCount = winBusCounts.get(win.id) ?? 0
      ns.push({
        id: win.id,
        type: "agent",
        position: { x: cx - CARD.hero / 2, y: hy },
        data: {
          it: win,
          rows: rowsOf(win),
          hero: true,
          repoTag,
          busCount,
          onOpenBus: () => onOpenBusThread(),
          onDetach: primary ? undefined : () => onDetach(win.id),
        } satisfies NodeData,
      })
      const rootEdge: Edge = { id: "e-root-" + win.id, source: "root", target: win.id, type: "flow", data: { kind: "own", live: winLive } satisfies ED }
      es.push(rootEdge)
      treeEs.push(rootEdge)

      let y = hy + H(win, repoTag, busCount) + RANK
      const place = (list: Card[], from = win.id) => {
        for (let r = 0; r * cols < list.length; r++) {
          const row = list.slice(r * cols, r * cols + cols)
          const rw = row.length * CARD.w + (row.length - 1) * GAP
          row.forEach((c, i) => {
            ns.push({ id: c.id, type: "agent", position: { x: cx - rw / 2 + i * (CARD.w + GAP), y }, data: { it: c, rows: rowsOf(c) } satisfies NodeData })
            const edge: Edge = { id: "e-" + from + "-" + c.id, source: from, target: c.id, type: "flow", data: { kind: kindOf(c) === "bec" ? "bec" : "own", live: isWork(c) } satisfies ED }
            es.push(edge)
            treeEs.push(edge)
          })
          y += Math.max(...row.map((c) => H(c))) + GAP / 2
        }
      }
      place(liveKids)
      const deeper = (list: Card[]) => {
        for (const w of list) {
          if (sub(w).length) {
            y += GAP / 2
            place(sub(w), w.id)
            deeper(sub(w))
          }
        }
      }
      deeper(liveKids)
      if (freeKids.length) {
        if (liveKids.length) y += GAP
        const labelId = multi ? `libres:${win.id}` : "libres"
        ns.push({ id: labelId, type: "label", position: { x: cx - width / 2, y }, data: { text: `Libres (${freeKids.length})` } satisfies LabelData, draggable: false, selectable: false })
        y += 28
        place(freeKids)
        fGroups.push({ labelId, ids: freeKids.map((c) => c.id) })
      }
      xOff += width + SUBTREE_GAP
    }

    for (const be of busEdgeList) {
      es.push({
        id: `bus-${be.thread.thread_id}-${be.source}-${be.target}`,
        source: be.source,
        target: be.target,
        type: "bus",
        data: {
          topic: (be.thread.topic || "Bus").slice(0, 26),
          hops: `${be.thread.hops_left}/${be.thread.max_hops}`,
          threadId: be.thread.thread_id,
        } satisfies BusED,
      })
    }

    return { nodes: ns, edges: es, freeGroups: fGroups, treeEdges: treeEs }
  }, [entries, need, work, H, onDetach, winBusCounts, busEdgeList, onOpenBusThread])

  const [nodes, setNodes, applyChanges] = useNodesState<Node>([])
  const onNodesChange = useCallback(
    (cs: NodeChange<Node>[]) => {
      applyChanges(cs)
      const got: Record<string, number> = {}
      for (const c of cs) if (c.type === "dimensions" && c.dimensions) got[c.id] = Math.round(c.dimensions.height)
      if (Object.keys(got).length) setHs((p) => (Object.entries(got).some(([k, v]) => p[k] !== v) ? { ...p, ...got } : p))
    },
    [applyChanges],
  )

  const parent = useMemo(() => Object.fromEntries(treeEdges.map((e) => [e.target, e.source])), [treeEdges])
  const kidsOf = useCallback(
    (id: string) => {
      const out: string[] = []
      const walk = (x: string) => {
        for (const e of treeEdges) {
          if (e.source === x && !out.includes(e.target)) {
            out.push(e.target)
            walk(e.target)
          }
        }
      }
      walk(id)
      return out
    },
    [treeEdges],
  )
  const size = useCallback(
    (id: string): [number, number] => {
      const n = built.find((b) => b.id === id)
      if (!n || n.type !== "agent") return [ROOT.w, ROOT.h]
      const d = n.data as NodeData
      return [d.hero ? CARD.hero : CARD.w, H(d.it, d.repoTag, d.busCount)]
    },
    [built, H],
  )
  const place = useCallback((want: Record<string, P>, held: Set<string>) => relax(want, size, held, parent), [size, parent])
  const withLabel = useCallback(
    (p: Record<string, P>) => {
      const next = { ...p }
      for (const g of freeGroups) {
        const f = g.ids.filter((id) => next[id])
        if (f.length) next[g.labelId] = { x: Math.min(...f.map((id) => next[id].x)), y: Math.min(...f.map((id) => next[id].y)) - 28 }
      }
      return next
    },
    [freeGroups],
  )

  const rest = useMemo(() => {
    const want = Object.fromEntries(built.filter((n) => n.type !== "label").map((n) => [n.id, saved[n.id] ?? n.position]))
    return withLabel(place(want, new Set(["root", ...Object.keys(saved).filter((id) => want[id])])))
  }, [built, saved, place, withLabel])

  const drag = useRef<{ id: string; from: Record<string, P>; tail: string[]; now: Record<string, P>; raf: number } | null>(null)
  useEffect(() => {
    if (drag.current) return
    setNodes((prev) =>
      built.map((n) => {
        const o = prev.find((p) => p.id === n.id)
        return { ...(o ?? n), position: rest[n.id] ?? n.position, data: n.data, draggable: n.draggable, selected: n.id === sel, className: moving ? "reflow-node" : undefined }
      }),
    )
  }, [built, rest, sel, setNodes, moving])

  const onStart = (_: unknown, n: Node) => {
    const from = Object.fromEntries(nodes.filter((m) => m.type !== "label").map((m) => [m.id, m.position]))
    drag.current = { id: n.id, from, tail: kidsOf(n.id), now: from, raf: 0 }
    setMoving(true)
  }
  const onDrag = (_: unknown, n: Node) => {
    const d = drag.current
    if (!d) return
    d.now = { ...d.now, [n.id]: n.position }
    d.raf ||= requestAnimationFrame(() => {
      d.raf = 0
      const at = d.now[d.id]
      const o = d.from[d.id]
      const dx = at.x - o.x
      const dy = at.y - o.y
      const want = { ...d.from, [d.id]: at }
      for (const k of d.tail) want[k] = { x: d.from[k].x + dx, y: d.from[k].y + dy }
      const p = withLabel(place(want, new Set(["root", d.id, ...d.tail])))
      d.now = p
      setNodes((prev) => prev.map((m) => (m.id === d.id ? m : { ...m, position: p[m.id] ?? m.position })))
    })
  }
  const onStop = (_: unknown, n: Node) => {
    const d = drag.current
    if (!d) return
    cancelAnimationFrame(d.raf)
    drag.current = null
    setTimeout(() => setMoving(false), 320)
    const now = { ...d.now, [n.id]: n.position }
    for (const g of freeGroups) delete now[g.labelId]
    const up = parent[n.id]
    const dy = up && now[up] ? Math.max(0, now[up].y + size(up)[1] + PAD - now[n.id].y) : 0
    if (dy) for (const k of [n.id, ...d.tail]) now[k] = { ...now[k], y: now[k].y + dy }
    const keep = place(now, new Set(["root", n.id, ...d.tail]))
    delete keep.root
    setSaved(keep)
    try {
      localStorage.setItem(pkey, JSON.stringify(keep))
    } catch {
      /* ignore */
    }
  }
  const reset = () => {
    setSaved({})
    try {
      localStorage.removeItem(pkey)
    } catch {
      /* ignore */
    }
    setMoving(true)
    setTimeout(() => setMoving(false), 320)
  }

  const { fitView } = useReactFlow()
  const box = useRef<HTMLDivElement>(null)
  const count = built.length
  useEffect(() => {
    const t = setTimeout(() => fitView(fitOf(count - 1)), 60)
    return () => clearTimeout(t)
  }, [count, fitView])
  useEffect(() => {
    const el = box.current
    if (!el) return
    let t = 0
    const ro = new ResizeObserver(() => {
      clearTimeout(t)
      t = window.setTimeout(() => fitView(fitOf(count - 1)), 120)
    })
    ro.observe(el)
    return () => {
      ro.disconnect()
      clearTimeout(t)
    }
  }, [count, fitView])

  return (
    <div
      ref={box}
      onDragOver={(e) => {
        if (!e.dataTransfer.types.includes(DRAG_MIME)) return
        e.preventDefault()
        e.dataTransfer.dropEffect = "link"
        if (!dropOver) setDropOver(true)
      }}
      onDragLeave={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget as globalThis.Node | null)) setDropOver(false)
      }}
      onDrop={(e) => {
        const wid = e.dataTransfer.getData(DRAG_MIME)
        setDropOver(false)
        if (!wid) return
        e.preventDefault()
        onDropWin(wid)
      }}
      className={cn("relative size-full blueprint-grid transition-shadow", dropOver && "ring-2 ring-inset ring-work")}
    >
      {dropOver && (
        <div className="pointer-events-none absolute inset-4 z-20 grid place-items-center rounded-xl border-2 border-dashed border-work bg-surface/80">
          <div className="flex items-center gap-2 rounded-lg border border-line-strong bg-elevated px-4 py-2 text-[13px] font-medium text-ink-1 shadow-[var(--shadow-md)]">
            <Link2 className="size-4 text-work" />
            Suelta para unir a esta red visual
          </div>
        </div>
      )}
      <ReactFlow
        nodes={nodes}
        onNodesChange={onNodesChange}
        edges={edges}
        nodeTypes={nodeTypes}
        edgeTypes={edgeTypes}
        colorMode={dark ? "dark" : "light"}
        fitView
        fitViewOptions={fitOf(nodes.length - 1)}
        minZoom={0.18}
        maxZoom={1.5}
        onNodeClick={(_, n) => n.type === "agent" && onOpen(n.id)}
        onEdgeClick={(_, e) => {
          if (e.type === "bus" && e.data && "threadId" in e.data) {
            onOpenBusThread((e.data as BusED).threadId)
          }
        }}
        nodesConnectable={false}
        nodesDraggable
        nodeDragThreshold={5}
        onNodeDragStart={onStart}
        onNodeDrag={onDrag}
        onNodeDragStop={onStop}
        elementsSelectable={false}
        proOptions={{ hideAttribution: true }}
        className="!bg-transparent"
      >
        <Controls showInteractive={false} position="top-right" className="!m-3">
          {Object.keys(saved).length > 0 && (
            <ControlButton onClick={reset} title="Restaurar disposición automática">
              <Undo2 className="size-3.5" />
            </ControlButton>
          )}
        </Controls>
      </ReactFlow>
    </div>
  )
}

function WindowComposer({ win, demoMode }: { win: Win; demoMode?: boolean }) {
  const [lines, setLines] = useState<{ t: number; ok: boolean; win: string; text: string }[]>([])
  const [v, setV] = useState("")
  const [open, setOpen] = useState(false)
  const [busy, setBusy] = useState(false)
  const [hist, setHist] = useState(-1)
  const ref = useRef<HTMLInputElement>(null)

  const blocked =
    win.kind === "claude"
      ? undefined
      : win.kind === "agy"
        ? win.linked
          ? undefined
          : "Se enlaza la primera vez que esta ventana corre un comando"
        : "Esta ventana solo se observa; usa el tab Bus para enviarle mensajes"
  const mine = lines.filter((l) => l.win === win.id)

  useEffect(() => {
    const k = (e: KeyboardEvent) => {
      if (e.key === "/" && !(e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement)) {
        e.preventDefault()
        ref.current?.focus()
      }
    }
    const reply = () => ref.current?.focus()
    addEventListener("keydown", k)
    addEventListener("ag:reply", reply)
    return () => {
      removeEventListener("keydown", k)
      removeEventListener("ag:reply", reply)
    }
  }, [])

  const zero: Usage = { in: 0, out: 0, cache: 0, turns: 0 }
  const totalTok = win.children.reduce((s, c) => s + used(c.usage), used(win.usage ?? zero))

  async function submit() {
    const text = v.trim()
    if (!text || blocked || busy) return
    if (demoMode) {
      setLines((p) => [...p, { t: Date.now() / 1000, ok: true, win: win.id, text }])
      setV("")
      toast.success(`Mensaje simulado a ${win.title}`)
      return
    }
    setBusy(true)
    const r = await post("/api/send", { to: win.kind === "agy" ? win.id : win.id.slice(2), text })
    setBusy(false)
    setLines((p) => [...p, { t: Date.now() / 1000, ok: r.ok, win: win.id, text: r.ok ? text : r.info || "No llegó" }])
    if (r.ok) {
      setV("")
      setHist(-1)
      toast.success(`Enviado a ${win.title}`)
    } else {
      setOpen(true)
      toast.error("No llegó", { description: r.info })
    }
  }

  const sent = mine.filter((l) => l.ok).map((l) => l.text)
  return (
    <div className="pointer-events-auto mx-auto w-full max-w-[740px]">
      <div className="overflow-hidden rounded-xl border border-line-strong bg-surface shadow-[var(--shadow-lg)]">
        {open && (
          <div className="max-h-44 overflow-y-auto border-b border-line bg-inset px-3.5 py-2.5 text-[12px]">
            {!mine.length && <p className="text-ink-3">Sin mensajes enviados a {win.title}.</p>}
            {mine.map((l, i) => (
              <div key={i} className="flex gap-2.5 py-1">
                <span className="shrink-0 font-mono text-[12px] tabular text-ink-3">{hhmm(l.t)}</span>
                <span className={l.ok ? "text-ink-1" : "text-bad"}>{l.text}</span>
              </div>
            ))}
          </div>
        )}
        <div className="flex items-center gap-2 px-3 py-2">
          <span className="inline-flex h-7 max-w-[210px] shrink-0 items-center gap-1.5 rounded-md border border-line bg-inset px-2 text-[12px] font-medium text-ink-1">
            <Brand kind={kindOf(win)} model={win.model} className="size-3.5" />
            <span className="truncate">{win.title}</span>
          </span>
          <input
            ref={ref}
            value={v}
            onChange={(e) => setV(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault()
                submit()
              } else if (e.key === "ArrowUp" && sent.length) {
                e.preventDefault()
                const i = hist < 0 ? sent.length - 1 : Math.max(0, hist - 1)
                setHist(i)
                setV(sent[i])
              } else if (e.key === "ArrowDown" && hist >= 0) {
                e.preventDefault()
                const i = hist + 1
                if (i >= sent.length) {
                  setHist(-1)
                  setV("")
                } else {
                  setHist(i)
                  setV(sent[i])
                }
              } else if (e.key === "Escape") {
                setOpen(false)
                ref.current?.blur()
              }
            }}
            spellCheck={false}
            disabled={!!blocked}
            placeholder={blocked || "Escribe una instrucción (/ para enfocar)…"}
            className="h-8 min-w-0 flex-1 bg-transparent text-[13.5px] text-ink-1 outline-none placeholder:text-ink-3 disabled:opacity-50"
          />
          {totalTok > 0 && (
            <span className="hidden font-mono text-[12px] tabular text-ink-3 sm:inline">{tok(totalTok)} tok</span>
          )}
          <button
            type="button"
            onClick={() => setOpen(!open)}
            aria-label="Historial de envíos"
            className={cn(
              "pressable grid size-7 shrink-0 place-items-center rounded-md border",
              open ? "border-line-strong bg-elevated text-ink-1" : "border-transparent text-ink-3 hover:bg-hover hover:text-ink-1",
            )}
          >
            <History className="size-3.5" />
          </button>
          <button
            type="button"
            disabled={!v.trim() || !!blocked || busy}
            onClick={submit}
            aria-label="Enviar"
            className="pressable grid size-8 shrink-0 place-items-center rounded-md bg-ink-1 text-ink-inv hover:opacity-90 disabled:opacity-30"
          >
            <ArrowUp className="size-4" />
          </button>
        </div>
      </div>
    </div>
  )
}

const nowUnix = () => Math.floor(Date.now() / 1000)

function BusThreadViewer({
  thread,
  messages,
  newPeer,
  demoMode,
  onClose,
  onRefresh,
  onLocalAdd,
}: {
  thread: BusThread | null
  messages: BusMessage[]
  newPeer: BusPeer | null
  demoMode?: boolean
  onClose: () => void
  onRefresh: () => void
  onLocalAdd: (msg: BusMessage, newTh?: BusThread, done?: boolean) => void
}) {
  const [topic, setTopic] = useState("")
  const [body, setBody] = useState("")
  const [done, setDone] = useState(false)
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState("")
  const scrollRef = useRef<HTMLDivElement>(null)
  const demoSeqRef = useRef(1)

  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight
    }
  }, [messages.length, thread?.thread_id])

  const sorted = messages.slice().sort((a, b) => a.seq - b.seq)
  const canReply = newPeer ? true : thread?.status === "open"

  async function handleSend() {
    const text = body.trim()
    if (!text || busy) return
    setErr("")
    const nowSec = nowUnix()

    if (demoMode) {
      if (newPeer) {
        const n = demoSeqRef.current++
        const tid = `demo-${n}`
        const th: BusThread = {
          thread_id: tid,
          topic: topic.trim() || text.slice(0, 60),
          created_by: "owner",
          created_at: nowSec,
          hops_left: 5,
          max_hops: 6,
          status: "open",
        }
        const msg: BusMessage = {
          msg_id: `m-${nowSec}-${n}`,
          thread_id: tid,
          seq: 1,
          from_peer: "owner",
          to_peer: newPeer.peer_id,
          body: text,
          expect_reply: !done,
          delivered_via: newPeer.transport || "bus-inbox",
          delivered_ok: true,
          delivered_info: "simulado",
          created_at: nowSec,
        }
        onLocalAdd(msg, th)
        toast.success(`Hilo simulado enviado a ${newPeer.name || newPeer.peer_id}`)
        setBody("")
        return
      }
      if (thread) {
        const n = demoSeqRef.current++
        const lastOther = [...sorted].reverse().find((m) => m.from_peer !== "owner")?.from_peer ?? sorted[sorted.length - 1]?.to_peer ?? "peer"
        const msg: BusMessage = {
          msg_id: `m-${nowSec}-${n}`,
          thread_id: thread.thread_id,
          seq: (sorted[sorted.length - 1]?.seq ?? 0) + 1,
          from_peer: "owner",
          to_peer: lastOther,
          body: text,
          expect_reply: !done,
          delivered_via: "bus-inbox",
          delivered_ok: true,
          delivered_info: "simulado",
          created_at: nowSec,
        }
        onLocalAdd(msg, undefined, done)
        toast.success(done ? "Respuesta enviada e hilo cerrado (demo)" : "Respuesta simulada en el bus")
        setBody("")
        return
      }
    }

    setBusy(true)
    const r = (await post(
      newPeer ? "/api/bus/send" : "/api/bus/reply",
      newPeer
        ? { to: newPeer.peer_id, topic: topic.trim(), body: text, done }
        : { thread_id: thread!.thread_id, body: text, done },
    )) as { ok: boolean; info?: string; error?: string; thread_id?: string }
    setBusy(false)

    if (r.ok) {
      setBody("")
      toast.success(newPeer ? `Enviado a ${newPeer.name || newPeer.peer_id}` : done ? "Hilo cerrado" : "Respuesta enviada")
      onRefresh()
    } else {
      const msg = r.info || r.error || "Endpoint /api/bus no disponible en este servidor"
      setErr(msg)
      toast.error("No se pudo enviar por el bus", { description: msg })
    }
  }

  return (
    <div className="pointer-events-auto flex max-h-[min(430px,calc(100%-130px))] w-[min(480px,calc(100%-24px))] flex-col overflow-hidden rounded-xl border border-line-strong bg-surface shadow-[var(--shadow-lg)]">
      {/* Header */}
      <div className="ag-grad-elevated flex items-center justify-between gap-2 border-b border-line px-3.5 py-2.5">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            {newPeer ? (
              <>
                {providerKind(newPeer.provider) ? (
                  <Brand kind={providerKind(newPeer.provider)!} className="size-3.5" />
                ) : (
                  <Terminal className="size-3.5 text-ink-2" />
                )}
                <span className="truncate text-[13.5px] font-semibold text-ink-1">
                  Nuevo hilo para {newPeer.name || newPeer.title || newPeer.peer_id}
                </span>
                <span className="shrink-0 rounded border border-line bg-inset px-1.5 py-0.5 font-mono text-[11px] text-ink-2">
                  {transportLabel(newPeer.transport)}
                </span>
              </>
            ) : (
              <>
                <span className="truncate text-[13.5px] font-semibold text-ink-1">{thread!.topic || "(sin tema)"}</span>
                <span
                  className={cn(
                    "shrink-0 rounded border px-1.5 py-0.5 font-mono text-[11px] font-medium leading-none",
                    thread!.status === "open"
                      ? "border-[var(--sig-work-border)] bg-[var(--sig-work-soft)] text-work"
                      : thread!.status === "exhausted"
                        ? "border-[var(--sig-bad-border)] bg-[var(--sig-bad-soft)] text-bad"
                        : "border-[var(--sig-ok-border)] bg-[var(--sig-ok-soft)] text-ok",
                  )}
                >
                  {thread!.status === "open" ? "Abierto" : thread!.status === "exhausted" ? "Agotado" : "Cerrado"}
                </span>
                <span className="shrink-0 font-mono text-[12px] tabular text-ink-3">
                  {thread!.hops_left}/{thread!.max_hops} hops
                </span>
              </>
            )}
          </div>
          {newPeer && (newPeer.repo || newPeer.branch) && (
            <p className="mt-0.5 truncate font-mono text-[12px] text-ink-3">
              {newPeer.repo || "sin repo"}
              {newPeer.branch ? ` / ${newPeer.branch}` : ""}
            </p>
          )}
        </div>
        <div className="flex shrink-0 items-center gap-1">
          {!newPeer && (
            <button
              type="button"
              onClick={onRefresh}
              title="Actualizar hilo"
              aria-label="Actualizar hilo"
              className="pressable grid size-7 place-items-center rounded-md text-ink-3 hover:bg-hover hover:text-ink-1"
            >
              <RefreshCw className="size-3.5" />
            </button>
          )}
          <button
            type="button"
            onClick={onClose}
            aria-label="Cerrar hilo"
            className="pressable grid size-7 place-items-center rounded-md text-ink-3 hover:bg-hover hover:text-ink-1"
          >
            <X className="size-4" />
          </button>
        </div>
      </div>

      {/* Body / Message History */}
      {!newPeer && (
        <div ref={scrollRef} className="min-h-0 flex-1 space-y-2.5 overflow-y-auto bg-inset/40 px-3.5 py-3">
          {sorted.length === 0 ? (
            <p className="text-[13px] text-ink-3">Sin mensajes registrados en este hilo.</p>
          ) : (
            sorted.map((m) => (
              <div key={m.msg_id ?? `${m.thread_id}-${m.seq}`} className="rounded-lg border border-line bg-surface px-3 py-2">
                <div className="flex flex-wrap items-center justify-between gap-2 text-[12px]">
                  <span className="font-mono font-medium text-ink-1">
                    #{m.seq} {m.from_peer} → {m.to_peer}
                  </span>
                  <div className="flex items-center gap-2 font-mono text-[11px] text-ink-3">
                    {m.delivered_via && (
                      <span className={cn(m.delivered_ok ? "text-ok" : "text-bad")}>
                        {transportLabel(m.delivered_via)}
                      </span>
                    )}
                    {m.created_at ? <span className="tabular">{hhmm(m.created_at)}</span> : null}
                  </div>
                </div>
                <p className="mt-1 whitespace-pre-wrap break-words text-[13px] leading-relaxed text-ink-1">{m.body}</p>
              </div>
            ))
          )}
        </div>
      )}

      {/* Composer / Reply Bar */}
      <form
        onSubmit={(e) => {
          e.preventDefault()
          handleSend()
        }}
        className="flex flex-col gap-2 border-t border-line bg-surface px-3.5 py-2.5"
      >
        {err && (
          <p className="rounded-md border border-[var(--sig-bad-border)] bg-[var(--sig-bad-soft)] px-2.5 py-1.5 text-[12px] text-bad">
            {err}
          </p>
        )}
        {newPeer && (
          <input
            value={topic}
            onChange={(e) => setTopic(e.target.value)}
            placeholder="Tema del hilo (opcional)…"
            className="h-8 rounded-md border border-line bg-inset px-2.5 text-[13px] text-ink-1 outline-none focus:border-work placeholder:text-ink-3"
          />
        )}
        <div className="flex items-center gap-2">
          <input
            value={body}
            onChange={(e) => setBody(e.target.value)}
            disabled={!canReply || busy}
            placeholder={
              canReply
                ? newPeer
                  ? `Mensaje vía ${transportLabel(newPeer.transport)}…`
                  : "Responder en el hilo…"
                : `Hilo ${thread?.status === "exhausted" ? "agotado" : "cerrado"}`
            }
            className="h-8 min-w-0 flex-1 rounded-md border border-line bg-inset px-2.5 text-[13px] text-ink-1 outline-none focus:border-work placeholder:text-ink-3 disabled:opacity-50"
          />
          <label className="inline-flex shrink-0 cursor-pointer items-center gap-1.5 text-[12px] text-ink-2">
            <input
              type="checkbox"
              checked={done}
              disabled={!canReply || busy}
              onChange={(e) => setDone(e.target.checked)}
              className="size-3.5 accent-ink-1"
            />
            <span>Cerrar</span>
          </label>
          <button
            type="submit"
            disabled={!body.trim() || !canReply || busy}
            aria-label="Enviar mensaje al bus"
            className="pressable inline-flex h-8 shrink-0 items-center gap-1.5 rounded-md bg-ink-1 px-3 text-[12.5px] font-medium text-ink-inv hover:opacity-90 disabled:opacity-35"
          >
            <ArrowUp className="size-3.5" />
            Enviar
          </button>
        </div>
      </form>
    </div>
  )
}

const JOINED_KEY = "agbench.red.joined"

export function RedView({
  repos,
  repo,
  win,
  fil,
  setFil,
  openId,
  treeOpen,
  dark,
  demoMode,
  bus,
  onPickWin,
  onOpen,
  onLaunch,
}: {
  repos: Repo[]
  repo: Repo
  win: Win
  fil: string
  setFil: (f: string) => void
  openId: string | null
  treeOpen: boolean
  dark: boolean
  demoMode?: boolean
  bus?: BusState
  onPickWin: (id: string) => void
  onOpen: (id: string) => void
  onLaunch: () => void
}) {
  const [compactOverlayOpen, setCompactOverlayOpen] = useState(false)
  const [wideBusOpen, setWideBusOpen] = useState(false)
  const compact = useCompact(() => setCompactOverlayOpen(false))
  const asideOpen = compact ? compactOverlayOpen : treeOpen || wideBusOpen
  const setAsideOpen = useCallback(
    (open: boolean) => {
      if (compact) setCompactOverlayOpen(open)
      else setWideBusOpen(open)
    },
    [compact],
  )
  const [asideTab, setAsideTab] = useState<"ventanas" | "bus">("ventanas")

  const { bus: fetchedBus, loading: busLoading, refresh: refreshBus } = useBus(bus, demoMode)
  const [demoBusExtras, setDemoBusExtras] = useState<{ threads: BusThread[]; messages: BusMessage[]; closedIds: string[] }>({
    threads: [],
    messages: [],
    closedIds: [],
  })

  const busState = useMemo<BusState>(() => {
    if (!demoMode || (!demoBusExtras.threads.length && !demoBusExtras.messages.length && !demoBusExtras.closedIds.length)) {
      return fetchedBus
    }
    const closed = new Set(demoBusExtras.closedIds)
    return {
      peers: fetchedBus.peers,
      threads: [
        ...fetchedBus.threads.map((t) => (closed.has(t.thread_id) ? { ...t, status: "done", hops_left: 0 } : t)),
        ...demoBusExtras.threads,
      ],
      messages: [...fetchedBus.messages, ...demoBusExtras.messages],
    }
  }, [fetchedBus, demoMode, demoBusExtras])

  const [activeThreadId, setActiveThreadId] = useState<string | null>(null)
  const [newPeerTarget, setNewPeerTarget] = useState<BusPeer | null>(null)

  const [joinedIds, setJoinedIds] = useState<string[]>(() => {
    try {
      const raw = JSON.parse(localStorage.getItem(JOINED_KEY) || "[]")
      return Array.isArray(raw) ? raw.filter((x): x is string => typeof x === "string") : []
    } catch {
      return []
    }
  })
  const [composerWinId, setComposerWinId] = useState<string | null>(null)

  const saveJoined = useCallback((next: string[]) => {
    setJoinedIds(next)
    try {
      localStorage.setItem(JOINED_KEY, JSON.stringify(next))
    } catch {
      /* ignore */
    }
  }, [])

  const winLookup = useMemo(() => {
    const map = new Map<string, { win: Win; repoName: string }>()
    for (const r of repos) for (const w of r.windows) map.set(w.id, { win: w, repoName: r.name })
    return map
  }, [repos])

  const joinWin = useCallback(
    (wid: string) => {
      if (wid === win.id) return
      const found = winLookup.get(wid)
      if (!found) return
      if (joinedIds.includes(wid)) {
        toast.message(`${nameOf(found.win)} ya está en esta red`)
        return
      }
      saveJoined([...joinedIds, wid])
      toast.success(`Unida a la red: ${nameOf(found.win)} (${found.repoName})`)
    },
    [win.id, winLookup, joinedIds, saveJoined],
  )

  const detachWin = useCallback(
    (wid: string) => {
      saveJoined(joinedIds.filter((id) => id !== wid))
      if (composerWinId === wid) setComposerWinId(null)
    },
    [joinedIds, saveJoined, composerWinId],
  )

  const toggleJoin = useCallback(
    (wid: string) => {
      if (wid === win.id) return
      if (joinedIds.includes(wid)) detachWin(wid)
      else joinWin(wid)
    },
    [win.id, joinedIds, detachWin, joinWin],
  )

  const entries = useMemo<CanvasEntry[]>(() => {
    const filterKids = (w: Win) => (w.children ?? []).filter((c) => fil === "all" || groupOf(c) === fil)
    const out: CanvasEntry[] = [{ win, repoName: repo.name, kids: filterKids(win), primary: true }]
    const seen = new Set<string>([win.id])
    for (const id of joinedIds) {
      if (seen.has(id)) continue
      const found = winLookup.get(id)
      if (!found) continue
      seen.add(id)
      out.push({ win: found.win, repoName: found.repoName, kids: filterKids(found.win), primary: false })
    }
    return out
  }, [win, repo.name, joinedIds, winLookup, fil])

  const activeIds = useMemo(() => new Set(entries.map((e) => e.win.id)), [entries])
  const canvasItems = useMemo(() => entries.flatMap((e) => flat(e.win)), [entries])
  const allItems = useMemo(() => repo.windows.flatMap(flat), [repo])
  const need = canvasItems.filter((c) => toneOf(c) === "need").length
  const work = canvasItems.filter(isWork).length

  const composerWin = entries.find((e) => e.win.id === composerWinId)?.win ?? win
  const otherRepos = useMemo(() => repos.filter((r) => r.name !== repo.name && r.windows.length > 0), [repos, repo.name])
  const canvasKey = entries.map((e) => e.win.id).sort().join("+")

  const handleOpen = useCallback(
    (id: string) => {
      onOpen(id)
      const owner = entries.find((e) => flat(e.win).some((x) => x.id === id))
      if (owner) setComposerWinId(owner.win.id)
    },
    [onOpen, entries],
  )

  const startDragWin = (e: React.DragEvent, wid: string) => {
    e.dataTransfer.setData(DRAG_MIME, wid)
    e.dataTransfer.effectAllowed = "link"
  }

  // Group bus peers by repo and summarize threads
  const peersByRepo = useMemo(() => {
    const groups = new Map<string, BusPeer[]>()
    for (const p of busState.peers) {
      const key = p.repo || "Sin repo"
      const list = groups.get(key)
      if (list) list.push(p)
      else groups.set(key, [p])
    }
    return Array.from(groups.entries()).sort((a, b) => a[0].localeCompare(b[0]))
  }, [busState.peers])

  const msgsByThread = useMemo(() => {
    const map = new Map<string, BusMessage[]>()
    for (const m of busState.messages) {
      const list = map.get(m.thread_id)
      if (list) list.push(m)
      else map.set(m.thread_id, [m])
    }
    return map
  }, [busState.messages])

  const openThreadsCount = busState.threads.filter((t) => t.status === "open").length
  const unreadCount = busState.messages.filter((m) => !m.read_at).length
  const busBadge = openThreadsCount || unreadCount

  const activeThread = busState.threads.find((t) => t.thread_id === activeThreadId) ?? null
  const activeThreadMsgs = activeThread ? msgsByThread.get(activeThread.thread_id) ?? [] : []

  const openBusFromCanvas = useCallback(
    (threadId?: string) => {
      if (threadId) {
        setNewPeerTarget(null)
        setActiveThreadId(threadId)
      }
      setAsideOpen(true)
      setAsideTab("bus")
    },
    [setAsideOpen],
  )

  return (
    <div className="relative flex h-full min-h-0">
      {/* Backdrop when aside overlays on <1024px */}
      {compact && asideOpen && (
        <div
          aria-hidden="true"
          onClick={() => setAsideOpen(false)}
          className="fixed inset-0 z-20 bg-black/40"
        />
      )}

      {/* Contextual Fleet Tree + Sesiones Bus Sidebar */}
      {asideOpen && (
        <aside
          className={cn(
            "flex w-[276px] max-w-[85vw] shrink-0 flex-col border-r border-line bg-surface",
            compact && "absolute inset-y-0 left-0 z-30 shadow-[var(--shadow-lg)]",
          )}
        >
          {/* 2-Tab Segmented Control: Ventanas | Bus */}
          <div className="border-b border-line bg-inset/60 p-2">
            <div className="flex items-center gap-1.5">
              <div role="tablist" aria-label="Modo del panel lateral" className="grid flex-1 grid-cols-2 gap-1 rounded-lg border border-line bg-inset p-0.5">
                <button
                  type="button"
                  role="tab"
                  aria-selected={asideTab === "ventanas"}
                  onClick={() => setAsideTab("ventanas")}
                  className={cn(
                    "pressable rounded-md px-2 py-1 text-[12.5px] font-medium transition-colors",
                    asideTab === "ventanas" ? "bg-surface text-ink-1 shadow-[var(--shadow-sm)]" : "text-ink-3 hover:text-ink-1",
                  )}
                >
                  Ventanas
                </button>
                <button
                  type="button"
                  role="tab"
                  aria-selected={asideTab === "bus"}
                  onClick={() => setAsideTab("bus")}
                  className={cn(
                    "pressable inline-flex items-center justify-center gap-1.5 rounded-md px-2 py-1 text-[12.5px] font-medium transition-colors",
                    asideTab === "bus" ? "bg-surface text-ink-1 shadow-[var(--shadow-sm)]" : "text-ink-3 hover:text-ink-1",
                  )}
                >
                  <span>Bus</span>
                  {busBadge > 0 && (
                    <span className="rounded-full bg-[var(--sig-work-soft)] px-1.5 py-0.5 font-mono text-[11px] font-semibold tabular leading-none text-work">
                      {busBadge}
                    </span>
                  )}
                </button>
              </div>
              {compact && (
                <button
                  type="button"
                  onClick={() => setAsideOpen(false)}
                  aria-label="Cerrar panel lateral"
                  className="pressable grid size-7 shrink-0 place-items-center rounded-md border border-line bg-surface text-ink-3 hover:text-ink-1"
                >
                  <X className="size-3.5" />
                </button>
              )}
            </div>
          </div>

          {asideTab === "ventanas" ? (
            <>
              <div className="flex items-center justify-between gap-2 border-b border-line px-3 py-2.5">
                <div className="flex min-w-0 items-center gap-2">
                  <FolderGit2 className="size-4 shrink-0 text-ink-3" />
                  <div className="min-w-0">
                    <div className="truncate text-[13.5px] font-semibold text-ink-1">{repo.name}</div>
                    <div className="font-mono text-[12px] tabular text-ink-3">
                      {repo.windows.length}v · {allItems.length} agentes
                    </div>
                  </div>
                </div>
                <button
                  type="button"
                  onClick={onLaunch}
                  title="Lanzar en este repo (L)"
                  className="pressable grid size-7 shrink-0 place-items-center rounded-md border border-line bg-elevated text-ink-1 hover:bg-hover"
                >
                  <Plus className="size-3.5" />
                </button>
              </div>

              <div className="min-h-0 flex-1 overflow-y-auto p-2">
                <div className="flex flex-col gap-1">
                  {repo.windows.map((w) => {
                    const isCurrent = w.id === win.id
                    const isJoined = !isCurrent && activeIds.has(w.id)
                    const children = flat(w).slice(1) as Card[]
                    return (
                      <div key={w.id} className="flex flex-col gap-0.5">
                        <div
                          draggable={!isCurrent}
                          onDragStart={(e) => startDragWin(e, w.id)}
                          className={cn(
                            "group flex w-full items-center gap-1 rounded-md pr-1",
                            isCurrent ? "bg-active font-medium text-ink-1" : isJoined ? "bg-elevated/70 text-ink-1" : "text-ink-2 hover:bg-hover hover:text-ink-1",
                          )}
                        >
                          <button
                            type="button"
                            onClick={() => (isCurrent || isJoined ? handleOpen(w.id) : onPickWin(w.id))}
                            className="pressable flex min-w-0 flex-1 items-center gap-2 px-2 py-1.5 text-left text-[13px]"
                          >
                            <SignalBar tone={toneOf(w)} paused={w.paused} />
                            <Brand kind={kindOf(w)} model={w.model} className="size-3.5" />
                            <span className="min-w-0 flex-1 truncate">{nameOf(w)}</span>
                            <span className="shrink-0 font-mono text-[12px] tabular text-ink-3">
                              {isWork(w) ? "ahora" : <Age t={w.updated} />}
                            </span>
                          </button>
                          {!isCurrent && (
                            <button
                              type="button"
                              onClick={() => toggleJoin(w.id)}
                              title={isJoined ? "Quitar de la red visual" : "Unir a esta red visual (o arrastra al lienzo)"}
                              aria-label={isJoined ? "Quitar de la red visual" : "Unir a esta red visual"}
                              className={cn(
                                "pressable grid size-6 shrink-0 place-items-center rounded",
                                isJoined ? "text-work hover:bg-hover" : "text-ink-3 opacity-60 hover:bg-hover hover:text-ink-1 group-hover:opacity-100",
                              )}
                            >
                              {isJoined ? <Unlink className="size-3" /> : <Link2 className="size-3" />}
                            </button>
                          )}
                        </div>
                        {children.map((c) => (
                          <button
                            key={c.id}
                            type="button"
                            onClick={() => handleOpen(c.id)}
                            className={cn(
                              "pressable flex w-full items-center gap-1.5 rounded-md py-1 pr-2 pl-6 text-left text-[12.5px]",
                              openId === c.id ? "bg-active text-ink-1" : "text-ink-2 hover:bg-hover hover:text-ink-1",
                              isFree(c) && "opacity-65",
                            )}
                          >
                            <SignalBar tone={toneOf(c)} paused={c.paused} className="h-2.5" />
                            <Brand kind={kindOf(c)} model={c.model} className="size-3" />
                            <span className="min-w-0 flex-1 truncate">{labelOf(c)}</span>
                            {c.cuenta && !c.who.toLowerCase().includes(c.cuenta.toLowerCase()) && (
                              <span className="shrink-0 rounded border border-line bg-inset px-1.5 py-0.5 text-[11px] font-medium leading-none text-ink-2">
                                {c.cuenta}
                              </span>
                            )}
                          </button>
                        ))}
                      </div>
                    )
                  })}
                </div>

                {otherRepos.length > 0 && (
                  <div className="mt-3 border-t border-line pt-2.5">
                    <div className="mb-1.5 flex items-center justify-between px-2">
                      <span className="font-mono text-[12px] font-semibold text-ink-3">Unir de otros repos</span>
                      <span className="font-mono text-[11px] text-ink-3">arrastra o +</span>
                    </div>
                    <div className="flex flex-col gap-2">
                      {otherRepos.map((r) => (
                        <div key={r.name} className="flex flex-col gap-0.5">
                          <div className="flex items-center gap-1.5 px-2 py-0.5 font-mono text-[12px] font-medium text-ink-3">
                            <FolderGit2 className="size-3 shrink-0" />
                            <span className="truncate">{r.name}</span>
                          </div>
                          {r.windows.map((w) => {
                            const isJoined = activeIds.has(w.id)
                            const subCount = flat(w).length - 1
                            return (
                              <div
                                key={w.id}
                                draggable
                                onDragStart={(e) => startDragWin(e, w.id)}
                                className={cn(
                                  "group flex w-full cursor-grab items-center gap-1 rounded-md pr-1 active:cursor-grabbing",
                                  isJoined ? "bg-elevated/80 text-ink-1 ring-1 ring-[var(--sig-work-border)]" : "text-ink-2 hover:bg-hover hover:text-ink-1",
                                )}
                              >
                                <button
                                  type="button"
                                  onClick={() => (isJoined ? handleOpen(w.id) : joinWin(w.id))}
                                  title={isJoined ? "En esta red · clic para inspeccionar" : "Clic o arrastra al lienzo para unir con sus subagentes"}
                                  className="pressable flex min-w-0 flex-1 items-center gap-1.5 px-1.5 py-1.5 text-left text-[12.5px]"
                                >
                                  <GripVertical className="size-3 shrink-0 text-ink-3/60" />
                                  <SignalBar tone={toneOf(w)} paused={w.paused} />
                                  <Brand kind={kindOf(w)} model={w.model} className="size-3.5" />
                                  <span className="min-w-0 flex-1 truncate">{nameOf(w)}</span>
                                  {subCount > 0 && <span className="shrink-0 font-mono text-[11px] tabular text-ink-3">+{subCount}</span>}
                                </button>
                                <button
                                  type="button"
                                  onClick={() => toggleJoin(w.id)}
                                  title={isJoined ? "Desunir de esta red" : "Unir a esta red"}
                                  aria-label={isJoined ? "Desunir de esta red" : "Unir a esta red"}
                                  className={cn(
                                    "pressable grid size-6 shrink-0 place-items-center rounded",
                                    isJoined ? "text-work hover:bg-hover" : "text-ink-3 hover:bg-hover hover:text-ink-1",
                                  )}
                                >
                                  {isJoined ? <X className="size-3" /> : <Link2 className="size-3" />}
                                </button>
                              </div>
                            )
                          })}
                        </div>
                      ))}
                    </div>
                  </div>
                )}
              </div>
            </>
          ) : (
            /* Sesiones Bus Tab */
            <div className="flex min-h-0 flex-1 flex-col">
              <div className="flex items-center justify-between border-b border-line px-3 py-2">
                <div className="flex items-center gap-1.5 font-mono text-[12px] text-ink-2">
                  <Radio className="size-3.5 text-work" />
                  <span>{busState.peers.length} peers</span>
                  <span className="text-ink-3">/</span>
                  <span>{busState.threads.length} hilos</span>
                </div>
                <button
                  type="button"
                  onClick={refreshBus}
                  disabled={busLoading}
                  title="Actualizar bus"
                  aria-label="Actualizar bus"
                  className="pressable grid size-7 place-items-center rounded-md border border-line bg-elevated text-ink-2 hover:text-ink-1 disabled:opacity-50"
                >
                  <RefreshCw className={cn("size-3.5", busLoading && "animate-spin")} />
                </button>
              </div>

              <div className="min-h-0 flex-1 space-y-4 overflow-y-auto p-2.5">
                {/* Active Peers grouped by Repo */}
                <div>
                  <div className="mb-1.5 px-1 font-mono text-[12px] font-semibold text-ink-3">Peers conectados</div>
                  {peersByRepo.length === 0 ? (
                    <p className="rounded-lg border border-dashed border-line px-3 py-3 text-[12.5px] text-ink-3">
                      Sin peers registrados en <code className="font-mono">bus.db</code>.
                    </p>
                  ) : (
                    <div className="space-y-2.5">
                      {peersByRepo.map(([repoKey, peers]) => (
                        <div key={repoKey} className="space-y-1">
                          <div className="flex items-center gap-1.5 px-1 font-mono text-[12px] font-medium text-ink-3">
                            <FolderGit2 className="size-3 shrink-0" />
                            <span className="truncate">{repoKey}</span>
                          </div>
                          {peers.map((p) => {
                            const pk = providerKind(p.provider)
                            const trLabel = transportLabel(p.transport)
                            const pushReady = Boolean(p.push_ready)
                            return (
                              <div
                                key={p.peer_id}
                                className="flex items-center gap-2 rounded-lg border border-line bg-inset/60 px-2.5 py-2"
                              >
                                <span
                                  aria-hidden="true"
                                  title={pushReady ? "Despertar inmediato (push_ready)" : "Solo buzón (bus-inbox)"}
                                  className={cn("size-2 shrink-0 rounded-full", pushReady ? "bg-ok" : "bg-idle/60")}
                                />
                                {pk ? <Brand kind={pk} className="size-3.5 shrink-0" /> : <Terminal className="size-3.5 shrink-0 text-ink-3" />}
                                <div className="min-w-0 flex-1">
                                  <div className="truncate text-[13px] font-medium text-ink-1">
                                    {p.name || p.title || p.peer_id}
                                  </div>
                                  <div className="flex items-center gap-1.5 font-mono text-[11.5px] text-ink-3">
                                    <span className="rounded border border-line bg-surface px-1 py-0.5 text-[11px] leading-none text-ink-2">
                                      {trLabel}
                                    </span>
                                    {p.branch && <span className="truncate">{p.branch}</span>}
                                  </div>
                                </div>
                                <button
                                  type="button"
                                  onClick={() => {
                                    setActiveThreadId(null)
                                    setNewPeerTarget(p)
                                    if (compact) setAsideOpen(false)
                                  }}
                                  title={`Nuevo hilo para ${p.name || p.peer_id}`}
                                  aria-label={`Nuevo hilo para ${p.name || p.peer_id}`}
                                  className="pressable grid size-7 shrink-0 place-items-center rounded-md border border-line bg-surface text-ink-2 hover:border-line-strong hover:text-ink-1"
                                >
                                  <MessageSquarePlus className="size-3.5" />
                                </button>
                              </div>
                            )
                          })}
                        </div>
                      ))}
                    </div>
                  )}
                </div>

                {/* Bus Threads */}
                <div className="border-t border-line pt-3">
                  <div className="mb-1.5 px-1 font-mono text-[12px] font-semibold text-ink-3">Hilos recientes</div>
                  {busState.threads.length === 0 ? (
                    <p className="rounded-lg border border-dashed border-line px-3 py-3 text-[12.5px] text-ink-3">
                      Ningún hilo abierto todavía.
                    </p>
                  ) : (
                    <div className="space-y-1.5">
                      {busState.threads.map((t) => {
                        const msgs = msgsByThread.get(t.thread_id) ?? []
                        const first = msgs[0]
                        const unread = msgs.some((m) => !m.read_at)
                        const selected = activeThreadId === t.thread_id && !newPeerTarget
                        return (
                          <button
                            key={t.thread_id}
                            type="button"
                            onClick={() => {
                              setNewPeerTarget(null)
                              setActiveThreadId(t.thread_id)
                              if (compact) setAsideOpen(false)
                            }}
                            className={cn(
                              "pressable flex w-full flex-col gap-1 rounded-lg border px-2.5 py-2 text-left transition-colors",
                              selected
                                ? "border-[var(--sig-work-border)] bg-[var(--sig-work-soft)]"
                                : "border-line bg-surface hover:border-line-strong hover:bg-hover",
                            )}
                          >
                            <div className="flex items-center gap-1.5">
                              {unread && <span className="size-2 shrink-0 rounded-full bg-work" aria-label="No leído" />}
                              <span className="min-w-0 flex-1 truncate text-[13px] font-medium text-ink-1">
                                {t.topic || "(sin tema)"}
                              </span>
                              <span
                                className={cn(
                                  "shrink-0 rounded border px-1.5 py-0.5 font-mono text-[11px] leading-none",
                                  t.status === "open"
                                    ? "border-[var(--sig-work-border)] bg-[var(--sig-work-soft)] text-work"
                                    : t.status === "exhausted"
                                      ? "border-[var(--sig-bad-border)] bg-[var(--sig-bad-soft)] text-bad"
                                      : "border-line bg-inset text-ink-3",
                                )}
                              >
                                {t.status === "open" ? "Abierto" : t.status === "exhausted" ? "Agotado" : "Cerrado"}
                              </span>
                            </div>
                            <div className="flex items-center justify-between gap-2 font-mono text-[11.5px] text-ink-3">
                              <span className="min-w-0 truncate">
                                {first ? `${first.from_peer} → ${first.to_peer}` : t.created_by}
                              </span>
                              <span className="shrink-0 tabular">
                                {t.hops_left}/{t.max_hops}
                              </span>
                            </div>
                          </button>
                        )
                      })}
                    </div>
                  )}
                </div>
              </div>
            </div>
          )}
        </aside>
      )}

      {/* Blueprint Graph Stage */}
      <div className="relative min-w-0 flex-1">
        <div className="pointer-events-none absolute top-3 left-3 z-10 flex max-w-[calc(100%-160px)] flex-wrap items-center gap-2">
          {!asideOpen && (
            <>
              <button
                type="button"
                onClick={() => {
                  setAsideTab("ventanas")
                  setAsideOpen(true)
                }}
                aria-label="Abrir árbol de ventanas"
                className="pointer-events-auto pressable inline-flex h-8 items-center gap-1.5 rounded-md border border-line-strong bg-surface px-2.5 text-[12px] font-medium text-ink-1 shadow-[var(--shadow-sm)] hover:bg-hover"
              >
                <PanelLeft className="size-3.5 text-ink-2" />
                <span>Ventanas</span>
              </button>
              <button
                type="button"
                onClick={() => {
                  setAsideTab("bus")
                  setAsideOpen(true)
                }}
                className="pointer-events-auto pressable inline-flex h-8 items-center gap-1.5 rounded-md border border-line-strong bg-surface px-2.5 text-[12px] font-medium text-ink-1 shadow-[var(--shadow-sm)] hover:bg-hover"
              >
                <Radio className="size-3.5 text-work" />
                <span>Bus</span>
                {busBadge > 0 && (
                  <span className="rounded-full bg-[var(--sig-work-soft)] px-1.5 py-0.5 font-mono text-[11px] font-semibold tabular leading-none text-work">
                    {busBadge}
                  </span>
                )}
              </button>
            </>
          )}

          <div className="pointer-events-auto inline-flex flex-wrap rounded-md border border-line-strong bg-surface p-0.5 shadow-[var(--shadow-sm)]">
            {RUNTIME_FILTERS.map(([k, label]) => (
              <button
                key={k}
                type="button"
                onClick={() => setFil(k)}
                aria-pressed={fil === k}
                className={cn(
                  "pressable inline-flex h-7 items-center gap-1.5 rounded-[5px] px-2.5 text-[12px] font-medium",
                  fil === k ? "bg-elevated text-ink-1 shadow-[var(--shadow-sm)]" : "text-ink-3 hover:text-ink-1",
                )}
              >
                {k !== "all" && <Brand kind={k} className="size-3.5" />}
                {label}
              </button>
            ))}
          </div>

          {entries.length > 1 && (
            <div className="pointer-events-auto inline-flex flex-wrap items-center gap-1 rounded-md border border-[var(--sig-work-border)] bg-surface px-2 py-1 text-[12px] shadow-[var(--shadow-sm)]">
              <Link2 className="size-3 shrink-0 text-work" />
              <span className="font-medium text-ink-1">Red unida ({entries.length})</span>
              {entries.slice(1).map((e) => (
                <span
                  key={e.win.id}
                  className="inline-flex items-center gap-1 rounded border border-line bg-inset px-1.5 py-0.5 font-mono text-[11px] text-ink-2"
                >
                  <span className="max-w-[110px] truncate">{e.repoName}</span>
                  <button
                    type="button"
                    onClick={() => detachWin(e.win.id)}
                    aria-label={`Quitar ${nameOf(e.win)}`}
                    className="text-ink-3 hover:text-ink-1"
                  >
                    <X className="size-3" />
                  </button>
                </span>
              ))}
              <button
                type="button"
                onClick={() => saveJoined([])}
                className="pressable ml-1 rounded px-1.5 py-0.5 text-[12px] text-ink-3 hover:bg-hover hover:text-ink-1"
              >
                Separar
              </button>
            </div>
          )}
        </div>

        <ReactFlowProvider>
          <GraphCanvas
            key={canvasKey}
            entries={entries}
            need={need}
            work={work}
            sel={openId}
            dark={dark}
            busState={busState}
            onOpen={handleOpen}
            onOpenBusThread={openBusFromCanvas}
            onDetach={detachWin}
            onDropWin={joinWin}
          />
        </ReactFlowProvider>

        {/* Floating Bus Thread Viewer / New Thread Composer */}
        {(activeThread || newPeerTarget) && (
          <div className="pointer-events-none absolute bottom-20 left-3 z-20 flex justify-start">
            <BusThreadViewer
              key={activeThread ? `t:${activeThread.thread_id}` : `p:${newPeerTarget?.peer_id ?? ""}`}
              thread={activeThread}
              messages={activeThreadMsgs}
              newPeer={newPeerTarget}
              demoMode={demoMode}
              onClose={() => {
                setActiveThreadId(null)
                setNewPeerTarget(null)
              }}
              onRefresh={refreshBus}
              onLocalAdd={(msg, newTh, closeThread) => {
                setDemoBusExtras((prev) => ({
                  threads: newTh ? [...prev.threads, newTh] : prev.threads,
                  messages: [...prev.messages, msg],
                  closedIds: closeThread && activeThread ? [...prev.closedIds, activeThread.thread_id] : prev.closedIds,
                }))
                if (newTh) {
                  setNewPeerTarget(null)
                  setActiveThreadId(newTh.thread_id)
                }
              }}
            />
          </div>
        )}

        <div className="pointer-events-none absolute inset-x-0 bottom-0 px-4 pb-3.5">
          <WindowComposer win={composerWin} demoMode={demoMode} />
        </div>
      </div>
    </div>
  )
}
