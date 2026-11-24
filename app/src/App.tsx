import { useCallback, useEffect, useMemo, useState } from "react"
import { FlaskConical, Radio } from "lucide-react"
import { Tooltip } from "radix-ui"
import { Group, Panel, Separator, useDefaultLayout } from "react-resizable-panels"
import { Toaster } from "sonner"
import { AgentInspector } from "@/components/inspector/AgentInspector"
import { ActivityDrawer } from "@/components/overlays/ActivityDrawer"
import { CommandPalette } from "@/components/overlays/CommandPalette"
import { ConsultorDrawer } from "@/components/overlays/ConsultorDrawer"
import { LaunchDialog } from "@/components/overlays/LaunchDialog"
import { NavRail, VIEWS, type View } from "@/components/shell/NavRail"
import { TopRibbon } from "@/components/shell/TopRibbon"
import { BandejaView, lanesOf } from "@/components/views/BandejaView"
import { ConsolaView } from "@/components/views/ConsolaView"
import { LupaView } from "@/components/views/LupaView"
import { RedView } from "@/components/views/RedView"
import { SlicesView } from "@/components/views/SlicesView"
import { TableroView } from "@/components/views/TableroView"
import { isWork, useAgencia, useBus, type Win } from "@/lib/agencia"
import { buildDemoState } from "@/lib/demo"
import { flat, toneOf, type Item } from "@/lib/tone"

const initialView = (): View =>
  VIEWS.find((v) => v === new URLSearchParams(location.search).get("v")) ?? "red"

const PORTFOLIO_DEMO = import.meta.env.VITE_PORTFOLIO_DEMO === "1"

const initialDemo = () => PORTFOLIO_DEMO || new URLSearchParams(location.search).get("demo") === "1"

const initialDark = () => {
  const q = new URLSearchParams(location.search).get("theme")
  if (q === "light") return false
  if (q === "dark") return true
  try {
    return localStorage.getItem("agbench.theme") !== "light"
  } catch {
    return true
  }
}

const usePhone = () => {
  const q = "(max-width: 767px)"
  const [m, setM] = useState(() => matchMedia(q).matches)
  useEffect(() => {
    const l = matchMedia(q)
    const f = () => setM(l.matches)
    l.addEventListener("change", f)
    return () => l.removeEventListener("change", f)
  }, [])
  return m
}

const isTyping = (e: KeyboardEvent) =>
  e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement || e.target instanceof HTMLSelectElement

