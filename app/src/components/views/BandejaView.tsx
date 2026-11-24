import { useEffect, useMemo, useRef, useState } from "react"
import { ArrowRight, CheckCircle2, MessageSquareWarning } from "lucide-react"
import { Age, SignalBar } from "@/components/bits"
import { Brand } from "@/components/brand"
import {
  isWork,
  kindOf,
  useBus,
  useReducedMotion,
  type BusMessage,
  type BusState,
  type BusThread,
  type Repo,
} from "@/lib/agencia"
import { flat, labelOf, lineOf, toneOf, type Item } from "@/lib/tone"
import { cn } from "@/lib/utils"

export type Entry = { it: Item; repo: string }

const entries = (repos: Repo[]): Entry[] =>
  repos.flatMap((r) => r.windows.flatMap(flat).map((it) => ({ it, repo: r.name })))
const oldest = (a: Entry, b: Entry) => a.it.updated - b.it.updated

export function lanesOf(repos: Repo[]) {
  const all = entries(repos)
  return {
    need: all.filter((e) => toneOf(e.it) === "need").sort(oldest),
    bad: all.filter((e) => toneOf(e.it) === "bad").sort(oldest),
    stalled: all.filter((e) => e.it.status === "stalled").sort(oldest),
    work: all.filter((e) => isWork(e.it)).length,
    total: all.length,
  }
}

type LaneKey = "need" | "bad" | "stalled"

