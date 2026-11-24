import { useState } from "react"
import {
  ArrowUp,
  CheckCircle2,
  ChevronRight,
  Circle,
  FolderGit2,
  HelpCircle,
  OctagonPause,
  Send,
  ThumbsDown,
  ThumbsUp,
  XCircle,
} from "lucide-react"
import { toast } from "sonner"
import { Brand } from "@/components/brand"
import { Viewer } from "@/components/viewer"
import {
  post,
  useNow,
  useReducedMotion,
  type Avance,
  type ClaimBoard,
  type Kind,
  type Repo,
  type Run,
  type Sweep,
  type Task,
} from "@/lib/agencia"
import { cn } from "@/lib/utils"

const dur = (s: number) => (s < 60 ? Math.round(s) + "s" : s < 3600 ? Math.round(s / 60) + "m" : (s / 3600).toFixed(1) + "h")
const CLAIMED = ["pass", "done"]
const SAY: Record<string, string> = { work: "haciendo", pass: "pasó", done: "hecho", fix: "corrigió", fail: "falló", blocked: "detenido" }
const PKIND = (k: string): Kind =>
  k === "bec" || k === "opencode" ? "bec" : k === "codex" || k === "cdx" ? "codex" : k === "agy" ? "agy" : "claude"
const shotOf = (t: Task) => t.audit?.shot || (/\.(png|jpe?g)$/i.test(t.last?.ev ?? "") ? t.last!.ev : "")
const isShot = (p: string) => /^web\/status\/.+\.(png|jpe?g)$/i.test(p)

function VerdictBadge({ t }: { t: Task }) {
  if (!t.audit) {
    return (
      <span className="inline-flex items-center gap-1.5 rounded border border-line bg-inset px-2 py-0.5 text-[12px] text-ink-3">
        <Circle className="size-3" />
        {t.last && ["pass", "done", "fail", "fix", "blocked"].includes(t.last.state) ? "Por auditar" : "Sin resultado"}
      </span>
    )
  }
  const v = t.audit.v
  const Icon = v === "sirve" ? CheckCircle2 : v === "no" ? XCircle : HelpCircle
  return (
    <div className="flex flex-wrap items-center gap-1.5">
      <span
        className={cn(
          "inline-flex items-center gap-1 rounded border px-2 py-0.5 text-[12px] font-medium",
          v === "sirve"
            ? "border-[var(--sig-ok-border)] bg-[var(--sig-ok-soft)] text-ok"
            : v === "no"
              ? "border-[var(--sig-bad-border)] bg-[var(--sig-bad-soft)] text-bad"
              : "border-[var(--sig-need-border)] bg-[var(--sig-need-soft)] text-need",
        )}
      >
        <Icon className="size-3.5" />
        {v === "sirve" ? "Sirve" : v === "no" ? "No sirve" : "Sin evidencia"}
      </span>
      {t.audit.claim && t.audit.claim !== "coincide" && (
        <span className="rounded border border-line bg-inset px-1.5 py-0.5 text-[12px] text-ink-2">
          el agente {t.audit.claim}
        </span>
      )}
    </div>
  )
}