export function App() {
  const [demoMode, setDemoMode] = useState(initialDemo)
  const demoState = useMemo(() => (demoMode ? buildDemoState() : null), [demoMode])
  const { state, live } = useAgencia(demoState)
  const { bus } = useBus(state?.bus, demoMode)

  const [dark, setDark] = useState(initialDark)
  const [view, setView] = useState<View>(initialView)
  const [repoName, setRepoName] = useState<string | null>(() =>
    new URLSearchParams(location.search).get("repo"),
  )
  const [winId, setWinId] = useState<string | null>(null)
  const [fil, setFil] = useState("all")
  const [pickedOpenId, setPickedOpenId] = useState<string | null | undefined>(undefined)
  const [treeOpen, setTreeOpen] = useState(true)
  const [launchOpen, setLaunchOpen] = useState(false)
  const [consultorOpen, setConsultorOpen] = useState(false)
  const [activityOpen, setActivityOpen] = useState(false)
  const [paletteOpen, setPaletteOpen] = useState(false)

  const phone = usePhone()

  useEffect(() => {
    document.documentElement.classList.toggle("dark", dark)
    try {
      localStorage.setItem("agbench.theme", dark ? "dark" : "light")
    } catch {
      /* ignore */
    }
  }, [dark])

  const repos = useMemo(() => state?.repos ?? [], [state?.repos])

  const defaultRepoName = useMemo(() => {
    if (!repos.length) return null
    const score = (r: (typeof repos)[number]) => {
      const all = r.windows.flatMap(flat)
      return [all.filter(isWork).length, all.filter((x) => toneOf(x) !== "idle").length]
    }
    return (
      [...repos].sort((a, b) => {
        const [x, y] = [score(a), score(b)]
        return y[0] - x[0] || y[1] - x[1]
      })[0]?.name ?? null
    )
  }, [repos])

  const effectiveRepoName = repoName ?? defaultRepoName
  const repo = repos.find((r) => r.name === effectiveRepoName) ?? repos[0]
  const allRepos = (view === "consola" || view === "tablero") && effectiveRepoName === "*"
  const win: Win | undefined = repo?.windows.find((w) => w.id === winId) ?? repo?.windows[0]
  const everything = useMemo<Item[]>(() => repos.flatMap((r) => r.windows.flatMap(flat)), [repos])
  const bandejaLanes = useMemo(() => lanesOf(repos), [repos])
  const initialAutoOpenId = useMemo(() => {
    if (phone || new URLSearchParams(location.search).get("insp") === "0") return null
    if (!win || !(view === "consola" || view === "bandeja")) return null
    return view === "bandeja" ? bandejaLanes.need[0]?.it.id ?? null : win.id
  }, [phone, win, view, bandejaLanes])
  const openId = pickedOpenId === undefined ? initialAutoOpenId : pickedOpenId
  const setOpenId = useCallback((id: string | null) => setPickedOpenId(id), [])
  const openItem = everything.find((x) => x.id === openId) ?? null
  const needAll = everything.filter((x) => toneOf(x) === "need").length
  const bandejaOrderCount = bandejaLanes.need.length + bandejaLanes.bad.length + bandejaLanes.stalled.length
  const showInspectorDock =
    !phone && (Boolean(openItem) || view === "consola" || (view === "bandeja" && bandejaOrderCount > 0))

  // The Bandeja/Inspector split keeps the size you drag it to (saved per browser, one layout per set of panels).
  const dockIds = showInspectorDock ? ["stage", "inspector"] : ["stage"]
  const split = useDefaultLayout({ id: "agbench-split", panelIds: dockIds, storage: localStorage, onlySaveAfterUserInteractions: true })

  const onOpen = useCallback((id: string) => setPickedOpenId(id), [])

  // Global keyboard shortcuts
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (document.querySelector("[data-agshot-viewer]")) return
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault()
        setPaletteOpen((p) => !p)
        return
      }
      if (isTyping(e) || e.metaKey || e.ctrlKey) return
      if (/^[1-9]$/.test(e.key) && repos[+e.key - 1]) {
        e.preventDefault()
        setRepoName(repos[+e.key - 1].name)
        setWinId(null)
      } else if (e.key === "i") setView("bandeja")
      else if (e.key === "v") setView("consola")
      else if (e.key === "g") setView("red")
      else if (e.key === "t") setView("tablero")
      else if (e.key === "s") setView("slices")
      else if (e.key === "r") setView("lupa")
      else if (e.key === "a") setActivityOpen(true)
      else if (e.key === "c") {
        e.preventDefault()
        setConsultorOpen(true)
      } else if (e.key === "l") {
        e.preventDefault()
        setLaunchOpen(true)
      } else if (e.key === "b") {
        e.preventDefault()
        setTreeOpen((v) => !v)
      } else if (e.key === "d") {
        e.preventDefault()
        setDark((v) => !v)
      } else if (e.key === "Escape") setOpenId(null)
    }
    addEventListener("keydown", onKey)
    return () => removeEventListener("keydown", onKey)
  }, [repos, setOpenId])

  useEffect(() => {
    document.title = (needAll ? `(${needAll}) ` : "") + (repo ? `${repo.name} · ` : "") + "Agencia Studio"
  }, [repo, needAll])

  // Loading / Disconnected / Empty State
  if (!state || !repo || !win) {
    return (
      <div className="grid h-svh place-items-center bg-canvas p-6">
        <div className="flex w-full max-w-md flex-col gap-4 rounded-xl border border-line-strong bg-surface p-6 shadow-[var(--shadow-md)]">
          <div className="flex items-center gap-3">
            <div className="grid size-9 place-items-center rounded-lg border border-line-strong bg-elevated font-mono text-[12px] font-bold text-ink-1">
              AG
            </div>
            <div>
              <h1 className="text-[15px] font-semibold text-ink-1">Agencia Studio</h1>
              <p className="text-[12.5px] text-ink-3">
                {!state
                  ? live === "off"
                    ? "Sin conexión con el servidor core (:8800)"
                    : "Conectando al flujo en vivo…"
                  : "No hay ventanas de agentes activas ahora"}
              </p>
            </div>
          </div>

          <div className="flex items-center gap-2 pt-2">
            <button
              type="button"
              onClick={() => setDemoMode(true)}
              className="pressable inline-flex h-8 items-center gap-1.5 rounded-md bg-ink-1 px-3.5 text-[12.5px] font-semibold text-ink-inv hover:opacity-90"
            >
              <FlaskConical className="size-3.5" />
              Cargar Modo Demo
            </button>
            <button
              type="button"
              onClick={() => location.reload()}
              className="pressable inline-flex h-8 items-center gap-1.5 rounded-md border border-line-strong bg-elevated px-3 text-[12.5px] font-medium text-ink-1 hover:bg-hover"
            >
              <Radio className="size-3.5" />
              Reintentar core
            </button>
          </div>
        </div>
      </div>
    )
  }

  const inspectorNode = openItem ? (
    <AgentInspector it={openItem} demoMode={demoMode} onClose={() => setOpenId(null)} />
  ) : (
    <div className="grid h-full place-items-center bg-surface px-8 text-center">
      <div>
        <p className="text-[13.5px] font-medium text-ink-2">Inspector de agente</p>
        <p className="mt-1 text-[12.5px] text-ink-3">
          {view === "bandeja"
            ? "Selecciona una prioridad de la bandeja para ver su informe y terminal."
            : "Selecciona cualquier fila de la consola para inspeccionar su ejecución."}
        </p>
      </div>
    </div>
  )

  return (
    <Tooltip.Provider delayDuration={200}>
      <div className="ag-grad-canvas flex h-svh w-full flex-col overflow-hidden text-ink-1 md:flex-row">
        {/* Left Architectural Navigation Rail */}
        <NavRail
          view={view}
          setView={(v) => {
            setView(v)
            if (v === "lupa" || v === "slices") setOpenId(null)
          }}
          needAll={needAll}
          live={live}
          dark={dark}
          onToggleTheme={() => setDark((d) => !d)}
          onLaunch={() => setLaunchOpen(true)}
          onConsultor={() => setConsultorOpen(true)}
          onActivity={() => setActivityOpen(true)}
          onPalette={() => setPaletteOpen(true)}
        />

        {/* Main Column: Top Telemetry Ribbon + Workspace */}
        <div className="flex min-w-0 flex-1 flex-col overflow-hidden">
          <TopRibbon
            view={view}
            repos={repos}
            repoName={effectiveRepoName}
            onSelectRepo={(name) => {
              setRepoName(name)
              setWinId(null)
              setOpenId(null)
            }}
            quota={state.quota}
            live={live}
            demoMode={demoMode}
            onToggleDemo={() => {
              setDemoMode((d) => !d)
              setRepoName(null)
              setWinId(null)
              setOpenId(null)
            }}
            showTreeToggle={view === "red"}
            treeOpen={treeOpen}
            onToggleTree={() => setTreeOpen((v) => !v)}
          />

          <main className="min-h-0 flex-1 overflow-hidden">
            <Group orientation="horizontal" className="h-full" defaultLayout={split.defaultLayout} onLayoutChanged={split.onLayoutChanged}>
              <Panel id="stage" minSize="42%">
                {view === "bandeja" && (
                  <BandejaView
                    repos={repos}
                    sel={openId}
                    onSel={setOpenId}
                    onConsola={() => setView("consola")}
                    bus={bus}
                    onOpenRed={() => setView("red")}
                  />
                )}
                {view === "consola" && (
                  <ConsolaView
                    repos={allRepos ? repos : [repo]}
                    all={allRepos}
                    sel={openId}
                    demoMode={demoMode}
                    onSel={setOpenId}
                  />
                )}
                {view === "red" && (
                  <RedView
                    repos={repos}
                    repo={repo}
                    win={win}
                    fil={fil}
                    setFil={setFil}
                    openId={openId}
                    treeOpen={treeOpen}
                    dark={dark}
                    demoMode={demoMode}
                    bus={bus}
                    onPickWin={(id) => {
                      setWinId(id)
                      setOpenId(null)
                    }}
                    onOpen={onOpen}
                    onLaunch={() => setLaunchOpen(true)}
                  />
                )}
                {view === "tablero" && (
                  <TableroView repos={allRepos ? repos : [repo]} avance={state.avance ?? {}} demoMode={demoMode} />
                )}
                {view === "slices" && <SlicesView repos={repos} onOpen={onOpen} demoMode={demoMode} />}
                {view === "lupa" && <LupaView repos={repos} repo={repo} onOpen={onOpen} demoMode={demoMode} />}
              </Panel>

              {showInspectorDock && (
                <>
                  {/* Same 1px line as before; the invisible ::before widens the grab area to 13px. */}
                  <Separator className="relative w-[1px] bg-line transition-colors hover:bg-work active:bg-work before:absolute before:inset-y-0 before:-inset-x-1.5 before:content-['']" />
                  <Panel id="inspector" defaultSize="38%" minSize="28%" maxSize="58%">
                    {inspectorNode}
                  </Panel>
                </>
              )}
            </Group>
          </main>
        </div>

        {/* Mobile Inspector Sheet */}
        {openItem && phone && (
          <div className="fixed inset-0 z-40 bg-surface">
            <AgentInspector it={openItem} demoMode={demoMode} onClose={() => setOpenId(null)} />
          </div>
        )}

        {/* Global Overlays */}
        <LaunchDialog
          open={launchOpen}
          onOpenChange={setLaunchOpen}
          cwd={win.cwd || "~"}
          demoMode={demoMode}
          homes={state.homes}
          quota={state.quota}
        />
        <ConsultorDrawer open={consultorOpen} onOpenChange={setConsultorOpen} demoMode={demoMode} />
        <ActivityDrawer open={activityOpen} onOpenChange={setActivityOpen} onOpenAgent={onOpen} demoMode={demoMode} />
        <CommandPalette
          open={paletteOpen}
          onOpenChange={setPaletteOpen}
          repos={repos}
          onWin={(r, w) => {
            setRepoName(r)
            setWinId(w)
            setOpenId(null)
            setView("red")
          }}
          onCard={(r, w, id) => {
            setRepoName(r)
            setWinId(w)
            setOpenId(id)
          }}
          onLaunch={() => setLaunchOpen(true)}
          onFilter={(f) => {
            setFil(f)
            setView("red")
          }}
        />
        <Toaster position="bottom-right" theme={dark ? "dark" : "light"} />
      </div>
    </Tooltip.Provider>
  )
}

export default App
