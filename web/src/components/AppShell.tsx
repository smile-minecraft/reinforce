import {
  Activity,
  BookOpen,
  ClipboardCheck,
  FileOutput,
  FileText,
  GraduationCap,
  Home,
  Layers,
  PanelLeft,
  PanelRight,
  Plus,
  Settings,
  User,
  X,
} from 'lucide-react'
import { useRef, useState, type ComponentType, type PointerEvent as ReactPointerEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, Outlet, useLocation } from 'react-router'
import { isMockMode } from '../api/client'
import { useAuthStatus, useMeta } from '../api/hooks/useAuth'
import { useWorkspace } from '../workspace/workspaceContextValue'
import type { WorkspaceTabKind } from '../workspace/workspaceLogic'

function tabIcon(kind: WorkspaceTabKind): ComponentType<{ className?: string }> {
  switch (kind) {
    case 'home':
      return Home
    case 'note':
      return FileText
    case 'card':
      return Layers
    case 'study':
    case 'session':
      return GraduationCap
    case 'analytics':
      return Activity
    case 'triage':
      return ClipboardCheck
    case 'export':
      return FileOutput
    case 'settings':
      return Settings
    case 'login':
      return User
  }
}

export function AppShell() {
  const { t } = useTranslation()
  const { data: authStatus } = useAuthStatus()
  const { data: meta } = useMeta()
  const location = useLocation()
  const mockActive = isMockMode()

  const {
    tabs,
    activeTabId,
    openTab,
    activateTab,
    closeTab,
    leftOpen,
    rightOpen,
    leftWidth,
    rightWidth,
    setLeftOpen,
    setRightOpen,
    setLeftWidth,
    setRightWidth,
    mobileLeftOpen,
    mobileRightOpen,
    setMobileLeftOpen,
    setMobileRightOpen,
    displayName,
    setRightPortalNode,
    setMobileRightPortalNode,
    hasCustomRightSidebar,
  } = useWorkspace()

  const [draggingSide, setDraggingSide] = useState<'left' | 'right' | null>(null)
  const dragStartRef = useRef<{ x: number; initialWidth: number }>({ x: 0, initialWidth: 224 })

  const needsSignIn = authStatus?.auth_mode === 'invite' && !authStatus.authenticated

  function startResize(side: 'left' | 'right', e: ReactPointerEvent<HTMLDivElement>) {
    e.preventDefault()
    e.currentTarget.setPointerCapture(e.pointerId)
    setDraggingSide(side)
    dragStartRef.current = {
      x: e.clientX,
      initialWidth: side === 'left' ? leftWidth : rightWidth,
    }
  }

  function moveResize(side: 'left' | 'right', e: ReactPointerEvent<HTMLDivElement>) {
    if (draggingSide !== side) return
    const dx = e.clientX - dragStartRef.current.x
    if (side === 'left') {
      setLeftWidth(dragStartRef.current.initialWidth + dx)
    } else {
      setRightWidth(dragStartRef.current.initialWidth - dx)
    }
  }

  function endResize(side: 'left' | 'right', e: ReactPointerEvent<HTMLDivElement>) {
    if (draggingSide !== side) return
    if (e.currentTarget.hasPointerCapture(e.pointerId)) {
      e.currentTarget.releasePointerCapture(e.pointerId)
    }
    setDraggingSide(null)
  }

  const navItems: Array<{
    id: string
    kind: WorkspaceTabKind
    path: string
    label: string
    icon: ComponentType<{ className?: string }>
    match: (pathname: string) => boolean
  }> = [
    {
      id: 'home',
      kind: 'home',
      path: '/',
      label: t('nav.reading'),
      icon: BookOpen,
      match: (p) => p === '/' || p.startsWith('/notes/') || p.startsWith('/cards/'),
    },
    {
      id: 'study',
      kind: 'study',
      path: '/study',
      label: t('nav.study'),
      icon: GraduationCap,
      match: (p) => p.startsWith('/study') || p === '/practical',
    },
    {
      id: 'analytics',
      kind: 'analytics',
      path: '/analytics',
      label: t('nav.analytics'),
      icon: Activity,
      match: (p) => p === '/analytics',
    },
    {
      id: 'triage',
      kind: 'triage',
      path: '/triage',
      label: t('nav.triage'),
      icon: ClipboardCheck,
      match: (p) => p.startsWith('/triage'),
    },
    {
      id: 'export',
      kind: 'export',
      path: '/export',
      label: t('nav.export'),
      icon: FileOutput,
      match: (p) => p === '/export',
    },
    {
      id: 'settings',
      kind: 'settings',
      path: '/settings',
      label: t('nav.settings'),
      icon: Settings,
      match: (p) => p === '/settings',
    },
  ]

  function renderLeftNav(onNavigate?: () => void) {
    return (
      <nav className="flex flex-1 flex-col gap-1 p-2.5" aria-label="Main navigation">
        {navItems.map((item) => {
          const Icon = item.icon
          const active = item.match(location.pathname)
          return (
            <button
              key={item.id}
              type="button"
              onClick={() => {
                openTab({
                  id: item.id,
                  kind: item.kind,
                  path: item.path,
                  title: item.label,
                })
                onNavigate?.()
              }}
              className={`flex min-h-10 items-center gap-2.5 rounded-lg px-3 text-left text-sm font-medium transition-colors ${
                active
                  ? 'bg-accent-soft text-accent'
                  : 'text-ink-muted hover:bg-subtle hover:text-ink'
              }`}
            >
              <Icon className="h-4 w-4 shrink-0" />
              <span className="truncate">{item.label}</span>
            </button>
          )
        })}

        <div className="mt-auto space-y-2 border-t border-border pt-3">
          {mockActive && (
            <div className="rounded-lg bg-warn-soft px-2.5 py-1.5 text-xs font-medium text-warn">
              {t('app.mockActive')}
            </div>
          )}
          <div className="px-2 text-[11px] text-ink-muted">
            {t('app.name')} {meta?.version ? `v${meta.version}` : ''}
          </div>
        </div>
      </nav>
    )
  }

  return (
    <div className="flex h-dvh flex-col overflow-hidden bg-canvas text-ink">
      {/* Unified Three-Band Top Navigation Bar (Brand | Multi-Tab Bar | User Profile) */}
      <header className="flex h-13 shrink-0 items-stretch border-b border-border bg-surface">
        {/* Left Brand Cell */}
        <div
          className="flex shrink-0 items-center justify-between gap-2 border-r border-border px-3 transition-[width] duration-150"
          style={leftOpen ? { width: undefined } : undefined}
        >
          <div
            className="hidden items-center justify-between gap-2 lg:flex"
            style={{ width: leftOpen ? `${leftWidth - 24}px` : 'auto' }}
          >
            <button
              type="button"
              onClick={() =>
                openTab({ id: 'home', kind: 'home', path: '/', title: t('workspace.tabs.home') })
              }
              className="flex min-w-0 items-baseline gap-1.5 text-left"
            >
              <span className="text-base font-bold tracking-tight text-ink">{t('app.name')}</span>
              {meta?.version && (
                <span className="font-mono text-[10px] text-ink-muted">{meta.version}</span>
              )}
            </button>
            <button
              type="button"
              onClick={() => setLeftOpen((v) => !v)}
              aria-label={t('workspace.sidebar.toggleLeft')}
              title={t('workspace.sidebar.toggleLeft')}
              className={`flex h-8 w-8 shrink-0 items-center justify-center rounded-lg transition-colors ${
                leftOpen
                  ? 'text-ink-muted hover:bg-subtle hover:text-ink'
                  : 'bg-accent-soft text-accent'
              }`}
            >
              <PanelLeft className="h-4 w-4" />
            </button>
          </div>

          {/* Mobile left drawer trigger */}
          <button
            type="button"
            onClick={() => setMobileLeftOpen(true)}
            aria-label={t('workspace.sidebar.toggleLeft')}
            className="flex h-9 w-9 items-center justify-center rounded-lg text-ink-muted hover:bg-subtle hover:text-ink lg:hidden"
          >
            <PanelLeft className="h-4 w-4" />
          </button>
        </div>

        {/* Center Multi-Tab Strip */}
        <div
          className="rf-pane-scroll flex min-w-0 flex-1 items-center gap-1.5 overflow-x-auto px-2.5 py-1.5"
          role="tablist"
          aria-label="Workspace tabs"
        >
          {tabs.map((tab) => {
            const Icon = tabIcon(tab.kind)
            const active = tab.id === activeTabId
            return (
              <div
                key={tab.id}
                role="tab"
                aria-selected={active}
                className={`group flex h-9 max-w-[210px] shrink-0 items-center gap-1.5 rounded-xl border px-2.5 text-xs font-medium transition-colors ${
                  active
                    ? 'border-accent/50 bg-accent-soft text-accent shadow-2xs'
                    : 'border-border bg-canvas/70 text-ink-muted hover:border-border hover:bg-subtle hover:text-ink'
                }`}
              >
                <button
                  type="button"
                  onClick={() => activateTab(tab.id)}
                  className="flex min-w-0 flex-1 items-center gap-1.5 truncate py-1 text-left"
                >
                  <Icon className="h-3.5 w-3.5 shrink-0" />
                  <span className="truncate">{tab.title}</span>
                </button>
                <button
                  type="button"
                  onClick={(e) => {
                    e.stopPropagation()
                    closeTab(tab.id)
                  }}
                  aria-label={`${t('workspace.tabs.closeTab')}: ${tab.title}`}
                  className="flex h-5 w-5 shrink-0 items-center justify-center rounded-md text-ink-muted/70 hover:bg-surface hover:text-ink"
                >
                  <X className="h-3 w-3" />
                </button>
              </div>
            )
          })}

          <button
            type="button"
            onClick={() =>
              openTab({ id: 'home', kind: 'home', path: '/', title: t('workspace.tabs.home') })
            }
            aria-label={t('workspace.tabs.newTab')}
            title={t('workspace.tabs.newTab')}
            className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg border border-dashed border-border text-ink-muted hover:border-accent hover:text-accent"
          >
            <Plus className="h-3.5 w-3.5" />
          </button>
        </div>

        {/* Right User Profile Cell */}
        <div className="flex shrink-0 items-center justify-between gap-2 border-l border-border px-3">
          <div
            className="hidden items-center justify-between gap-2 lg:flex"
            style={{ width: rightOpen ? `${rightWidth - 24}px` : 'auto' }}
          >
            <button
              type="button"
              onClick={() => setRightOpen((v) => !v)}
              aria-label={t('workspace.sidebar.toggleRight')}
              title={t('workspace.sidebar.toggleRight')}
              className={`flex h-8 w-8 shrink-0 items-center justify-center rounded-lg transition-colors ${
                rightOpen
                  ? 'text-ink-muted hover:bg-subtle hover:text-ink'
                  : 'bg-accent-soft text-accent'
              }`}
            >
              <PanelRight className="h-4 w-4" />
            </button>

            <button
              type="button"
              onClick={() =>
                openTab({
                  id: 'settings',
                  kind: 'settings',
                  path: '/settings',
                  title: t('workspace.tabs.settings'),
                })
              }
              className="flex min-w-0 items-center gap-2 rounded-lg px-1.5 py-1 hover:bg-subtle"
            >
              <span className="truncate text-sm font-medium text-ink">{displayName}</span>
              <span className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full border border-border bg-canvas text-ink-muted">
                <User className="h-3.5 w-3.5" />
              </span>
            </button>
          </div>

          {/* Mobile right drawer trigger */}
          <button
            type="button"
            onClick={() => setMobileRightOpen(true)}
            aria-label={t('workspace.sidebar.toggleRight')}
            className="flex h-9 w-9 items-center justify-center rounded-lg text-ink-muted hover:bg-subtle hover:text-ink lg:hidden"
          >
            <PanelRight className="h-4 w-4" />
          </button>
        </div>
      </header>

      {/* Invite Auth Notice Banner */}
      {needsSignIn && location.pathname !== '/login' && (
        <div className="shrink-0 border-b border-border bg-accent-soft px-4 py-2 text-xs text-ink">
          <div className="flex items-center justify-between gap-3">
            <span>{t('auth.subtitleInvite')}</span>
            <Link
              to="/login"
              className="inline-flex h-8 shrink-0 items-center rounded-lg bg-accent px-3 text-xs font-medium text-white hover:bg-accent-hover"
            >
              {t('nav.signIn')}
            </Link>
          </div>
        </div>
      )}

      {/* Three-Column Workbench Body */}
      <div className="flex min-h-0 flex-1 overflow-hidden">
        {/* Desktop Left Collapsible & Resizable Sidebar */}
        {leftOpen && (
          <>
            <aside
              style={{ width: `${leftWidth}px` }}
              className="rf-pane-scroll hidden shrink-0 flex-col overflow-y-auto bg-canvas lg:flex"
            >
              {renderLeftNav()}
            </aside>
            <div
              role="separator"
              aria-orientation="vertical"
              aria-label={t('workspace.sidebar.resizeLeft')}
              data-dragging={draggingSide === 'left'}
              onPointerDown={(e) => startResize('left', e)}
              onPointerMove={(e) => moveResize('left', e)}
              onPointerUp={(e) => endResize('left', e)}
              onPointerCancel={(e) => endResize('left', e)}
              className="rf-resize-handle hidden lg:block"
            />
          </>
        )}

        {/* Center Main Content Frame (All primary views live here) */}
        <main className="relative flex min-w-0 flex-1 flex-col overflow-hidden bg-surface">
          <Outlet />
        </main>

        {/* Desktop Right Collapsible & Resizable Context Sidebar */}
        {rightOpen && (
          <>
            <div
              role="separator"
              aria-orientation="vertical"
              aria-label={t('workspace.sidebar.resizeRight')}
              data-dragging={draggingSide === 'right'}
              onPointerDown={(e) => startResize('right', e)}
              onPointerMove={(e) => moveResize('right', e)}
              onPointerUp={(e) => endResize('right', e)}
              onPointerCancel={(e) => endResize('right', e)}
              className="rf-resize-handle hidden lg:block"
            />
            <aside
              style={{ width: `${rightWidth}px` }}
              className="hidden shrink-0 flex-col overflow-hidden bg-canvas lg:flex"
            >
              <div ref={setRightPortalNode} className="flex min-h-0 flex-1 flex-col overflow-hidden" />
              {!hasCustomRightSidebar && (
                <div className="flex flex-1 flex-col p-4 text-xs text-ink-muted">
                  <div className="font-semibold text-ink">
                    {t('workspace.sidebar.quickOverview')}
                  </div>
                  <p className="mt-2 leading-relaxed">{t('app.tagline')}</p>
                  <div className="mt-4 space-y-1.5 border-t border-border pt-3">
                    {tabs.map((tab) => (
                      <button
                        key={tab.id}
                        type="button"
                        onClick={() => activateTab(tab.id)}
                        className={`block w-full truncate rounded-md px-2 py-1.5 text-left transition-colors ${
                          tab.id === activeTabId
                            ? 'bg-accent-soft font-medium text-accent'
                            : 'hover:bg-subtle hover:text-ink'
                        }`}
                      >
                        {tab.title}
                      </button>
                    ))}
                  </div>
                </div>
              )}
            </aside>
          </>
        )}
      </div>

      {/* Mobile Left Drawer */}
      {mobileLeftOpen && (
        <div className="fixed inset-0 z-50 flex lg:hidden">
          <div
            className="fixed inset-0 bg-black/40 backdrop-blur-[1px]"
            onClick={() => setMobileLeftOpen(false)}
          />
          <aside className="relative z-10 flex h-full w-64 max-w-[82vw] flex-col border-r border-border bg-surface shadow-xl">
            <div className="flex h-13 items-center justify-between border-b border-border px-4">
              <span className="text-base font-bold tracking-tight text-ink">{t('app.name')}</span>
              <button
                type="button"
                onClick={() => setMobileLeftOpen(false)}
                aria-label={t('common.close')}
                className="flex h-9 w-9 items-center justify-center rounded-lg text-ink-muted hover:bg-subtle"
              >
                <X className="h-4 w-4" />
              </button>
            </div>
            {renderLeftNav(() => setMobileLeftOpen(false))}
          </aside>
        </div>
      )}

      {/* Mobile Right Drawer */}
      {mobileRightOpen && (
        <div className="fixed inset-0 z-50 flex justify-end lg:hidden">
          <div
            className="fixed inset-0 bg-black/40 backdrop-blur-[1px]"
            onClick={() => setMobileRightOpen(false)}
          />
          <aside className="relative z-10 flex h-full w-80 max-w-[88vw] flex-col border-l border-border bg-surface shadow-xl">
            <div className="flex h-13 items-center justify-between border-b border-border px-4">
              <span className="text-sm font-semibold text-ink">{displayName}</span>
              <button
                type="button"
                onClick={() => setMobileRightOpen(false)}
                aria-label={t('common.close')}
                className="flex h-9 w-9 items-center justify-center rounded-lg text-ink-muted hover:bg-subtle"
              >
                <X className="h-4 w-4" />
              </button>
            </div>
            <div
              ref={setMobileRightPortalNode}
              className="flex min-h-0 flex-1 flex-col overflow-hidden"
            />
            {!hasCustomRightSidebar && (
              <div className="flex flex-1 flex-col p-4 text-xs text-ink-muted">
                <div className="font-semibold text-ink">
                  {t('workspace.sidebar.quickOverview')}
                </div>
                <p className="mt-2 leading-relaxed">{t('app.tagline')}</p>
                <div className="mt-4 space-y-1.5 border-t border-border pt-3">
                  {tabs.map((tab) => (
                    <button
                      key={tab.id}
                      type="button"
                      onClick={() => {
                        activateTab(tab.id)
                        setMobileRightOpen(false)
                      }}
                      className={`block w-full truncate rounded-md px-2 py-1.5 text-left transition-colors ${
                        tab.id === activeTabId
                          ? 'bg-accent-soft font-medium text-accent'
                          : 'hover:bg-subtle hover:text-ink'
                      }`}
                    >
                      {tab.title}
                    </button>
                  ))}
                </div>
              </div>
            )}
          </aside>
        </div>
      )}
    </div>
  )
}