function VoteGroup({ repo, t, demoMode }: { repo: string; t: Task; demoMode?: boolean }) {
  const [busy, setBusy] = useState(false)
  if (!t.audit) return null
  const go = async (agree: boolean) => {
    if (demoMode) {
      toast.success(agree ? "Voto 'De acuerdo' simulado" : "Voto 'No' simulado")
      return
    }
    setBusy(true)
    const r = await post("/api/avance/voto", { repo, key: t.key, agree })
    setBusy(false)
    if (r.ok) toast.success(r.info || "Anotado")
    else toast.error(r.info || "No se pudo registrar")
  }
  return (
    <div className="inline-flex rounded-md border border-line bg-inset p-0.5" role="group" aria-label="¿De acuerdo con el auditor?">
      <button
        type="button"
        disabled={busy}
        onClick={() => go(true)}
        className={cn(
          "pressable inline-flex h-6 items-center gap-1 rounded px-2 text-[12px] font-medium",
          t.vote === true ? "bg-[var(--sig-ok-soft)] text-ok" : "text-ink-3 hover:text-ink-1",
        )}
      >
        <ThumbsUp className="size-3" />
        De acuerdo
      </button>
      <button
        type="button"
        disabled={busy}
        onClick={() => go(false)}
        className={cn(
          "pressable inline-flex h-6 items-center gap-1 rounded px-2 text-[12px] font-medium",
          t.vote === false ? "bg-[var(--sig-bad-soft)] text-bad" : "text-ink-3 hover:text-ink-1",
        )}
      >
        <ThumbsDown className="size-3" />
        No
      </button>
    </div>
  )
}

function TaskRow({
  repo,
  t,
  now,
  orq,
  gate,
  demoMode,
}: {
  repo: string
  t: Task
  now: number
  orq: string
  gate?: boolean
  demoMode?: boolean
}) {
  const [ask, setAsk] = useState(false)
  const [v, setV] = useState("")
  const [arm, setArm] = useState(false)
  const shot = shotOf(t)

  const say = async (text: string) => {
    if (demoMode) {
      toast.success("Orden simulada al Orquestador")
      return true
    }
    if (!orq) {
      toast.error("El Orquestador no está abierto")
      return false
    }
    const r = await post("/api/send", { to: orq, text })
    if (r.ok) toast.success("Enviado al Orquestador")
    else toast.error(r.info || "No se pudo enviar")
    return r.ok
  }

  return (
    <div className="grid grid-cols-1 gap-3 px-4 py-3.5 lg:grid-cols-[72px_1fr_auto] lg:items-start">
      <span
        className={cn(
          "inline-flex h-6 w-fit items-center justify-center rounded border px-2 font-mono text-[12px] font-semibold",
          t.ms ? "border-line-strong bg-inset text-ink-1" : "border-[var(--sig-need-border)] bg-[var(--sig-need-soft)] text-need",
        )}
        title={t.done_when ? `Done when: ${t.done_when}` : "Sin meta en GOAL.md"}
      >
        {t.ms || "Desvío"}
      </span>

      <div className="min-w-0 flex flex-col gap-1">
        <div className="flex flex-wrap items-center gap-2">
          <span className="text-[14px] font-semibold text-ink-1">{gate ? "Firmada por el Orquestador" : t.title}</span>
          {!gate && (
            <span className="inline-flex shrink-0 items-center gap-1.5 rounded border border-line bg-inset px-2 py-0.5 text-[12px] text-ink-2">
              <Brand kind={PKIND(t.kind)} className="size-3.5" />
              <span>{t.who || t.kind}</span>
            </span>
          )}
        </div>
        <p className="text-[13px] text-ink-2">
          <span className="text-ink-3">
            {gate
              ? "Done when: "
              : `Dice (${t.last ? `${SAY[t.last.state] ?? t.last.state}, hace ${dur(now - t.last.t)}` : "sin reporte"}): `}
          </span>
          {gate ? t.done_when : t.last?.line ?? "—"}
        </p>
        <p className="text-[13px] text-ink-1">
          {t.audit ? (
            <>
              <span className="font-semibold text-ink-2">Auditor: </span>
              {t.audit.why}
            </>
          ) : (
            <span className="text-ink-3">El auditor todavía no revisa esta evidencia.</span>
          )}
        </p>
        {!gate && ask && (
          <form
            className="mt-1.5 flex items-center gap-2 rounded-md border border-line-strong bg-inset px-2.5 py-1.5"
            onSubmit={async (e) => {
              e.preventDefault()
              if (v.trim() && (await say(`Para ${t.who} (${t.line}): ${v.trim()}`))) {
                setAsk(false)
                setV("")
              }
            }}
          >
            <input
              autoFocus
              value={v}
              onChange={(e) => setV(e.target.value)}
              onKeyDown={(e) => e.key === "Escape" && setAsk(false)}
              placeholder={`Instrucción al Orquestador sobre ${t.who}…`}
              className="min-w-0 flex-1 bg-transparent text-[13px] text-ink-1 outline-none placeholder:text-ink-3"
            />
            <button
              type="submit"
              disabled={!v.trim()}
              className="pressable grid size-6 place-items-center rounded bg-ink-1 text-ink-inv disabled:opacity-35"
            >
              <ArrowUp className="size-3.5" />
            </button>
          </form>
        )}
        {shot && isShot(shot) && (
          <Viewer
            className="mt-2 h-24 w-40 overflow-hidden rounded border border-line bg-inset hover:border-line-strong"
            src={`/api/repofile?repo=${encodeURIComponent(repo)}&p=${encodeURIComponent(shot)}`}
          >
            <img
              src={`/api/repofile?repo=${encodeURIComponent(repo)}&p=${encodeURIComponent(shot)}`}
              alt={`Prueba: ${shot}`}
              loading="lazy"
              className="size-full object-cover"
            />
          </Viewer>
        )}
      </div>

      <div className="flex flex-wrap items-center gap-2 lg:flex-col lg:items-end">
        <VerdictBadge t={t} />
        <VoteGroup repo={repo} t={t} demoMode={demoMode} />
        {!gate && (
          <div className="flex items-center gap-1.5">
            <button
              type="button"
              onClick={() => setAsk(!ask)}
              className={cn(
                "pressable inline-flex h-7 items-center gap-1 rounded-md border px-2.5 text-[12px] font-medium",
                ask ? "border-line-strong bg-elevated text-ink-1" : "border-line bg-surface text-ink-2 hover:bg-hover hover:text-ink-1",
              )}
            >
              <Send className="size-3" />
              Ordenar
            </button>
            <button
              type="button"
              onClick={async () => {
                if (!arm) {
                  setArm(true)
                  setTimeout(() => setArm(false), 3000)
                  return
                }
                setArm(false)
                await say(`Frena ya a ${t.who} (${t.line}) y deja su línea en blocked en board.json.`)
              }}
              className={cn(
                "pressable inline-flex h-7 items-center gap-1 rounded-md border px-2.5 text-[12px] font-medium",
                arm
                  ? "border-[var(--sig-bad-border)] bg-[var(--sig-bad-soft)] text-bad"
                  : "border-line bg-surface text-ink-2 hover:text-bad",
              )}
            >
              <OctagonPause className="size-3" />
              {arm ? "¿Frenar?" : "Frenar"}
            </button>
          </div>
        )}
      </div>
    </div>
  )
}

