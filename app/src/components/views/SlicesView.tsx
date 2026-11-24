import { useEffect, useState } from "react"
import { ChevronRight } from "lucide-react"
import { Viewer } from "@/components/viewer"
import { page, STATUS, type Card, type Repo, type Win } from "@/lib/agencia"
import { buildDemoRun } from "@/lib/demo"
import { flat, isWin, type Item } from "@/lib/tone"
import { cn } from "@/lib/utils"

type Account = { id: string; name: string; spent: number; over: boolean }
type Ref = { p: string; name: string }
type Job = {
  id: string
  who: string
  brief: string
  status: "q" | "r" | "ok" | "fallo" | "pregunta"
  model: string
  steps: number
  secs: number
  line: string
  reply: string
  refs: Ref[]
  tokens: number
  usd: number
  t: number
}
type SFile = Ref & { add: number; rem: number; isNew: boolean; size: number; t: number; view: boolean }
type Commit = { hash: string; subject: string; t: number }
type Note = { t: number; did: string; now: string; next: string }
type Slice = {
  name: string
  path: string
  ahead: number
  dirty: number
  last: string
  age: number | null
  claude: number
  codex: number
  bec: number
  running: number
  queued: number
  done: number
  failed: number
  steps: number
  tokens: number
  usd: number
  added: number
  removed: number
  files_n: number
  files: SFile[]
  commits: Commit[]
  jobs: Job[]
  story: Note[]
  story_err: string
}
type RunEntry = { repo: string; path: string; state: "on" | "done"; until: number; at?: string }
type Run = {
  ok: boolean
  state: "off" | "on" | "done"
  repo?: string
  goal?: string
  started?: number
  until?: number
  pct?: number
  haiku?: { awake: boolean; used: number; cap: number }
  accounts?: Account[]
  slices?: Slice[]
  runs?: RunEntry[]
}

const left = (s: number) => {
  const m = Math.max(0, Math.round(s / 60))
  return m < 60 ? `${m} min` : `${Math.floor(m / 60)}h ${String(m % 60).padStart(2, "0")}m`
}
const ago = (s: number | null) =>
  s === null ? "sin actividad" : s < 60 ? "hace un momento" : s < 3600 ? `hace ${Math.round(s / 60)}m` : `hace ${Math.round(s / 3600)}h`
const since = (t: number) => ago(Date.now() / 1000 - t)
const clock = (t: number) => new Date(t * 1000).toLocaleTimeString("es-MX", { hour: "2-digit", minute: "2-digit", hourCycle: "h23" })
const short = (n: number) => (n >= 1e6 ? `${(n / 1e6).toFixed(1)}M` : n >= 1e3 ? `${Math.round(n / 1e3)}k` : String(n))
const sizeFmt = (n: number) => (n >= 1048576 ? `${(n / 1048576).toFixed(1)} MB` : n >= 1024 ? `${Math.round(n / 1024)} KB` : `${n} B`)
const rate = (ok: number, bad: number) => (ok + bad ? `${Math.round((100 * ok) / (ok + bad))}%` : "—")
const JOB_WORD = { q: "En cola", r: "Corriendo", ok: "Listo", fallo: "Falló", pregunta: "Pregunta" } as const
const known = (x: string) => x && !/^sin dato/i.test(x)
const cap = (x: string) => (x ? x.charAt(0).toUpperCase() + x.slice(1) : "")

function Kpi({ label, value, sub, meter }: { label: string; value: string; sub?: string; meter?: number }) {
  return (
    <div className="rounded-lg border border-line bg-surface p-3.5">
      <div className="text-[12px] text-ink-3">{label}</div>
      <div className="mt-1 font-mono text-[20px] font-semibold tabular-nums text-ink-1">{value}</div>
      {meter !== undefined && (
        <div className="mt-2 h-1.5 w-full overflow-hidden rounded-full bg-inset">
          <div className="h-full bg-work" style={{ width: `${Math.min(100, Math.max(0, meter))}%` }} />
        </div>
      )}
      {sub && <div className="mt-1.5 text-[12px] text-ink-3">{sub}</div>}
    </div>
  )
}

