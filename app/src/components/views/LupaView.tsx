import { useEffect, useRef, useState } from "react"
import { Age, SignalBar } from "@/components/bits"
import { Brand } from "@/components/brand"
import { kindOf, type Card, type Repo, type Win } from "@/lib/agencia"
import { buildDemoRe } from "@/lib/demo"
import { flat, nameOf, toneOf } from "@/lib/tone"
import { cn } from "@/lib/utils"

type Target = { id: string; img: string; proc: string; addr: string; name: string; area: string }
type Got = { target: Target; code: string; ms: number; created: boolean }

const med = (ms: number) => (ms < 1000 ? `${ms} ms` : `${(ms / 1000).toFixed(1)} s`)

export function LupaView({ repos, repo, onOpen, demoMode }: { repos: Repo[]; repo?: Repo; onOpen: (id: string) => void; demoMode?: boolean }) {
  const [targets, setTargets] = useState<Target[]>([])
  const [sel, setSel] = useState<string | null>(null)
  const [got, setGot] = useState<Got | null>(null)
  const [plain, setPlain] = useState("")
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState("")
  const [loaded, setLoaded] = useState(false)
  const asked = useRef("")

  useEffect(() => {
    if (demoMode) {
      const d = buildDemoRe()
      setTargets(d.targets)
      setSel((s) => s ?? d.targets[0].id)
      setLoaded(true)
      return
    }
    fetch("/api/re")
      .then((r) => r.json())
      .then((j) => {
        setLoaded(true)
        if (!j?.ok || !j.targets?.length) return
        setTargets(j.targets)
        setSel((s) => s ?? j.targets[0].id)
      })
      .catch(() => {
        setLoaded(true)
        setErr("La Agencia no contesta desde el servidor.")
      })
  }, [demoMode])

  useEffect(() => {
    if (!sel || asked.current === sel) return
    asked.current = sel
    if (demoMode) {
      const d = buildDemoRe().details[sel]
      if (d) {
        setGot({ target: d.target, code: d.code, ms: d.ms, created: d.created })
        setPlain(d.plain)
        setBusy(false)
        setErr("")
      }
      return
    }
    let dead = false
    setBusy(true)
    setErr("")
    setGot(null)
    setPlain("")
    const id = encodeURIComponent(sel)
    fetch(`/api/re?id=${id}`)
      .then((r) => r.json())
      .then((j) => {
        if (dead) return
        if (!j?.ok) {
          setErr(j?.info || "Ghidra no devolvió nada para esa dirección.")
          setBusy(false)
          return
        }
        setGot(j as Got)
        setBusy(false)
        fetch(`/api/re/plain?id=${id}`)
          .then((r) => r.json())
          .then((p) => !dead && p?.ok && setPlain(p.plain || ""))
          .catch(() => {})
      })
      .catch(() => {
        if (!dead) {
          setErr("Se cortó la conexión con el servidor.")
          setBusy(false)
        }
      })
    return () => {
      dead = true
    }
  }, [sel, demoMode])

  const pick = (id: string) => {
    asked.current = ""
    setSel(id)
  }

  const live = repos
    .flatMap((r) =>
      r.windows
        .flatMap(flat)
        .filter((x) => x.re?.length)
        .map((x) => ({ x, h: x.re![x.re!.length - 1], repoName: r.name })),
    )
    .sort((a, b) => (a.repoName === repo?.name ? 0 : 1) - (b.repoName === repo?.name ? 0 : 1) || b.h.t - a.h.t)
    .slice(0, 12)
  const manyRepos = new Set(live.map((l) => l.repoName)).size > 1
  const showChip = (name: string) => (repo ? name !== repo.name : manyRepos)

  const lines = got ? got.code.replace(/\n+$/, "").split("\n") : []

  return (
    <div className="flex h-full min-h-0 flex-col bg-canvas md:flex-row">
      {/* Left Index Rail (stacks on top on mobile) */}
      <aside className="flex max-h-[210px] w-full shrink-0 flex-col border-b border-line bg-surface md:max-h-none md:w-[290px] md:border-r md:border-b-0">
        <div className="min-h-0 flex-1 overflow-y-auto p-3">
          <div className="mb-2 flex items-center justify-between text-[12px] font-semibold text-ink-2">
            <span>Actividad binaria (30m)</span>
            <span className="font-mono text-[12px] tabular-nums text-ink-3">{live.length}</span>
          </div>
          {live.length > 0 ? (
            <div className="mb-5 flex flex-col gap-2">
              {live.map(({ x, h, repoName }) => (
                <div key={x.id} className="rounded-lg border border-line bg-inset p-2.5">
                  <button type="button" onClick={() => onOpen(x.id)} className="flex w-full items-center gap-2 text-left">
                    <SignalBar tone={toneOf(x)} className="h-3" />
                    <Brand kind={kindOf(x as Card | Win)} className="size-3.5" />
                    <span className="min-w-0 flex-1 truncate text-[13px] font-medium text-ink-1">{nameOf(x)}</span>
                    {showChip(repoName) && (
                      <span className="shrink-0 rounded border border-line bg-surface px-1 py-0.5 font-mono text-[12px] text-ink-3">
                        {repoName}
                      </span>
                    )}
                    <span className="font-mono text-[12px] tabular-nums text-ink-3">
                      <Age t={h.t} />
                    </span>
                  </button>
                  <p className="mt-1 line-clamp-2 text-[13px] text-ink-2">{h.why}</p>
                  {!!h.addrs.length && (
                    <div className="mt-2 flex flex-wrap gap-1">
                      {h.addrs.map((a) => (
                        <button
                          key={a}
                          type="button"
                          onClick={() => pick(a)}
                          className={cn(
                            "rounded border px-1.5 py-0.5 font-mono text-[12px] tabular-nums",
                            sel === a ? "border-work bg-[var(--sig-work-soft)] text-work" : "border-line bg-surface text-ink-2 hover:text-ink-1",
                          )}
                        >
                          {a}
                        </button>
                      ))}
                    </div>
                  )}
                </div>
              ))}
            </div>
          ) : (
            <p className="mb-5 text-[13px] text-ink-3">Ningún agente registró actividad binaria en los últimos 30 min.</p>
          )}

          <div className="mb-2 flex items-center justify-between border-t border-line pt-3 text-[12px] font-semibold text-ink-2">
            <span>Funciones indexadas</span>
            <span className="font-mono text-[12px] tabular-nums text-ink-3">{targets.length || "—"}</span>
          </div>
          <div className="flex flex-col gap-1">
            {targets.map((t) => (
              <button
                key={t.id}
                type="button"
                onClick={() => pick(t.id)}
                className={cn(
                  "pressable flex flex-col gap-0.5 rounded-md border px-2.5 py-2 text-left",
                  sel === t.id
                    ? "border-line-strong bg-active text-ink-1"
                    : "border-transparent hover:bg-hover text-ink-2",
                )}
              >
                <span className="truncate text-[13px] font-medium text-ink-1">{t.name}</span>
                <span className="flex items-center justify-between gap-2 font-mono text-[12px] tabular-nums text-ink-3">
                  <span className="truncate">{t.area}</span>
                  <span>{t.addr}</span>
                </span>
              </button>
            ))}
            {!targets.length && !err && (
              <p className="py-4 text-[13px] text-ink-3">
                {loaded ? "Sin binario o índice de Ghidra configurado para este espacio." : "Cargando índice de Ghidra…"}
              </p>
            )}
          </div>
        </div>
      </aside>

      {/* Decompiler Pane */}
      <section className="flex min-w-0 flex-1 flex-col overflow-hidden">
        {err && <div className="p-6 text-[13.5px] text-bad">{err}</div>}
        {busy && (
          <div className="flex items-center gap-2.5 p-6 text-[13.5px] text-work" role="status">
            <span className="size-2 rounded-full bg-work animate-sig-pulse" />
            Ghidra está decompilando <span className="font-mono text-[13px] tabular-nums">{targets.find((t) => t.id === sel)?.addr ?? sel}</span>…
          </div>
        )}
        {!busy && !got && !err && loaded && targets.length === 0 && (
          <div className="grid flex-1 place-items-center p-8">
            <div className="flex max-w-md flex-col gap-2 text-center">
              <h3 className="text-[14px] font-semibold text-ink-1">Sin objetivo binario en este espacio</h3>
              <p className="text-[13.5px] leading-relaxed text-ink-2">
                Este workspace no tiene un binario ni un índice de Ghidra configurado, así que no hay funciones que decompilar.
                Si algún agente tocó direcciones (actividad binaria del panel izquierdo), puedes abrirlas igual.
              </p>
            </div>
          </div>
        )}
        {!busy && !got && !err && !(loaded && targets.length === 0) && (
          <div className="grid flex-1 place-items-center p-6 text-[13.5px] text-ink-3">
            Elige una función o dirección del panel izquierdo para decompilarla.
          </div>
        )}
        {got && (
          <>
            <header className="flex flex-wrap items-center justify-between gap-3 border-b border-line bg-surface px-5 py-3.5">
              <div className="min-w-0">
                <h2 className="truncate font-mono text-[15px] font-semibold text-ink-1">{got.target.name}</h2>
                <p className="flex flex-wrap items-center gap-x-2 gap-y-1 text-[12px] text-ink-3">
                  <code className="font-mono text-[12px] tabular-nums text-work">{got.target.addr}</code>
                  {got.target.proc && <span>en {got.target.proc}</span>}
                  {got.target.area && (
                    <span className="rounded border border-line bg-inset px-1.5 py-0.5 font-mono text-[12px] text-ink-2">
                      {got.target.area}
                    </span>
                  )}
                </p>
              </div>
              <div className="flex items-center gap-2 font-mono text-[12px] tabular-nums">
                {got.created && (
                  <span className="rounded border border-[var(--sig-need-border)] bg-[var(--sig-need-soft)] px-2 py-0.5 text-need">
                    Creada por Ghidra
                  </span>
                )}
                <span className="rounded border border-line bg-inset px-2 py-0.5 text-ink-2 tabular-nums">
                  {lines.length} líneas · {got.ms ? med(got.ms) : "caché"}
                </span>
              </div>
            </header>

            <div className="border-b border-line bg-elevated px-5 py-3.5">
              <h3 className="text-[12.5px] font-semibold text-ink-2">Explicación en llano</h3>
              {plain ? (
                <div className="mt-1.5 flex flex-col gap-1.5 text-[13.5px] leading-relaxed text-ink-1">
                  {plain
                    .split("\n")
                    .map((p) => p.trim())
                    .filter(Boolean)
                    .map((p, i) => (
                      <p key={i}>{p}</p>
                    ))}
                </div>
              ) : (
                <p className="mt-1 text-[13px] text-ink-3">Traduciendo pseudocódigo…</p>
              )}
            </div>

            <div className="min-h-0 flex-1 overflow-auto bg-inset p-4 font-mono text-[13px] leading-6" role="log" aria-label="Pseudocódigo">
              {lines.map((l, i) => (
                <div key={i} className="grid grid-cols-[44px_1fr] gap-3 hover:bg-hover/50">
                  <span className="select-none text-right tabular-nums text-ink-3">{i + 1}</span>
                  <span className="whitespace-pre text-ink-1">{l || " "}</span>
                </div>
              ))}
            </div>
          </>
        )}
      </section>
    </div>
  )
}