function RunCard({ c, img, label, restart }: { c: Run; img: (p: string) => string; label: string; restart?: boolean }) {
  const pct = c.of ? Math.round((100 * c.ok) / c.of) : 0
  return (
    <div className="flex flex-col gap-2 rounded-lg border border-line bg-inset p-3" title={c.why}>
      {c.shot && (
        <Viewer className="h-24 w-full overflow-hidden rounded border border-line bg-surface" src={img(c.shot)} title={label}>
          <img src={img(c.shot) + "&t=" + Math.round(c.t)} alt={label} loading="lazy" className="size-full object-cover" />
        </Viewer>
      )}
      <div className="flex items-center justify-between text-[12px]">
        <span className="font-medium text-ink-2">{label}</span>
        {c.live && <span className="font-mono text-[12px] text-work">corriendo</span>}
      </div>
      <div className="font-mono text-[18px] font-semibold tabular text-ink-1">
        {c.ok}
        <span className="ml-1 text-[12px] font-normal text-ink-3">{restart ? "seguidas" : `/ ${c.of}`}</span>
      </div>
      {!restart && (
        <div className="h-1.5 w-full overflow-hidden rounded-full bg-hover">
          <div className="h-full bg-ok" style={{ width: `${pct}%` }} />
        </div>
      )}
      <p className="truncate text-[12px] text-ink-3">
        {c.live ? `en ${c.at || "arranque"}` : c.bad ? `se detiene en ${c.bad}` : c.ok < c.of ? `corte en ${c.at || "?"}` : "completo"}
      </p>
    </div>
  )
}