function LeaderRow({ it, depth, onOpen }: { it: Item; depth: number; onOpen: (id: string) => void }) {
  const win = isWin(it) ? (it as Win) : null
  const kids = win?.children ?? []
  const nested = kids.filter((c) => isWin(c as Item) || c.kind === "sub")
  const jobs = kids.filter((c) => !nested.includes(c))
  const busy = jobs.filter((j) => j.status === "working" || j.status === "queued").length

  return (
    <>
      <button
        type="button"
        onClick={() => onOpen(it.id)}
        style={{ paddingLeft: 14 + depth * 20 }}
        className="pressable flex w-full items-center gap-3 border-b border-line-subtle py-2 pr-4 text-left text-[13px] hover:bg-hover"
      >
        <span className="font-medium text-ink-1">{it.title}</span>
        <span className="rounded border border-line bg-inset px-1.5 py-0.5 font-mono text-[12px] text-ink-2">
          {STATUS[it.status]}
        </span>
        <span className="min-w-0 flex-1 truncate text-ink-3">
          {it.now.startsWith("|") ? "Entregó una tabla — abre para verla" : it.now}
        </span>
      </button>
      {nested.map((c) => (
        <LeaderRow key={c.id} it={c as Item} depth={depth + 1} onOpen={onOpen} />
      ))}
      {jobs.length > 0 && (
        <details style={{ paddingLeft: 14 + (depth + 1) * 20 }} className="border-b border-line-subtle py-1.5 pr-4 text-[12px] text-ink-3">
          <summary className="cursor-pointer select-none hover:text-ink-1">
            {jobs.length} {jobs.length === 1 ? "trabajo lanzado" : "trabajos lanzados"}
            {busy ? `, ${busy} activos` : ""}
          </summary>
          <div className="mt-1.5 flex flex-col gap-1">
            {jobs.map((j: Card) => (
              <button
                key={j.id}
                type="button"
                onClick={() => onOpen(j.id)}
                className="flex items-center gap-2 rounded px-2 py-1 text-left hover:bg-hover hover:text-ink-1"
              >
                <span className="font-mono text-[12px] text-ink-3">{STATUS[j.status]}</span>
                <span className="truncate text-ink-2">{j.title}</span>
              </button>
            ))}
          </div>
        </details>
      )}
    </>
  )
}