export function BandejaView({
  repos,
  sel,
  onSel,
  onConsola,
  bus,
  onOpenRed,
  demoMode,
}: {
  repos: Repo[]
  sel: string | null
  onSel: (id: string | null) => void
  onConsola: () => void
  bus?: BusState
  onOpenRed?: () => void
  demoMode?: boolean
}) {
  const { need, bad, stalled, work, total } = useMemo(() => lanesOf(repos), [repos])
  const order = useMemo(() => [...need, ...bad, ...stalled], [need, bad, stalled])
  const { bus: liveBus } = useBus(bus, demoMode)
  const reduced = useReducedMotion()

  const [highlightLane, setHighlightLane] = useState<LaneKey | null>(null)
  const needRef = useRef<HTMLDivElement>(null)
  const badRef = useRef<HTMLDivElement>(null)
  const stalledRef = useRef<HTMLDivElement>(null)
  const timerRef = useRef<number>(0)

  useEffect(() => () => clearTimeout(timerRef.current), [])

  const escalatedBus = useMemo(() => {
    const msgsByThread = new Map<string, BusMessage[]>()
    for (const m of liveBus.messages) {
      const list = msgsByThread.get(m.thread_id)
      if (list) list.push(m)
      else msgsByThread.set(m.thread_id, [m])
    }
    const items: { thread: BusThread; last?: BusMessage; forOwner: boolean }[] = []
    for (const t of liveBus.threads) {
      const msgs = (msgsByThread.get(t.thread_id) ?? []).slice().sort((a, b) => a.seq - b.seq)
      const forOwner = t.status !== "done" && msgs.some((m) => m.to_peer.toLowerCase() === "owner")
      if (t.status === "exhausted" || forOwner) {
        items.push({ thread: t, last: msgs[msgs.length - 1], forOwner })
      }
    }
    return items
  }, [liveBus])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (document.querySelector("[data-agshot-viewer]")) return
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement || e.metaKey || e.ctrlKey) return
      if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return
      e.preventDefault()
      const i = order.findIndex((x) => x.it.id === sel)
      const j = Math.min(order.length - 1, Math.max(0, i + (e.key === "ArrowDown" ? 1 : -1)))
      if (order[j]) onSel(order[j].it.id)
    }
    addEventListener("keydown", onKey)
    return () => removeEventListener("keydown", onKey)
  }, [order, sel, onSel])

  const jumpToLane = (lane: LaneKey) => {
    const el = lane === "need" ? needRef.current : lane === "bad" ? badRef.current : stalledRef.current
    if (!el) return
    el.scrollIntoView({ behavior: reduced ? "auto" : "smooth", block: "start" })
    setHighlightLane(lane)
    clearTimeout(timerRef.current)
    timerRef.current = window.setTimeout(() => setHighlightLane(null), 1300)
  }

  const [lead, ...restNeed] = need

  const headline =
    need.length > 0
      ? need.length === 1
        ? "1 decisión te espera"
        : `${need.length} decisiones te esperan`
      : bad.length > 0
        ? bad.length === 1
          ? "1 agente falló"
          : `${bad.length} agentes fallaron`
        : stalled.length > 0
          ? stalled.length === 1
            ? "1 agente sin señal"
            : `${stalled.length} agentes sin señal`
          : escalatedBus.length > 0
            ? escalatedBus.length === 1
              ? "1 hilo del bus requiere atención"
              : `${escalatedBus.length} hilos del bus requieren atención`
            : "Todo despejado"

  return (
    <div className="ag-grad-canvas flex h-full flex-col overflow-x-hidden overflow-y-auto">
      {/* Editorial Header Strip */}
      <div className="ag-grad-surface border-b border-line px-4 py-4 sm:px-6 sm:py-5">
        <div className="flex flex-wrap items-end justify-between gap-3 sm:gap-4">
          <div className="min-w-0">
            <h1 className="text-balance text-[22px] font-semibold tracking-tight text-ink-1 sm:text-[26px]">{headline}</h1>
            <p className="mt-1.5 text-[14px] text-ink-2">
              {work === 0 ? "Ningún agente trabajando" : work === 1 ? "1 agente trabajando" : `${work} agentes trabajando`} de{" "}
              <span className="font-mono tabular text-ink-1">{total}</span> registrados en tus repositorios.
            </p>
          </div>
          <button
            type="button"
            onClick={onConsola}
            className="pressable ag-grad-elevated inline-flex h-10 shrink-0 items-center gap-2 rounded-lg border border-line-strong px-4 text-[14px] font-medium text-ink-1 hover:bg-hover"
          >
            Abrir Consola completa
            <ArrowRight className="size-4 text-ink-2" />
          </button>
        </div>

        {/* Interactive KPI Counters */}
        <div className="mt-4 grid grid-cols-3 gap-2 sm:max-w-md">
          <MetricCell
            label="Te esperan"
            value={need.length}
            tone={need.length > 0 ? "need" : "idle"}
            disabled={need.length === 0}
            onClick={() => jumpToLane("need")}
          />
          <MetricCell
            label="Fallaron"
            value={bad.length}
            tone={bad.length > 0 ? "bad" : "idle"}
            disabled={bad.length === 0}
            onClick={() => jumpToLane("bad")}
          />
          <MetricCell
            label="Sin señal"
            value={stalled.length}
            tone={stalled.length > 0 ? "stalled" : "idle"}
            disabled={stalled.length === 0}
            onClick={() => jumpToLane("stalled")}
          />
        </div>
      </div>

      {/* Priority Queue */}
      <div className="flex flex-1 flex-col gap-5 p-4 sm:gap-6 sm:p-6">
        {lead && (
          <div
            ref={needRef}
            className={cn(
              "scroll-mt-4 rounded-xl transition-shadow duration-200",
              highlightLane === "need" && "ring-2 ring-[var(--sig-need)] ring-offset-2 ring-offset-canvas",
            )}
          >
            <section aria-label="Siguiente prioridad">
              <div className="mb-2 flex items-center justify-between text-[12px] text-ink-3">
                <span className="font-medium text-need">Mayor tiempo esperando</span>
                <span className="font-mono text-[12px]">↑ ↓ navegar</span>
              </div>
              <button
                type="button"
                onClick={() => onSel(sel === lead.it.id ? null : lead.it.id)}
                aria-pressed={sel === lead.it.id}
                className={cn(
                  "pressable ag-grad-need-hero flex w-full flex-col gap-2.5 rounded-xl border p-4 text-left",
                  sel === lead.it.id
                    ? "border-[var(--sig-need-border)]"
                    : "border-line-strong hover:border-[var(--sig-need-border)]",
                )}
              >
                <div className="flex items-center gap-2.5">
                  <SignalBar tone="need" className="h-4" />
                  <Brand kind={kindOf(lead.it)} model={lead.it.model} className="size-4" />
                  <span className="min-w-0 flex-1 truncate text-[14px] font-semibold text-ink-1">{labelOf(lead.it)}</span>
                  <span className="max-w-[120px] shrink-0 truncate rounded border border-line bg-inset px-1.5 py-0.5 font-mono text-[12px] text-ink-2">
                    {lead.repo}
                  </span>
                  <span className="ml-auto shrink-0 font-mono text-[12px] tabular text-need">
                    <Age t={lead.it.updated} />
                  </span>
                </div>
                <p className="text-[14px] leading-snug text-ink-1">{lineOf(lead.it)}</p>
              </button>
            </section>
          </div>
        )}

        <QueueLane title="También te esperan" list={restNeed} sel={sel} onSel={onSel} />
        <div
          ref={badRef}
          className={cn(
            "scroll-mt-4 rounded-xl transition-shadow duration-200",
            highlightLane === "bad" && "ring-2 ring-[var(--sig-bad)] ring-offset-2 ring-offset-canvas",
          )}
        >
          <QueueLane title="Fallaron" list={bad} sel={sel} onSel={onSel} />
        </div>
        <div
          ref={stalledRef}
          className={cn(
            "scroll-mt-4 rounded-xl transition-shadow duration-200",
            highlightLane === "stalled" && "ring-2 ring-ink-2 ring-offset-2 ring-offset-canvas",
          )}
        >
          <QueueLane title="Sin señal" list={stalled} sel={sel} onSel={onSel} />
        </div>

        {escalatedBus.length > 0 && (
          <section aria-label="Escalaciones del bus" className="flex flex-col gap-2">
            <div className="flex items-center gap-2 text-[13px] font-semibold text-ink-2">
              <MessageSquareWarning className="size-4 text-need" />
              <span>Bus de sesiones</span>
              <span className="rounded bg-inset px-1.5 py-0.5 font-mono text-[12px] tabular text-ink-3">
                {escalatedBus.length}
              </span>
            </div>
            <div className="divide-y divide-line overflow-hidden rounded-xl border border-line bg-surface">
              {escalatedBus.map(({ thread, last, forOwner }) => {
                const exhausted = thread.status === "exhausted"
                return (
                  <button
                    key={thread.thread_id}
                    type="button"
                    onClick={() => onOpenRed?.()}
                    className="pressable flex w-full flex-col gap-1.5 px-3.5 py-3 text-left transition-colors hover:bg-hover"
                  >
                    <div className="flex items-center gap-2">
                      <span
                        className={cn(
                          "shrink-0 rounded border px-1.5 py-0.5 font-mono text-[12px] font-medium leading-none",
                          exhausted
                            ? "border-[var(--sig-bad-border)] bg-[var(--sig-bad-soft)] text-bad"
                            : "border-[var(--sig-need-border)] bg-[var(--sig-need-soft)] text-need",
                        )}
                      >
                        {exhausted ? "Hops agotados" : "Para OWNER"}
                      </span>
                      <span className="min-w-0 flex-1 truncate text-[13.5px] font-medium text-ink-1">
                        {thread.topic || "(sin tema)"}
                      </span>
                      <span className="shrink-0 font-mono text-[12px] tabular text-ink-3">
                        {thread.hops_left}/{thread.max_hops} hops
                      </span>
                      {onOpenRed && (
                        <span className="shrink-0 rounded border border-line bg-inset px-2 py-0.5 text-[12px] font-medium text-ink-2">
                          Abrir en Red
                        </span>
                      )}
                    </div>
                    {last && (
                      <p className="line-clamp-2 text-[13px] text-ink-2">
                        <span className="font-mono text-[12px] text-ink-3">{last.from_peer}: </span>
                        {last.body}
                      </p>
                    )}
                    {!last && forOwner && (
                      <p className="text-[13px] text-ink-3">Hilo dirigido a OWNER pendiente de respuesta en Red.</p>
                    )}
                  </button>
                )
              })}
            </div>
          </section>
        )}

        {!order.length && !escalatedBus.length && (
          <div className="flex flex-1 flex-col items-center justify-center rounded-xl border border-dashed border-line bg-surface/50 px-6 py-12 text-center">
            <CheckCircle2 className="mb-2.5 size-7 text-ok" strokeWidth={1.6} />
            <p className="text-[14px] font-medium text-ink-1">Ningún agente requiere intervención ahora</p>
            <p className="mt-1 max-w-sm text-pretty text-[13px] text-ink-3">
              Cuando un agente haga una pregunta, falle o quede bloqueado, aparecerá aquí ordenado por tiempo de espera.
            </p>
          </div>
        )}
      </div>
    </div>
  )
}