function EffortSection({ repo, sw, boards }: { repo: string; sw?: Sweep | null; boards: ClaimBoard[] }) {
  const img = (p: string) => `/api/repofile?repo=${encodeURIComponent(repo)}&p=${encodeURIComponent(p)}`
  const claims = boards.flatMap((b) => b.claims)
  const fronts = boards.flatMap((b) => b.fronts).filter((f) => f.done !== null)
  const approved = claims.filter((c) => c.verdict?.v === "APPROVE").length
  const refuted = claims.filter((c) => c.verdict?.v === "REFUTE").length
  const graded = claims.filter((c) => c.verdict?.v === "GRADE").length

  const contextLabel = boards.length
    ? `${repo} · ${boards.length === 1 ? "1 tablero" : `${boards.length} tableros`}`
    : `${repo} · Barrido automático`

  const heading = sw
    ? `Barrido automático (${sw.ok}/${sw.total})`
    : boards.length === 1 && boards[0].title
      ? boards[0].title
      : "Resumen de tableros"

  const tiles: { label: string; value: string; sub: string; ok?: boolean }[] = []
  if (sw) {
    tiles.push({
      label: "Corridas sin fallo",
      value: `${sw.ok} / ${sw.total}`,
      sub: "último barrido",
      ok: sw.ok >= sw.total,
    })
  }
  if (claims.length > 0) {
    tiles.push({
      label: "Afirmaciones aprobadas",
      value: `${approved} / ${claims.length}`,
      sub: refuted ? `${refuted} refutadas` : graded ? `${graded} calificadas` : "en revisión",
      ok: claims.length > 0 && approved === claims.length && refuted === 0,
    })
  }
  if (fronts.length > 0) {
    tiles.push({
      label: "Frentes entregados",
      value: `${fronts.filter((f) => f.done).length} / ${fronts.length}`,
      sub: "con ruta de entrega",
      ok: fronts.every((f) => f.done),
    })
  }

  return (
    <section className="rounded-xl border border-line-strong bg-surface p-5">
      <p className="font-mono text-[12px] text-ink-3">{contextLabel}</p>
      <h2 className="mt-1 text-balance text-[18px] font-semibold tracking-tight text-ink-1">{heading}</h2>
      {tiles.length > 0 && (
        <div
          className={cn(
            "mt-4 grid grid-cols-1 gap-3",
            tiles.length === 2 ? "sm:grid-cols-2" : tiles.length >= 3 ? "sm:grid-cols-3" : "",
          )}
        >
          {tiles.map((tile) => (
            <StatTile key={tile.label} label={tile.label} value={tile.value} sub={tile.sub} ok={tile.ok} />
          ))}
        </div>
      )}
      {sw && (sw.chapters.length > 0 || sw.auto.length > 0) && (
        <div className="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-4">
          {sw.chapters.map((c) => (
            <RunCard key={c.n} c={c} img={img} label={`Capítulo ${c.n}`} />
          ))}
          {sw.auto.map((a) => (
            <RunCard key={a.dir} c={a} img={img} label={`${a.dir === "auto" ? "Reinicio" : a.dir.toUpperCase()} (${a.start || "?"})`} restart />
          ))}
        </div>
      )}
    </section>
  )
}

