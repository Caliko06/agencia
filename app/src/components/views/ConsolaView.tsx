import { useEffect, useMemo, useState } from "react"
import { Eraser, Pause } from "lucide-react"
import { toast } from "sonner"
import { Age, SignalBar } from "@/components/bits"
import { Acct, Brand } from "@/components/brand"
import { Shots } from "@/components/shots"
import { isWork, kindOf, post, tok, usd, used, type Card, type Repo, type Win } from "@/lib/agencia"
import { flat, isWin, labelOf, lineOf, modelOf, nameOf, toneOf, type Item } from "@/lib/tone"
import { cn } from "@/lib/utils"

type Row = { it: Item; depth: number; repo: string }
const FILTERS = [
  ["all", "Todos"],
  ["work", "Trabajando"],
  ["need", "Te esperan"],
  ["pause", "En pausa"],
] as const
type Filter = (typeof FILTERS)[number][0]

const passes = (it: Item, f: Filter) =>
  f === "all" || (f === "pause" ? !!it.paused : f === "work" ? isWork(it) : toneOf(it) === "need")

function rowsOf(repos: Repo[], f: Filter): Row[] {
  const out: Row[] = []
  for (const r of repos) {
    for (const w of r.windows.flatMap((x) => [x, ...(x.children.filter((c) => isWin(c as Item)) as unknown as Win[])])) {
      const deep = (c: Card, d: number): Row[] => [...(passes(c, f) ? [{ it: c, depth: d, repo: r.name }] : []), ...(c.children ?? []).flatMap((k) => deep(k, d + 1))]
      const kids = w.children.filter((c) => !isWin(c as Item)).flatMap((c) => deep(c, 1))
      const me = passes(w, f)
      if (!me && !kids.length) continue
      out.push({ it: w, depth: 0, repo: r.name }, ...kids)
    }
  }
  return out
}