export function SlicesView({ repos, onOpen, demoMode }: { repos: Repo[]; onOpen: (id: string) => void; demoMode?: boolean }) {
  const [run, setRun] = useState<Run | null>(null)
  const [wantRepo, setWantRepo] = useState<string>("")
  const [err, setErr] = useState(false)
  const [pick, setPick] = useState<string | null>(null)

  useEffect(() => {
    if (demoMode) {
      setRun(buildDemoRun())
      setErr(false)
      return
    }
    let dead = false
    const url = wantRepo ? `/api/real?repo=${encodeURIComponent(wantRepo)}` : "/api/real"
    const load = () =>
      fetch(url)
        .then((r) => r.json())
        .then((j: Run) => {
          if (!dead) {
            setRun(j)
            setErr(false)
          }
        })
        .catch(() => {
          if (!dead) setErr(true)
        })
    load()
    const t = setInterval(() => !page.asleep && load(), 5000)
    return () => {
      dead = true
      clearInterval(t)
    }
  }, [demoMode, wantRepo])

  if (err && !run) {
    return <div className="grid h-full place-items-center bg-canvas p-6 text-[13px] text-bad">La Agencia no contesta desde el servidor.</div>
  }
  if (!run) {
    return <div className="grid h-full place-items-center bg-canvas p-6 text-[13px] text-ink-3">Cargando estado de Agencia Real…</div>
  }
  if (run.state === "off") {
    return (
      <div className="grid h-full place-items-center bg-canvas p-6">
        <div className="max-w-lg rounded-xl border border-line-strong bg-surface p-6">
          <h2 className="text-[18px] font-semibold tracking-tight text-ink-1">Agencia Real está apagada</h2>
          <p className="mt-2 text-[13px] text-ink-2">Actívala escribiendo este comando en la ventana del Orquestador dentro del repo:</p>
          <code className="mt-3 block rounded-lg border border-line bg-inset px-3.5 py-2.5 font-mono text-[13px] text-work">
            /agencia-real 6h 30% tu objetivo
          </code>
          <p className="mt-3 text-[12px] text-ink-3">
            6h define el límite de tiempo y 30% el tope semanal permitido por cuenta.
          </p>
        </div>
      </div>
    )
  }

  const slices = run.slices ?? []
  const live = run.state === "on"
  const now = Date.now() / 1000
  const sel = slices.find((s) => s.name === pick) ?? slices[0]
  const accounts = [...(run.accounts ?? [])].sort((a, b) => b.spent - a.spent)
  const sum = (f: (s: Slice) => number) => slices.reduce((n, s) => n + f(s), 0)
  const total = sum((s) => s.jobs.length)
  const done = sum((s) => s.done)
  const failed = sum((s) => s.failed)
  const usd = sum((s) => s.usd)
  const span = (run.until ?? 0) - (run.started ?? 0)
  const usedPct = span > 0 ? (100 * (now - (run.started ?? 0))) / span : 0
  const leaders = repos.find((r) => r.name === run.repo)?.windows.filter((w) => w.kind !== "orphan") ?? []
  const agents = leaders.flatMap(flat).length

  return (
    <div className="h-full overflow-y-auto bg-canvas p-6">
      <div className="mx-auto flex max-w-[1280px] flex-col gap-5">
        {/* Run Switcher when multiple Agencia Real runs exist */}
        {(run.runs?.length ?? 0) > 1 && (
          <div className="flex flex-wrap items-center gap-1.5">
            {run.runs!.map((r) => {
              const active = r.repo === run.repo
              return (
                <button
                  key={r.path}
                  type="button"
                  onClick={() => {
                    setWantRepo(r.repo)
                    setPick(null)
                  }}
                  className={cn(
                    "pressable inline-flex h-8 items-center gap-2 rounded-lg border px-3 text-[13px] font-medium",
                    active
                      ? "border-line-strong bg-elevated text-ink-1 shadow-[var(--shadow-sm)]"
                      : "border-line bg-surface text-ink-2 hover:bg-hover hover:text-ink-1",
                  )}
                >
                  <span className={cn("size-1.5 rounded-full", r.state === "on" ? "bg-ok" : "bg-ink-3")} />
                  <span>{r.repo}</span>
                </button>
              )
            })}
          </div>
        )}

        {/* Run Header */}
        <header className="rounded-xl border border-line-strong bg-surface p-5">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <span className={cn("font-mono text-[12px] font-medium", live ? "text-ok" : "text-ink-3")}>
              {live ? `En ejecución en ${run.repo} · quedan ${left((run.until ?? 0) - now)}` : `Corrida finalizada en ${run.repo}`}
            </span>
            {err && <span className="rounded border border-[var(--sig-need-border)] bg-[var(--sig-need-soft)] px-2 py-0.5 text-[12px] text-need">Sin conexión, datos en caché</span>}
          </div>
          <h1 className="mt-1 text-[20px] font-semibold tracking-tight text-ink-1">{run.goal}</h1>

          <div className="mt-4 grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-6">
            <Kpi label="Tiempo usado" value={`${Math.min(100, Math.round(usedPct))}%`} sub={`de ${left(span)}`} meter={usedPct} />
            <Kpi label="Trabajos listos" value={`${done} / ${total}`} sub={total ? `${Math.round((100 * done) / total)}% lanzados` : "sin trabajos"} meter={total ? (100 * done) / total : 0} />
            <Kpi label="Tasa de éxito" value={rate(done, failed)} sub={`${failed} fallidos`} />
            <Kpi label="Commits" value={String(sum((s) => s.ahead))} sub={`${sum((s) => s.dirty)} sin commit`} />
            <Kpi label="Archivos" value={String(sum((s) => s.files_n))} sub={`+${sum((s) => s.added)} −${sum((s) => s.removed)}`} />
            <Kpi label="Tokens" value={short(sum((s) => s.tokens))} sub={usd ? `$${usd.toFixed(2)} DeepSeek` : `${sum((s) => s.steps)} pasos`} />
          </div>
        </header>

        {/* Account Quota Cap Strip */}
        <section className="rounded-xl border border-line bg-surface p-4">
          <div className="mb-2.5 flex items-baseline justify-between">
            <h2 className="text-[13px] font-semibold text-ink-1">Cuota semanal por cuenta (tope {run.pct}%)</h2>
          </div>
          <div className="flex flex-wrap gap-2">
            {accounts.map((a) => (
              <div
                key={a.id}
                className={cn(
                  "flex items-center gap-2 rounded-md border px-2.5 py-1.5 text-[12px]",
                  a.over ? "border-[var(--sig-bad-border)] bg-[var(--sig-bad-soft)] text-bad" : "border-line bg-inset text-ink-2",
                )}
              >
                <span>{a.name}</span>
                <b className={cn("font-mono tabular-nums", a.over ? "text-bad" : "text-ink-1")}>{a.spent}%</b>
              </div>
            ))}
            {!accounts.length && <p className="text-[13px] text-ink-3">Midiendo cuotas iniciales…</p>}
          </div>
        </section>

        {leaders.length > 0 && (
          <section className="overflow-hidden rounded-xl border border-line bg-surface">
            <div className="border-b border-line bg-elevated px-4 py-2.5 text-[13px] font-semibold text-ink-1">
              Cadena de mando <span className="ml-2 font-mono text-[12px] font-normal text-ink-3">{agents} agentes</span>
            </div>
            <div>{leaders.map((w) => <LeaderRow key={w.id} it={w} depth={0} onOpen={onOpen} />)}</div>
          </section>
        )}

        {/* Slice Comparison Matrix */}
        {slices.length > 0 ? (
          <>
            <section className="overflow-hidden rounded-xl border border-line bg-surface">
              <div className="border-b border-line bg-elevated px-4 py-2.5 text-[13px] font-semibold text-ink-1">
                Comparación de slices
              </div>
              <div className="overflow-x-auto">
                <table className="w-full border-collapse text-left text-[13px]">
                  <thead>
                    <tr className="border-b border-line bg-inset text-[12px] text-ink-3">
                      <th className="px-4 py-2 font-medium">Slice</th>
                      <th className="px-4 py-2 font-medium">Ahora</th>
                      <th className="px-4 py-2 font-medium">Trabajos</th>
                      <th className="px-4 py-2 text-right font-medium">Éxito</th>
                      <th className="px-4 py-2 text-right font-medium">Tokens</th>
                      <th className="px-4 py-2 text-right font-medium">Commits</th>
                      <th className="px-4 py-2 text-right font-medium">Líneas</th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-line-subtle">
                    {slices.map((s) => {
                      const on = s === sel
                      const story = s.story.at(-1)
                      return (
                        <tr
                          key={s.name}
                          onClick={() => setPick(s.name)}
                          tabIndex={0}
                          aria-selected={on}
                          onKeyDown={(e) => {
                            if (e.key === "Enter" || e.key === " ") {
                              e.preventDefault()
                              setPick(s.name)
                            }
                          }}
                          className={cn("cursor-pointer transition-colors focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-work", on ? "bg-active" : "hover:bg-hover")}
                        >
                          <td className="px-4 py-2.5 font-mono font-semibold text-ink-1">{s.name}</td>
                          <td className="max-w-[340px] truncate px-4 py-2.5 text-ink-2">{cap(story?.now || s.last || "Sin recuento")}</td>
                          <td className="px-4 py-2.5 font-mono tabular-nums text-ink-2">
                            {s.done} / {s.jobs.length}
                          </td>
                          <td className={cn("px-4 py-2.5 text-right font-mono tabular-nums", s.failed > 0 ? "text-bad" : "text-ink-1")}>
                            {rate(s.done, s.failed)}
                          </td>
                          <td className="px-4 py-2.5 text-right font-mono tabular-nums text-ink-2">{short(s.tokens)}</td>
                          <td className="px-4 py-2.5 text-right font-mono tabular-nums text-ink-2">{s.ahead}</td>
                          <td className="px-4 py-2.5 text-right font-mono tabular-nums text-ink-2">
                            +{s.added} −{s.removed}
                          </td>
                        </tr>
                      )
                    })}
                  </tbody>
                </table>
              </div>
            </section>

            {sel && (
              <section className="grid grid-cols-1 gap-4 lg:grid-cols-2 xl:grid-cols-3">
                {/* Recount */}
                <div className="rounded-xl border border-line bg-surface p-4">
                  <h3 className="mb-3 text-[13px] font-semibold text-ink-1">Recuento · {sel.name}</h3>
                  {sel.story.at(-1) ? (
                    <div className="flex flex-col gap-3">
                      <div className="rounded-lg border border-line bg-inset p-3 text-[13px]">
                        <p className="text-ink-1">{cap(sel.story.at(-1)!.now)}</p>
                        {known(sel.story.at(-1)!.did) && <p className="mt-1.5 text-ink-2"><b>Hecho:</b> {cap(sel.story.at(-1)!.did)}</p>}
                        {known(sel.story.at(-1)!.next) && <p className="mt-1 text-ink-2"><b>Sigue:</b> {cap(sel.story.at(-1)!.next)}</p>}
                        <p className="mt-1.5 font-mono text-[12px] text-ink-3">{since(sel.story.at(-1)!.t)}</p>
                      </div>
                      <ol className="flex flex-col gap-1.5 text-[12px] text-ink-2">
                        {sel.story
                          .slice(0, -1)
                          .reverse()
                          .map((n) => (
                            <li key={n.t} className="flex gap-2">
                              <time className="shrink-0 font-mono text-[12px] tabular-nums text-ink-3">{clock(n.t)}</time>
                              <span>{cap(known(n.did) ? n.did : n.now)}</span>
                            </li>
                          ))}
                      </ol>
                    </div>
                  ) : (
                    <p className="text-[13px] text-ink-3">
                      {sel.story_err ? `Error de Haiku: ${sel.story_err}` : "Esperando el primer recuento de Haiku."}
                    </p>
                  )}
                </div>

                {/* Jobs */}
                <div className="rounded-xl border border-line bg-surface p-4">
                  <h3 className="mb-3 text-[13px] font-semibold text-ink-1">Trabajos ({sel.jobs.length})</h3>
                  <div className="flex flex-col gap-2">
                    {sel.jobs.map((j) => (
                      <details key={j.id} className="group rounded-lg border border-line bg-inset p-2.5 text-[13px]">
                        <summary className="cursor-pointer list-none">
                          <div className="flex items-center justify-between gap-2">
                            <span className="flex items-center gap-1 font-mono text-[12px] font-semibold text-ink-2">
                              <ChevronRight className="size-3.5 shrink-0 text-ink-3 transition-transform duration-150 group-open:rotate-90" aria-hidden="true" />
                              {JOB_WORD[j.status]}
                            </span>
                            <span className="font-mono text-[12px] tabular-nums text-ink-3">
                              {j.who} · {j.steps} pasos
                            </span>
                          </div>
                          <div className="mt-1 font-medium text-ink-1">{j.brief}</div>
                        </summary>
                        <div className="mt-2 border-t border-line pt-2 text-ink-2">
                          <p className="whitespace-pre-wrap">{j.reply || "Sin texto de resultado todavía."}</p>
                          <div className="mt-2 flex flex-wrap gap-2">
                            {j.refs.map((r) => (
                              <Viewer key={r.p} src={r.p} cwd={sel.path} className="font-mono text-[12px] text-work hover:underline">
                                {r.name}
                              </Viewer>
                            ))}
                            <button type="button" onClick={() => onOpen(j.id)} className="font-mono text-[12px] text-ink-1 underline">
                              Ver terminal
                            </button>
                          </div>
                        </div>
                      </details>
                    ))}
                    {!sel.jobs.length && <p className="text-[13px] text-ink-3">Sin trabajos en esta slice.</p>}
                  </div>
                </div>

                {/* Files & Commits */}
                <div className="flex flex-col gap-4 rounded-xl border border-line bg-surface p-4">
                  <div>
                    <h3 className="mb-2 text-[13px] font-semibold text-ink-1">Archivos ({sel.files_n})</h3>
                    <ul className="flex flex-col gap-1 font-mono text-[12px]">
                      {sel.files.map((f) => (
                        <li key={f.p} className="flex items-center justify-between gap-2">
                          {f.view ? (
                            <Viewer src={f.p} className="truncate text-left text-ink-1 hover:text-work">
                              {f.name}
                            </Viewer>
                          ) : (
                            <span className="truncate text-ink-3">{f.name}</span>
                          )}
                          <span className="shrink-0 tabular-nums text-ink-3">{f.isNew ? sizeFmt(f.size) : `+${f.add} −${f.rem}`}</span>
                        </li>
                      ))}
                      {!sel.files.length && <li className="font-sans text-[13px] text-ink-3">Sin archivos escritos.</li>}
                    </ul>
                  </div>
                  <div className="border-t border-line pt-3">
                    <h3 className="mb-2 text-[13px] font-semibold text-ink-1">Commits ({sel.ahead})</h3>
                    <ul className="flex flex-col gap-1.5 text-[12px]">
                      {sel.commits.map((c) => (
                        <li key={c.hash} className="flex items-baseline gap-2">
                          <code className="shrink-0 font-mono text-[12px] text-work">{c.hash}</code>
                          <span className="min-w-0 flex-1 truncate text-ink-1">{c.subject}</span>
                          <span className="shrink-0 font-mono text-[12px] text-ink-3">{since(c.t)}</span>
                        </li>
                      ))}
                      {!sel.commits.length && <li className="text-ink-3">Sin commits propios.</li>}
                    </ul>
                  </div>
                </div>
              </section>
            )}
          </>
        ) : (
          <div className="rounded-xl border border-dashed border-line bg-surface p-6 text-center text-[13px] text-ink-3">
            Todavía no hay slices. El Orquestador las crea con <code className="font-mono">slice nombre</code>.
          </div>
        )}
      </div>
    </div>
  )
}