function ClaimBoardSection({ b }: { b: ClaimBoard }) {
  const count = (v: string) => b.claims.filter((c) => c.verdict?.v === v).length
  const approved = count("APPROVE")
  const refuted = count("REFUTE")
  const graded = count("GRADE")
  const pending = b.claims.filter((c) => !c.verdict).length

  return (
    <section className="overflow-hidden rounded-xl border border-line bg-surface">
      <div className="ag-grad-elevated border-b border-line px-4 py-3">
        <div className="flex flex-wrap items-baseline justify-between gap-2">
          <h3 className="text-[14px] font-semibold text-ink-1">Tablero: {b.title}</h3>
          <div className="flex flex-wrap items-center gap-3 font-mono text-[12px] tabular text-ink-3">
            <span>{approved} aprobadas</span>
            {refuted > 0 && <span className="text-bad">{refuted} refutadas</span>}
            {graded > 0 && <span className="text-work">{graded} calificadas</span>}
            <span>{pending} pendientes</span>
          </div>
        </div>
        {b.why && <p className="mt-1 text-pretty text-[13px] text-ink-2">{b.why}</p>}
      </div>

      {b.fronts.length > 0 && (
        <div className="flex flex-wrap items-center gap-2 border-b border-line bg-inset/60 px-4 py-2.5">
          <span className="font-mono text-[12px] font-medium text-ink-3">Frentes:</span>
          {b.fronts.map((f) => (
            <div
              key={f.id}
              title={f.out ? `${f.scope} — ${f.out}` : f.scope}
              className="inline-flex max-w-full items-center gap-2 rounded-md border border-line bg-surface px-2.5 py-1 text-[12px]"
            >
              <span className="font-mono font-semibold text-ink-1">{f.id}</span>
              {f.who && <span className="text-ink-2">{f.who}</span>}
              {f.scope && <span className="max-w-[200px] truncate text-ink-3">{f.scope}</span>}
              <span
                className={cn(
                  "rounded border px-1.5 py-0.5 font-mono text-[11px] font-medium leading-none",
                  f.done === true
                    ? "border-[var(--sig-ok-border)] bg-[var(--sig-ok-soft)] text-ok"
                    : f.done === false
                      ? "border-[var(--sig-need-border)] bg-[var(--sig-need-soft)] text-need"
                      : "border-line bg-inset text-ink-3",
                )}
              >
                {f.done === true ? "Entregado" : f.done === false ? "Pendiente" : "En curso"}
              </span>
            </div>
          ))}
        </div>
      )}

      {b.claims.length > 0 ? (
        <div className="divide-y divide-line">
          {b.claims.map((c) => {
            const v = c.verdict
            return (
              <div key={c.id} className="grid grid-cols-1 gap-2 px-4 py-3 lg:grid-cols-[72px_1fr_auto] lg:items-start">
                <span className="font-mono text-[12px] font-semibold text-ink-2">{c.id}</span>
                <div className="min-w-0 flex flex-col gap-1">
                  <p className="text-[13.5px] font-medium text-ink-1">{c.claim}</p>
                  <p className="text-[12.5px] text-ink-3">Evidencia exigida: {c.need}</p>
                  {v && (
                    <p className="text-[13px] text-ink-2">
                      <b className="text-ink-1">Adversario:</b> {v.why || "sin explicación"}
                      {v.missing ? ` (Falta: ${v.missing})` : ""}
                    </p>
                  )}
                </div>
                <span
                  className={cn(
                    "inline-flex w-fit items-center rounded border px-2 py-0.5 text-[12px] font-medium",
                    v?.v === "APPROVE"
                      ? "border-[var(--sig-ok-border)] bg-[var(--sig-ok-soft)] text-ok"
                      : v?.v === "REFUTE"
                        ? "border-[var(--sig-bad-border)] bg-[var(--sig-bad-soft)] text-bad"
                        : v?.v === "GRADE"
                          ? "border-[var(--sig-work-border)] bg-[var(--sig-work-soft)] text-work"
                          : "border-line bg-inset text-ink-3",
                  )}
                >
                  {v?.v === "APPROVE" ? "Aprobada" : v?.v === "REFUTE" ? "Refutada" : v ? `Grado ${v.grade ?? "?"}/3` : "Sin veredicto"}
                </span>
              </div>
            )
          })}
        </div>
      ) : (
        <p className="px-4 py-6 text-center text-[13px] text-ink-3">Este tablero aún no tiene afirmaciones registradas.</p>
      )}

      {b.summaries && b.summaries.length > 0 && (
        <details className="group border-t border-line bg-inset/40">
          <summary className="flex cursor-pointer list-none items-center gap-2 px-4 py-2.5 text-[12.5px] font-medium text-ink-2 hover:bg-hover hover:text-ink-1">
            <ChevronRight className="size-3.5 shrink-0 text-ink-3 transition-transform duration-150 group-open:rotate-90" />
            <span>Resúmenes de sesión ({b.summaries.length})</span>
          </summary>
          <div className="divide-y divide-line border-t border-line bg-surface">
            {b.summaries.map((s, idx) => (
              <div key={idx} className="px-4 py-3">
                {s.head && <h4 className="text-[13px] font-semibold text-ink-1">{s.head}</h4>}
                {s.body && <p className="mt-1 whitespace-pre-wrap text-[12.5px] leading-relaxed text-ink-2">{s.body}</p>}
              </div>
            ))}
          </div>
        </details>
      )}
    </section>
  )
}

