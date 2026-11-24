import { useEffect, useMemo, useState } from "react"
import { X } from "lucide-react"
import { Dialog } from "radix-ui"
import { Age } from "@/components/bits"
import { page, tok, type Activity } from "@/lib/agencia"
import { buildDemoActivity } from "@/lib/demo"
import { modelOf } from "@/lib/tone"

function StackedBars({ rows }: { rows: { name: string; in: number; out: number; cache?: number; agents: number }[] }) {
  const max = Math.max(1, ...rows.map((r) => r.in + r.out + (r.cache ?? 0)))
  return (
    <div className="flex flex-col gap-2.5">
      {rows.map((r) => (
        <div key={r.name} className="flex flex-col gap-1">
          <div className="flex items-center justify-between gap-2 font-mono text-[11.5px]">
            <span className="truncate font-medium text-ink-1" title={r.name}>
              {modelOf(r.name) || r.name}
            </span>
            <span className="shrink-0 tabular text-ink-3">
              {tok(r.in)} in · {tok(r.out)} out{r.cache ? ` · ${tok(r.cache)} caché` : ""}
            </span>
          </div>
          <div className="flex h-2 overflow-hidden rounded-sm bg-inset">
            {!!r.cache && <div className="bg-work/30" style={{ width: `${(r.cache / max) * 100}%` }} />}
            <div className="bg-work" style={{ width: `${(r.in / max) * 100}%` }} />
            <div className="bg-ok" style={{ width: `${(r.out / max) * 100}%` }} />
          </div>
        </div>
      ))}
    </div>
  )
}

export function ActivityDrawer({
  open,
  onOpenChange,
  onOpenAgent,
  demoMode,
}: {
  open: boolean
  onOpenChange: (o: boolean) => void
  onOpenAgent: (id: string) => void
  demoMode?: boolean
}) {
  const [fetched, setFetched] = useState<Activity | null>(null)
  const demoActivity = useMemo(() => (demoMode && open ? buildDemoActivity() : null), [demoMode, open])
  const a = demoMode ? demoActivity : fetched

  useEffect(() => {
    if (!open || demoMode) return
    let dead = false
    const load = () =>
      fetch("/api/activity")
        .then((r) => r.json())
        .then((j) => !dead && setFetched(j))
        .catch(() => {})
    load()
    const t = setInterval(() => !page.asleep && load(), 5000)
    return () => {
      dead = true
      clearInterval(t)
    }
  }, [open, demoMode])

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="anim-overlay fixed inset-0 z-50 bg-black/55" />
        <Dialog.Content className="anim-drawer fixed top-0 right-0 bottom-0 z-50 flex w-[min(94vw,660px)] flex-col border-l border-line-strong bg-surface shadow-[var(--shadow-lg)]">
          <header className="flex items-start justify-between gap-3 border-b border-line bg-elevated px-5 py-4">
            <div>
              <Dialog.Title className="text-[15px] font-semibold text-ink-1">Actividad de tokens (24h)</Dialog.Title>
              <Dialog.Description className="mt-0.5 text-[12.5px] text-ink-3">
                Distribución por modelo, cuenta y agente. Azul = entrada, verde = salida, atenuado = caché.
              </Dialog.Description>
            </div>
            <Dialog.Close asChild>
              <button
                type="button"
                aria-label="Cerrar actividad"
                className="pressable grid size-7 place-items-center rounded-md border border-line bg-surface text-ink-3 hover:bg-hover hover:text-ink-1"
              >
                <X className="size-4" />
              </button>
            </Dialog.Close>
          </header>

          <div className="min-h-0 flex-1 overflow-y-auto p-5">
            {!a ? (
              <p className="text-[13px] text-ink-3">Cargando telemetría de consumo…</p>
            ) : (
              <div className="flex flex-col gap-6">
                <section>
                  <h3 className="mb-2.5 text-[12.5px] font-semibold text-ink-2">Por modelo</h3>
                  <StackedBars rows={a.by_model.map((m) => ({ name: m.model || "?", ...m }))} />
                </section>
                <section className="border-t border-line pt-5">
                  <h3 className="mb-2.5 text-[12.5px] font-semibold text-ink-2">Por cuenta</h3>
                  <StackedBars rows={a.by_account.map((m) => ({ name: m.account || "?", ...m }))} />
                </section>
                <section className="border-t border-line pt-5">
                  <h3 className="mb-2.5 text-[12.5px] font-semibold text-ink-2">Por agente</h3>
                  <div className="overflow-x-auto rounded-lg border border-line">
                    <table className="w-full border-collapse text-left text-[12px]">
                      <thead>
                        <tr className="border-b border-line bg-inset text-ink-3">
                          <th className="px-3 py-2 font-medium">Agente</th>
                          <th className="px-3 py-2 font-medium">Modelo</th>
                          <th className="px-3 py-2 text-right font-medium">In</th>
                          <th className="px-3 py-2 text-right font-medium">Out</th>
                          <th className="px-3 py-2 text-right font-medium">Hace</th>
                        </tr>
                      </thead>
                      <tbody className="divide-y divide-line-subtle">
                        {a.agents.slice(0, 40).map((g) => {
                          const label = g.id.startsWith("sub:") || g.who === "Becario" ? g.title : `${g.who} — ${g.title}`
                          return (
                            <tr
                              key={g.id}
                              onClick={() => {
                                onOpenChange(false)
                                onOpenAgent(g.id)
                              }}
                              className="cursor-pointer hover:bg-hover"
                            >
                              <td className="max-w-[200px] truncate px-3 py-2 text-ink-1" title={label}>
                                {label}
                              </td>
                            <td className="px-3 py-2 font-mono text-[11px] text-ink-3">{modelOf(g.model) || "—"}</td>
                            <td className="px-3 py-2 text-right font-mono tabular text-ink-2">{tok(g.in)}</td>
                            <td className="px-3 py-2 text-right font-mono tabular text-ink-2">{tok(g.out)}</td>
                            <td className="px-3 py-2 text-right font-mono tabular text-ink-3">
                              <Age t={g.updated} />
                            </td>
                          </tr>
                          )
                        })}
                      </tbody>
                    </table>
                  </div>
                </section>
              </div>
            )}
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}
