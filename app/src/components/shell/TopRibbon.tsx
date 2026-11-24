import { useCallback, useEffect, useRef, useState } from "react"
import { FlaskConical, PanelLeft, Radio } from "lucide-react"
import { Brand } from "@/components/brand"
import { QuotaPopover } from "@/components/shell/QuotaPopover"
import type { View } from "@/components/shell/NavRail"
import { isWork, workingBy, type LiveStatus, type Quota, type Repo } from "@/lib/agencia"
import { flat, toneOf } from "@/lib/tone"
import { cn } from "@/lib/utils"

export function TopRibbon({
  view,
  repos,
  repoName,
  onSelectRepo,
  quota,
  live,
  demoMode,
  onToggleDemo,
  showTreeToggle,
  treeOpen,
  onToggleTree,
}: {
  view: View
  repos: Repo[]
  repoName: string | null
  onSelectRepo: (name: string) => void
  quota?: Quota
  live: LiveStatus
  demoMode: boolean
  onToggleDemo: () => void
  showTreeToggle: boolean
  treeOpen: boolean
  onToggleTree: () => void
}) {
  const counts = workingBy(repos)
  const allowAll = view === "consola" || view === "tablero"
  const allSelected = allowAll && repoName === "*"
  const tablistRef = useRef<HTMLDivElement>(null)
  const [overflow, setOverflow] = useState({ left: false, right: false })

  const checkOverflow = useCallback(() => {
    const el = tablistRef.current
    if (!el) return
    const left = el.scrollLeft > 2
    const right = el.scrollLeft + el.clientWidth < el.scrollWidth - 2
    setOverflow((p) => (p.left === left && p.right === right ? p : { left, right }))
  }, [])

  useEffect(() => {
    const el = tablistRef.current
    if (!el) return
    checkOverflow()
    const ro = new ResizeObserver(checkOverflow)
    ro.observe(el)
    return () => ro.disconnect()
  }, [checkOverflow, repos.length, view])

  const fadeMask =
    overflow.left && overflow.right
      ? "linear-gradient(to right, transparent, black 20px, black calc(100% - 20px), transparent)"
      : overflow.right
        ? "linear-gradient(to right, black, black calc(100% - 24px), transparent)"
        : overflow.left
          ? "linear-gradient(to right, transparent, black 24px, black)"
          : undefined

  const tally = (r: Repo) => {
    const all = r.windows.flatMap(flat)
    return {
      wait: all.filter((x) => toneOf(x) === "need").length,
      work: all.filter(isWork).length,
    }
  }

  return (
    <header className="ag-grad-ribbon flex h-12 shrink-0 items-center gap-2 border-b border-line px-3 md:h-14 md:gap-3 md:px-4">
      {showTreeToggle && (
        <button
          type="button"
          onClick={onToggleTree}
          aria-pressed={treeOpen}
          title="Alternar explorador de ventanas (B)"
          className={cn(
            "pressable grid size-9 shrink-0 place-items-center rounded-lg border",
            treeOpen
              ? "border-line-strong bg-elevated text-ink-1"
              : "border-line bg-surface text-ink-2 hover:bg-hover hover:text-ink-1",
          )}
        >
          <PanelLeft className="size-4" />
        </button>
      )}

      {/* Repository Switcher */}
      {view !== "bandeja" && view !== "slices" ? (
        <div
          ref={tablistRef}
          onScroll={checkOverflow}
          role="tablist"
          aria-label="Repositorios"
          style={fadeMask ? { maskImage: fadeMask, WebkitMaskImage: fadeMask } : undefined}
          className="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
        >
          {allowAll && (
            <button
              type="button"
              role="tab"
              aria-selected={allSelected}
              onClick={() => onSelectRepo("*")}
              className={cn(
                "pressable inline-flex h-9 shrink-0 items-center gap-2 rounded-lg border px-3 text-[14px] font-medium",
                allSelected
                  ? "ag-grad-elevated border-line-strong text-ink-1 shadow-[var(--shadow-sm)]"
                  : "border-transparent text-ink-2 hover:bg-hover hover:text-ink-1",
              )}
            >
              Todos
              <span className="font-mono text-[11px] tabular text-ink-3">{repos.length}</span>
            </button>
          )}
          {repos.map((r, i) => {
            const t = tally(r)
            const active = !allSelected && r.name === repoName
            return (
              <button
                key={r.name}
                type="button"
                role="tab"
                aria-selected={active}
                onClick={() => onSelectRepo(r.name)}
                title={`${r.name} (tecla ${i + 1})`}
                className={cn(
                  "pressable inline-flex h-9 shrink-0 items-center gap-2 rounded-lg border px-3 text-[14px] font-medium",
                  active
                    ? "ag-grad-elevated border-line-strong text-ink-1 shadow-[var(--shadow-sm)]"
                    : "border-transparent text-ink-2 hover:bg-hover hover:text-ink-1",
                )}
              >
                <span
                  aria-hidden="true"
                  className={cn(
                    "h-2.5 w-[2.5px] rounded-full",
                    t.wait > 0 ? "bg-need" : t.work > 0 ? "bg-work animate-sig-pulse" : "bg-idle/40",
                  )}
                />
                <span className="max-w-[160px] truncate">{r.name}</span>
                {t.wait > 0 && (
                  <span className="rounded bg-[var(--sig-need-soft)] px-1 font-mono text-[10.5px] font-semibold tabular text-need">
                    {t.wait}
                  </span>
                )}
              </button>
            )
          })}
        </div>
      ) : (
        <div className="flex min-w-0 flex-1 items-center gap-2">
          <span className="text-[13px] font-semibold tracking-tight text-ink-1">
            {view === "bandeja" ? "Bandeja de prioridades" : "Agencia Real (Slices)"}
          </span>
          <span className="font-mono text-[11px] text-ink-3">
            {repos.length} {repos.length === 1 ? "repo" : "repos"}
          </span>
        </div>
      )}

      {/* Right Telemetry Strip */}
      <div className="flex shrink-0 items-center gap-2.5">
        {/* Live Fleet Counters */}
        <div
          role="status"
          aria-label="Agentes activos ahora"
          title="Agentes trabajando ahora en todos los repos"
          className="hidden items-center gap-3 rounded-md border border-line bg-inset px-2.5 py-1 lg:flex"
        >
          <span className="inline-flex items-center gap-1.5 text-[12px] text-ink-2">
            <Brand kind="claude" className="size-3.5" />
            <span className="font-mono font-semibold tabular text-ink-1">{counts.claude}</span>
          </span>
          <span className="h-3 w-px bg-line" aria-hidden="true" />
          <span className="inline-flex items-center gap-1.5 text-[12px] text-ink-2">
            <Brand kind="codex" className="size-3.5" />
            <span className="font-mono font-semibold tabular text-ink-1">{counts.codex}</span>
          </span>
          <span className="h-3 w-px bg-line" aria-hidden="true" />
          <span className="inline-flex items-center gap-1.5 text-[12px] text-ink-2">
            <Brand kind="bec" className="size-3.5" />
            <span className="font-mono font-semibold tabular text-ink-1">{counts.opencode}</span>
          </span>
          <span className="h-3 w-px bg-line" aria-hidden="true" />
          <span className="inline-flex items-center gap-1.5 text-[12px] text-ink-2" title="Antigravity (agy)">
            <Brand kind="agy" className="size-3.5" />
            <span className="font-mono font-semibold tabular text-ink-1">{counts.agy}</span>
          </span>
        </div>

        {/* Quota Telemetry Popover */}
        <QuotaPopover quota={quota} />

        {/* Live vs Demo Switch */}
        {import.meta.env.VITE_PORTFOLIO_DEMO === "1" ? (
          <span
            title="Vista de demostración con datos sintéticos"
            className="inline-flex h-8 shrink-0 items-center gap-1.5 rounded-lg border border-[var(--sig-need-border)] bg-[var(--sig-need-soft)] px-2.5 font-mono text-[12px] font-medium whitespace-nowrap text-need md:h-9 md:gap-2 md:px-3 md:text-[13px]"
          >
            <FlaskConical className="size-3 shrink-0" />
            <span>Demo</span>
          </span>
        ) : (
          <button
            type="button"
            onClick={onToggleDemo}
            title={demoMode ? "Salir de Modo Demo y volver al Core en vivo" : "Activar datos de demostración para probar el benchmark"}
            className={cn(
              "pressable inline-flex h-9 items-center gap-2 rounded-lg border px-3 font-mono text-[13px] font-medium",
              demoMode
                ? "border-[var(--sig-need-border)] bg-[var(--sig-need-soft)] text-need"
                : live === "sse" || live === "poll"
                  ? "border-line-strong bg-surface text-ink-1 hover:bg-hover"
                  : live === "connecting" || live === "sleep"
                    ? "border-line-strong bg-inset text-ink-2 hover:bg-hover"
                    : "border-[var(--sig-bad-border)] bg-[var(--sig-bad-soft)] text-bad",
            )}
          >
            {demoMode ? (
              <>
                <FlaskConical className="size-3" />
                Demo
              </>
            ) : (
              <>
                <Radio
                  className={cn(
                    "size-3",
                    live === "sse"
                      ? "text-ok"
                      : live === "poll"
                        ? "text-need"
                        : live === "connecting"
                          ? "text-ink-3 animate-sig-pulse"
                          : live === "sleep"
                            ? "text-ink-3"
                            : "text-bad",
                  )}
                />
                {live === "sse"
                  ? "Vivo"
                  : live === "poll"
                    ? "Sondeo"
                    : live === "connecting"
                      ? "Conectando…"
                      : live === "sleep"
                        ? "Dormida"
                        : "Offline"}
              </>
            )}
          </button>
        )}
      </div>
    </header>
  )
}