function StatTile({ label, value, sub, ok }: { label: string; value: string; sub: string; ok?: boolean }) {
  return (
    <div className="rounded-lg border border-line bg-inset px-3.5 py-3">
      <div className="text-[12px] text-ink-3">{label}</div>
      <div className={cn("mt-1 font-mono text-[20px] font-semibold tabular", ok ? "text-ok" : "text-ink-1")}>{value}</div>
      <div className="mt-0.5 text-[12px] text-ink-3">{sub}</div>
    </div>
  )
}

function RepoBoard({
  repo,
  a,
  now,
  multi,
  demoMode,
}: {
  repo: string
  a: Avance
  now: number
  multi?: boolean
  demoMode?: boolean
}) {
  const hasGoal = a.goal.length > 0
  const hasBoardsOrSweep = Boolean(a.sweep || a.boards?.length)
  const cur = a.goal.find((g) => g.id === a.cur)
  const mine = hasGoal ? a.tasks.filter((t) => t.ms === a.cur) : a.tasks
  const claimed = mine.filter((t) => t.last && CLAIMED.includes(t.last.state)).length
  const ok = mine.filter((t) => t.audit?.v === "sirve").length
  const signed = a.goal.filter((g) => g.state === "VERIFIED")
  const held = signed.filter((g) => g.gate?.audit?.v === "sirve").length
  const rest = hasGoal ? a.tasks.filter((t) => t.ms !== a.cur) : []

  return (
    <div id={`repo-board-${repo}`} className="flex flex-col gap-5 scroll-mt-4">
      {multi && (
        <div className="flex flex-wrap items-center justify-between gap-2 rounded-lg border border-line-strong bg-elevated px-4 py-2.5">
          <div className="flex items-center gap-2">
            <FolderGit2 className="size-4 shrink-0 text-ink-3" />
            <h2 className="text-[15px] font-semibold text-ink-1">{repo}</h2>
          </div>
          <div className="flex items-center gap-3 font-mono text-[12px] tabular text-ink-3">
            {a.boards?.length ? <span>{a.boards.length === 1 ? "1 tablero" : `${a.boards.length} tableros`}</span> : null}
            {hasGoal ? <span>{a.goal.length} metas</span> : null}
            {a.tasks.length > 0 ? <span>{a.tasks.length} tareas</span> : null}
          </div>
        </div>
      )}

      {hasBoardsOrSweep && <EffortSection repo={repo} sw={a.sweep} boards={a.boards ?? []} />}
      {(a.boards ?? []).map((b) => (
        <ClaimBoardSection key={b.file} b={b} />
      ))}

      {/* Goal Hero (only when GOAL.md milestones exist) */}
      {hasGoal && (
        <section className="rounded-xl border border-line-strong bg-surface p-3.5 sm:p-5">
          <div className="flex flex-wrap items-baseline justify-between gap-2">
            <span className="font-mono text-[12px] text-ink-3">{repo} · Verificación de metas</span>
            <span className={cn("text-[12px]", a.err ? "text-bad" : a.busy ? "text-work" : "text-ink-3")}>
              {a.err
                ? `Error del auditor: ${a.err}`
                : a.busy
                  ? "Auditor revisando evidencia…"
                  : a.pending
                    ? "Cambios pendientes: audita en 1 min"
                    : "Auditor en espera de commits o reportes"}
            </span>
          </div>
          <h2 className="mt-1.5 text-balance text-[18px] font-semibold tracking-tight text-ink-1">
            {cur ? (
              <>
                <span className="mr-2 font-mono text-work">{cur.id}</span>
                {cur.done_when}
              </>
            ) : (
              "Todas las metas están firmadas"
            )}
          </h2>

          <div className="mt-4 grid grid-cols-2 gap-3 lg:grid-cols-4">
            <StatTile label="Agentes dicen" value={`${claimed} / ${mine.length}`} sub="tareas reportadas listas" />
            <StatTile
              label="Auditor confirma"
              value={`${ok} / ${mine.length}`}
              sub="evidencia verificada"
              ok={mine.length > 0 && ok >= claimed && ok > 0}
            />
            <StatTile label="Metas firmadas válidas" value={`${held} / ${signed.length}`} sub="sostenidas por auditor" />
            <StatTile
              label="Calibración OWNER"
              value={a.votes ? `${a.agree} / ${a.votes}` : "—"}
              sub={a.votes ? "coincidencias con tu criterio" : "vota abajo para calibrar"}
            />
          </div>

          {/* Milestone Strip */}
          <div className="mt-4 flex flex-wrap gap-2">
            {a.goal.map((g) => {
              const v = g.gate?.audit?.v
              const sym = g.state !== "VERIFIED" ? "○" : v === "sirve" ? "✓" : v ? "✕" : "?"
              return (
                <div
                  key={g.id}
                  title={`${g.id} · ${g.state}\nDone when: ${g.done_when}`}
                  className={cn(
                    "flex items-center gap-2 rounded-md border px-2.5 py-1.5 font-mono text-[12px]",
                    g.id === a.cur
                      ? "border-[var(--sig-work-border)] bg-[var(--sig-work-soft)] text-ink-1"
                      : g.state === "VERIFIED"
                        ? "border-[var(--sig-ok-border)] bg-[var(--sig-ok-soft)] text-ok"
                        : "border-line bg-inset text-ink-3",
                  )}
                >
                  <span className="font-semibold">{g.id}</span>
                  <span>{sym}</span>
                </div>
              )
            })}
          </div>
        </section>
      )}

      {/* Current Milestone Tasks or Standalone Tasks */}
      {(hasGoal || mine.length > 0) && (
        <section className="overflow-hidden rounded-xl border border-line bg-surface">
          <div className="border-b border-line bg-elevated px-4 py-3">
            <h3 className="text-[14px] font-semibold text-ink-1">
              {hasGoal ? `Tareas de ${a.cur || "la meta"}` : `Tareas del plan (${mine.length})`}
            </h3>
            <p className="text-[12px] text-ink-3">Lo que afirma cada agente frente al veredicto ciego del auditor.</p>
          </div>
          <div className="divide-y divide-line">
            {mine.length ? (
              mine.map((t) => <TaskRow key={t.key} repo={repo} t={t} now={now} orq={a.orq} demoMode={demoMode} />)
            ) : (
              <p className="p-5 text-[13px] text-ink-3">
                El plan del Orquestador no tiene tareas para {a.cur || "esta meta"}.
              </p>
            )}
          </div>
        </section>
      )}

      {signed.some((g) => g.gate) && (
        <section className="overflow-hidden rounded-xl border border-line bg-surface">
          <div className="border-b border-line bg-elevated px-4 py-3">
            <h3 className="text-[14px] font-semibold text-ink-1">Metas firmadas</h3>
          </div>
          <div className="divide-y divide-line">
            {signed
              .filter((g) => g.gate)
              .map((g) => (
                <TaskRow key={g.id} repo={repo} t={g.gate!} now={now} orq={a.orq} gate demoMode={demoMode} />
              ))}
          </div>
        </section>
      )}

      {rest.length > 0 && (
        <section className="overflow-hidden rounded-xl border border-line bg-surface">
          <div className="border-b border-line bg-elevated px-4 py-3">
            <h3 className="text-[14px] font-semibold text-ink-1">Otras tareas del plan</h3>
          </div>
          <div className="divide-y divide-line">
            {rest.map((t) => (
              <TaskRow key={t.key} repo={repo} t={t} now={now} orq={a.orq} demoMode={demoMode} />
            ))}
          </div>
        </section>
      )}

      {!hasGoal && !hasBoardsOrSweep && a.tasks.length === 0 && (
        <div className="rounded-xl border border-dashed border-line bg-surface p-6 text-center">
          <p className="text-[13.5px] font-medium text-ink-1">Sin metas ni tableros en {repo}</p>
          <p className="mt-1 text-pretty text-[12.5px] text-ink-3">
            Este repositorio aún no registra metas en <code className="font-mono">GOAL.md</code> ni afirmaciones en{" "}
            <code className="font-mono">board.json</code>.
          </p>
        </div>
      )}
    </div>
  )
}