export function ConsolaView({
  repos,
  all,
  sel,
  demoMode,
  onSel,
}: {
  repos: Repo[]
  all: boolean
  sel: string | null
  demoMode?: boolean
  onSel: (id: string | null) => void
}) {
  const [f, setF] = useState<Filter>("all")
  const [arm, setArm] = useState(false)
  const [busy, setBusy] = useState(false)

  const rows = useMemo(() => rowsOf(repos, f), [repos, f])
  const everything = useMemo(() => repos.flatMap((r) => r.windows.flatMap(flat)), [repos])
  const count = (k: Filter) => everything.filter((x) => passes(x, k)).length
  const safe = everything.filter((x) => x.safe)

  useEffect(() => {
    if (!arm) return
    const h = setTimeout(() => setArm(false), 3000)
    return () => clearTimeout(h)
  }, [arm])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (document.querySelector("[data-agshot-viewer]")) return
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement || e.metaKey || e.ctrlKey) return
      if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return
      e.preventDefault()
      const i = rows.findIndex((r) => r.it.id === sel)
      const j = Math.min(rows.length - 1, Math.max(0, i + (e.key === "ArrowDown" ? 1 : -1)))
      if (rows[j]) onSel(rows[j].it.id)
    }
    addEventListener("keydown", onKey)
    return () => removeEventListener("keydown", onKey)
  }, [rows, sel, onSel])

  async function sweep() {
    if (demoMode) {
      toast.info("Limpieza global simulada en Modo Demo")
      return
    }
    if (!arm) {
      setArm(true)
      return
    }
    setArm(false)
    setBusy(true)
    const r = await post("/api/control", { id: "*", action: "sweep" })
    setBusy(false)
    if (r.ok) toast.success(r.info || "Limpieza completada")
    else toast.error("No se pudo limpiar", { description: r.info })
  }

  let lastRepo = ""

  return (
    <div className="@container/consola flex h-full flex-col bg-canvas">
      {/* Toolbar */}
      <div className="flex flex-wrap items-center gap-2 border-b border-line bg-surface px-3 py-2 sm:px-4 sm:py-2.5">
        <div role="group" aria-label="Filtrar procesos" className="inline-flex max-w-full overflow-x-auto rounded-md border border-line bg-inset p-0.5 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
          {FILTERS.map(([k, label]) => (
            <button
              key={k}
              type="button"
              onClick={() => setF(k)}
              aria-pressed={f === k}
              className={cn(
                "pressable inline-flex h-8 shrink-0 items-center gap-1.5 rounded-md px-2.5 text-[12.5px] font-medium whitespace-nowrap sm:h-9 sm:gap-2 sm:px-3 sm:text-[13.5px]",
                f === k ? "ag-grad-elevated text-ink-1 shadow-[var(--shadow-sm)]" : "text-ink-2 hover:text-ink-1",
              )}
            >
              {label}
              <span className="font-mono text-[12px] tabular-nums text-ink-3">{count(k)}</span>
            </button>
          ))}
        </div>

        <button
          type="button"
          disabled={busy}
          onClick={sweep}
          title={
            safe.length
              ? "Cierra procesos listos y huérfanos:\n" + safe.map((x) => `• ${x.title}: ${x.safe}`).join("\n")
              : "Limpia procesos huérfanos de agentes cerrados"
          }
          className={cn(
            "pressable ml-auto inline-flex h-8 items-center gap-1.5 rounded-md border px-2.5 text-[12px] font-medium disabled:opacity-40",
            arm
              ? "border-[var(--sig-bad-border)] bg-[var(--sig-bad-soft)] text-bad"
              : "border-line bg-surface text-ink-2 hover:border-line-strong hover:bg-hover hover:text-ink-1",
          )}
        >
          <Eraser className="size-3.5" />
          {busy ? "Cerrando…" : arm ? `Confirmar cierre (${safe.length})` : "Limpiar"}
          <span className="font-mono text-[12px] tabular-nums text-ink-3">{safe.length}</span>
        </button>
      </div>

      {/* Column Headers */}
      <div className="grid grid-cols-[minmax(0,1fr)_92px_56px] items-center gap-2 border-b border-line bg-elevated px-3 py-2 text-[12px] font-medium text-ink-3 @[480px]/consola:grid-cols-[minmax(160px,1.4fr)_104px_58px_minmax(130px,1.2fr)] @[480px]/consola:gap-2.5 @[680px]/consola:grid-cols-[minmax(180px,1.5fr)_110px_84px_62px_minmax(160px,1.4fr)] @[680px]/consola:gap-3 @[680px]/consola:px-4">
        <span>Agente</span>
        <span>Modelo</span>
        <span className="hidden text-right @[680px]/consola:block">Tokens</span>
        <span className="text-right">Tiempo</span>
        <span className="hidden @[480px]/consola:block">Estado actual</span>
      </div>

      {/* Process Rows */}
      <div className="min-h-0 flex-1 overflow-y-auto divide-y divide-line-subtle" role="table">
        {rows.map(({ it, depth, repo }) => {
          const showRepo = all && repo !== lastRepo ? (lastRepo = repo) : ""
          const tone = toneOf(it)
          const detail = lineOf(it)
          const active = sel === it.id
          return (
            <div key={it.id}>
              {showRepo && (
                <div className="border-b border-line bg-inset px-4 py-1.5 font-mono text-[12px] font-semibold text-ink-2">
                  {showRepo}
                </div>
              )}
              <div
                role="row"
                tabIndex={0}
                onClick={() => onSel(active ? null : it.id)}
                onKeyDown={(e) => {
                  if (e.target === e.currentTarget && (e.key === "Enter" || e.key === " ")) {
                    e.preventDefault()
                    onSel(active ? null : it.id)
                  }
                }}
                className={cn(
                  "grid cursor-pointer grid-cols-[minmax(0,1fr)_92px_56px] items-center gap-2 px-3 py-2.5 text-[13.5px] transition-colors @[480px]/consola:grid-cols-[minmax(160px,1.4fr)_104px_58px_minmax(130px,1.2fr)] @[480px]/consola:gap-2.5 @[680px]/consola:grid-cols-[minmax(180px,1.5fr)_110px_84px_62px_minmax(160px,1.4fr)] @[680px]/consola:gap-3 @[680px]/consola:px-4",
                  active ? "bg-active" : "bg-surface hover:bg-hover",
                )}
              >
                <div className="flex min-w-0 items-center gap-2" style={depth ? { paddingLeft: `${Math.min(depth, 4) * 1.1}rem` } : undefined}>
                  {it.paused ? (
                    <Pause className="size-3.5 shrink-0 text-ink-3" />
                  ) : (
                    <SignalBar tone={tone} paused={it.paused} />
                  )}
                  <Brand kind={kindOf(it)} model={it.model} className="size-4" />
                  <span className={cn("truncate", depth === 0 ? "font-medium text-ink-1" : "text-ink-2")}>
                    {labelOf(it)}
                  </span>
                  <span className="inline-flex shrink-0">
                    <Acct kind={kindOf(it)} name={isWin(it) ? it.title : it.who} />
                  </span>
                  <Shots shots={it.shots} name={nameOf(it)} />
                </div>

                <span className="truncate font-mono text-[12px] text-ink-3" title={it.model}>
                  {modelOf(it.model) || "—"}
                </span>

                <span className="hidden text-right font-mono text-[12px] tabular-nums text-ink-2 @[680px]/consola:block">
                  {used(it.usage) > 0 ? tok(used(it.usage)) : "—"}
                  {it.usage?.usd ? <span className="ml-1 text-[12px] text-ink-3">{usd(it.usage.usd)}</span> : null}
                </span>

                <span className={cn("text-right font-mono text-[12px] tabular-nums", isWork(it) ? "text-work" : "text-ink-3")}>
                  {isWork(it) ? "ahora" : <Age t={it.updated} />}
                </span>

                <span
                  className={cn(
                    "hidden truncate text-[13.5px] @[480px]/consola:block",
                    tone === "need" ? "font-medium text-need" : tone === "bad" ? "text-bad" : "text-ink-2",
                  )}
                  title={detail || (it.safe ? "Lista para cerrar" : "Libre")}
                >
                  {it.paused ? (
                    "En pausa"
                  ) : tone === "idle" && it.status !== "stalled" ? (
                    detail ? (
                      <>
                        <span className="text-ink-3">{it.safe ? "Lista para cerrar" : "Libre"} · </span>
                        {detail}
                      </>
                    ) : it.safe ? (
                      "Lista para cerrar"
                    ) : (
                      "Libre"
                    )
                  ) : (
                    detail
                  )}
                </span>
              </div>
            </div>
          )
        })}

        {!rows.length && (
          <div className="px-4 py-14 text-center text-[13.5px] text-ink-3">
            {f === "all" ? "No hay ventanas activas en este repositorio." : "Ningún agente coincide con este filtro."}
          </div>
        )}
      </div>
    </div>
  )
}