function MetricCell({
  label,
  value,
  tone,
  disabled,
  onClick,
}: {
  label: string
  value: number
  tone: "need" | "bad" | "stalled" | "idle"
  disabled?: boolean
  onClick?: () => void
}) {
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      className={cn(
        "ag-grad-metric flex items-baseline justify-between rounded-lg border px-3 py-2 text-left transition-colors",
        !disabled && "pressable hover:border-line-strong",
        tone === "stalled" && value > 0 ? "border-[var(--sig-need-border)]" : "border-line",
      )}
    >
      <span className="text-[12px] text-ink-3">{label}</span>
      <span
        className={cn(
          "font-mono text-[15px] font-semibold tabular",
          tone === "need"
            ? "text-need"
            : tone === "bad"
              ? "text-bad"
              : tone === "stalled"
                ? "text-ink-1"
                : "text-ink-3",
        )}
      >
        {value}
      </span>
    </button>
  )
}

function QueueLane({
  title,
  list,
  sel,
  onSel,
}: {
  title: string
  list: Entry[]
  sel: string | null
  onSel: (id: string | null) => void
}) {
  if (!list.length) return null
  return (
    <section aria-label={title} className="flex flex-col gap-2">
      <div className="flex items-center gap-2 text-[13px] font-semibold text-ink-2">
        <span>{title}</span>
        <span className="rounded bg-inset px-1.5 py-0.5 font-mono text-[12px] tabular text-ink-3">{list.length}</span>
      </div>
      <div className="divide-y divide-line overflow-hidden rounded-xl border border-line bg-surface">
        {list.map(({ it, repo }) => {
          const active = sel === it.id
          const tone = toneOf(it)
          return (
            <button
              key={it.id}
              type="button"
              onClick={() => onSel(active ? null : it.id)}
              aria-pressed={active}
              className={cn(
                "pressable flex w-full flex-col gap-1 px-3.5 py-3 text-left transition-colors",
                active ? "bg-active" : "hover:bg-hover",
              )}
            >
              <div className="flex items-center gap-2">
                <SignalBar tone={tone} paused={it.paused} />
                <Brand kind={kindOf(it)} model={it.model} className="size-3.5" />
                <span className="min-w-0 flex-1 truncate text-[13.5px] font-medium text-ink-1">{labelOf(it)}</span>
                <span className="max-w-[120px] shrink-0 truncate rounded border border-line bg-inset px-1.5 py-0.5 font-mono text-[12px] text-ink-3">
                  {repo}
                </span>
                <span className="ml-auto shrink-0 font-mono text-[12px] tabular text-ink-3">
                  <Age t={it.updated} />
                </span>
              </div>
              <p className="line-clamp-2 pl-3 text-[13px] text-ink-2">{lineOf(it)}</p>
            </button>
          )
        })}
      </div>
    </section>
  )
}