export function TableroView({
  repos,
  avance,
  demoMode,
}: {
  repos: Repo[]
  avance: Record<string, Avance>
  demoMode?: boolean
}) {
  const now = useNow()
  const reduced = useReducedMotion()
  const names = new Set(repos.map((r) => r.name))
  const list = Object.entries(avance).filter(([r]) => names.has(r))

  return (
    <div className="h-full overflow-y-auto bg-canvas p-4 sm:p-6">
      <div className="mx-auto flex max-w-[1160px] flex-col gap-6">
        {list.length > 1 && (
          <nav aria-label="Saltar a repositorio" className="flex flex-wrap items-center gap-2 rounded-xl border border-line bg-surface px-3.5 py-2.5">
            <span className="font-mono text-[12px] font-medium text-ink-3">Repositorios ({list.length}):</span>
            {list.map(([r]) => (
              <button
                key={r}
                type="button"
                onClick={() =>
                  document.getElementById(`repo-board-${r}`)?.scrollIntoView({
                    behavior: reduced ? "auto" : "smooth",
                    block: "start",
                  })
                }
                className="pressable inline-flex items-center gap-1.5 rounded-md border border-line bg-inset px-2.5 py-1 font-mono text-[12px] font-medium text-ink-2 hover:border-line-strong hover:bg-hover hover:text-ink-1"
              >
                <FolderGit2 className="size-3.5 text-ink-3" />
                {r}
              </button>
            ))}
          </nav>
        )}

        {list.length ? (
          list.map(([r, a]) => <RepoBoard key={r} repo={r} a={a} now={now} multi={list.length > 1} demoMode={demoMode} />)
        ) : (
          <div className="rounded-xl border border-dashed border-line bg-surface p-8 text-center">
            <h2 className="text-balance text-[16px] font-semibold text-ink-1">Sin tablero de metas en este repositorio</h2>
            <p className="mt-1.5 text-pretty text-[13px] text-ink-3">
              Este repo no tiene <code className="font-mono">GOAL.md</code> ni <code className="font-mono">web/status/board.json</code> para auditar.
            </p>
          </div>
        )}
      </div>
    </div>
  )
}
