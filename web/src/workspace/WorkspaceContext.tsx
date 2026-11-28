import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react'
import { createPortal } from 'react-dom'
import { useTranslation } from 'react-i18next'
import { useLocation, useNavigate } from 'react-router'
import {
  WorkspaceContext,
  useWorkspace,
  type WorkspaceContextValue,
} from './workspaceContextValue'
import {
  closeWorkspaceTab,
  openOrFocusTab,
  updateWorkspaceTabTitle,
  type WorkspaceTab,
  type WorkspaceTabKind,
} from './workspaceLogic'

const STORAGE_LAYOUT_KEY = 'reinforce:workspace:layout:v1'
const STORAGE_TABS_KEY = 'reinforce:workspace:tabs:v1'
const STORAGE_USER_NAME_KEY = 'reinforce:workspace:displayName:v1'

interface LayoutPrefs {
  leftOpen: boolean
  rightOpen: boolean
  leftWidth: number
  rightWidth: number
}

interface TabState {
  tabs: WorkspaceTab[]
  activeTabId: string
}

const DEFAULT_LAYOUT: LayoutPrefs = {
  leftOpen: true,
  rightOpen: true,
  leftWidth: 224,
  rightWidth: 272,
}

function clamp(v: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, v))
}

function loadLayoutPrefs(): LayoutPrefs {
  if (typeof window === 'undefined') return DEFAULT_LAYOUT
  try {
    const raw = window.localStorage.getItem(STORAGE_LAYOUT_KEY)
    if (!raw) return DEFAULT_LAYOUT
    const parsed = JSON.parse(raw) as Partial<LayoutPrefs>
    return {
      leftOpen: typeof parsed.leftOpen === 'boolean' ? parsed.leftOpen : DEFAULT_LAYOUT.leftOpen,
      rightOpen: typeof parsed.rightOpen === 'boolean' ? parsed.rightOpen : DEFAULT_LAYOUT.rightOpen,
      leftWidth: clamp(Number(parsed.leftWidth) || DEFAULT_LAYOUT.leftWidth, 180, 340),
      rightWidth: clamp(Number(parsed.rightWidth) || DEFAULT_LAYOUT.rightWidth, 220, 380),
    }
  } catch {
    return DEFAULT_LAYOUT
  }
}

function loadDisplayName(): string {
  if (typeof window === 'undefined') return 'Learner'
  try {
    return window.localStorage.getItem(STORAGE_USER_NAME_KEY)?.trim() || 'Learner'
  } catch {
    return 'Learner'
  }
}

function localizedBuiltinTitle(tabId: string, t: (key: string) => string): string | null {
  switch (tabId) {
    case 'home':
      return t('workspace.tabs.home')
    case 'study':
      return t('workspace.tabs.study')
    case 'analytics':
      return t('workspace.tabs.analytics')
    case 'export':
      return t('workspace.tabs.export')
    case 'settings':
      return t('workspace.tabs.settings')
    case 'login':
      return t('nav.signIn')
    default:
      return null
  }
}

function routeToTabDescriptor(
  pathname: string,
  t: (key: string) => string,
): WorkspaceTab {
  if (pathname === '/' || pathname === '') {
    return { id: 'home', kind: 'home', path: '/', title: t('workspace.tabs.home') }
  }
  const noteMatch = pathname.match(/^\/notes\/([^/]+)$/)
  if (noteMatch) {
    const id = decodeURIComponent(noteMatch[1])
    return { id: `note:${id}`, kind: 'note', path: pathname, title: t('workspace.tabs.note') }
  }
  const cardMatch = pathname.match(/^\/cards\/([^/]+)$/)
  if (cardMatch) {
    const id = decodeURIComponent(cardMatch[1])
    return { id: `card:${id}`, kind: 'card', path: pathname, title: t('workspace.tabs.card') }
  }
  const sessionMatch = pathname.match(/^\/study\/session\/([^/]+)$/)
  if (sessionMatch) {
    const id = decodeURIComponent(sessionMatch[1])
    return {
      id: `session:${id}`,
      kind: 'session',
      path: pathname,
      title: t('workspace.tabs.review'),
    }
  }
  if (pathname === '/study' || pathname === '/practical') {
    return { id: 'study', kind: 'study', path: pathname, title: t('workspace.tabs.study') }
  }
  if (pathname === '/analytics') {
    return { id: 'analytics', kind: 'analytics', path: '/analytics', title: t('workspace.tabs.analytics') }
  }
  if (pathname === '/triage' || pathname.startsWith('/triage/')) {
    return { id: `triage:${pathname}`, kind: 'triage', path: pathname, title: t('workspace.tabs.triage') }
  }
  if (pathname === '/export') {
    return { id: 'export', kind: 'export', path: '/export', title: t('workspace.tabs.export') }
  }
  if (pathname === '/settings') {
    return { id: 'settings', kind: 'settings', path: '/settings', title: t('workspace.tabs.settings') }
  }
  if (pathname === '/login') {
    return { id: 'login', kind: 'login', path: '/login', title: t('nav.signIn') }
  }
  return { id: `route:${pathname}`, kind: 'home', path: pathname, title: t('workspace.tabs.home') }
}

function loadSavedTabs(fallback: WorkspaceTab): WorkspaceTab[] {
  if (typeof window === 'undefined') return [fallback]
  try {
    const raw = window.sessionStorage.getItem(STORAGE_TABS_KEY)
    if (!raw) return [fallback]
    const parsed = JSON.parse(raw)
    if (!Array.isArray(parsed) || parsed.length === 0) return [fallback]
    const valid = parsed.filter(
      (item): item is WorkspaceTab =>
        typeof item === 'object' &&
        item !== null &&
        typeof item.id === 'string' &&
        typeof item.path === 'string' &&
        typeof item.title === 'string',
    )
    return valid.length > 0 ? valid : [fallback]
  } catch {
    return [fallback]
  }
}

export function WorkspaceProvider({ children }: { children: ReactNode }) {
  const { t } = useTranslation()
  const location = useLocation()
  const navigate = useNavigate()

  const homeFallback = useMemo<WorkspaceTab>(
    () => ({ id: 'home', kind: 'home', path: '/', title: t('workspace.tabs.home') }),
    [t],
  )

  const [tabState, setTabState] = useState<TabState>(() => {
    const initial = routeToTabDescriptor(location.pathname, t)
    const saved = loadSavedTabs(homeFallback)
    const result = openOrFocusTab(saved, initial)
    return { tabs: result.tabs, activeTabId: result.activeId }
  })

  const [layout, setLayout] = useState<LayoutPrefs>(loadLayoutPrefs)
  const [mobileLeftOpen, setMobileLeftOpen] = useState(false)
  const [mobileRightOpen, setMobileRightOpen] = useState(false)
  const [displayName, setDisplayNameState] = useState<string>(loadDisplayName)

  const [rightPortalNode, setRightPortalNode] = useState<HTMLDivElement | null>(null)
  const [mobileRightPortalNode, setMobileRightPortalNode] = useState<HTMLDivElement | null>(null)
  const [rightSidebarCount, setRightSidebarCount] = useState(0)

  const pendingTabTitleRef = useRef<Map<string, string>>(new Map())

  useEffect(() => {
    try {
      window.localStorage.setItem(STORAGE_LAYOUT_KEY, JSON.stringify(layout))
    } catch {
      // Ignore quota errors
    }
  }, [layout])

  useEffect(() => {
    try {
      window.sessionStorage.setItem(STORAGE_TABS_KEY, JSON.stringify(tabState.tabs))
    } catch {
      // Ignore quota errors
    }
  }, [tabState.tabs])

  // Sync URL navigation -> atomic { tabs, activeTabId } update
  useEffect(() => {
    const descriptor = routeToTabDescriptor(location.pathname, t)
    const navState = location.state as { tabTitle?: string } | null
    const pendingTitle = pendingTabTitleRef.current.get(descriptor.id)
    if (pendingTitle) {
      pendingTabTitleRef.current.delete(descriptor.id)
    }

    setTabState((prev) => {
      const existing = prev.tabs.find(
        (item) => item.id === descriptor.id || item.path === descriptor.path,
      )
      const resolvedTitle =
        navState?.tabTitle ||
        pendingTitle ||
        (existing ? existing.title : descriptor.title)
      const result = openOrFocusTab(prev.tabs, { ...descriptor, title: resolvedTitle })
      return { tabs: result.tabs, activeTabId: result.activeId }
    })
  }, [location.pathname, location.state, t])

  const openTab = useCallback(
    (tab: {
      id: string
      kind: WorkspaceTabKind
      path: string
      title: string
      navState?: unknown
    }) => {
      pendingTabTitleRef.current.set(tab.id, tab.title)
      setTabState((prev) => {
        const result = openOrFocusTab(prev.tabs, {
          id: tab.id,
          kind: tab.kind,
          path: tab.path,
          title: tab.title,
        })
        return { tabs: result.tabs, activeTabId: result.activeId }
      })
      setMobileLeftOpen(false)
      void navigate(tab.path, {
        state:
          tab.navState && typeof tab.navState === 'object'
            ? { ...(tab.navState as Record<string, unknown>), tabTitle: tab.title }
            : { tabTitle: tab.title },
      })
    },
    [navigate],
  )

  const activateTab = useCallback(
    (tabId: string) => {
      const target = tabState.tabs.find((item) => item.id === tabId)
      if (!target) return
      setTabState((prev) => ({ ...prev, activeTabId: target.id }))
      if (location.pathname !== target.path) {
        void navigate(target.path)
      }
    },
    [tabState.tabs, location.pathname, navigate],
  )

  const closeTab = useCallback(
    (tabId: string) => {
      const result = closeWorkspaceTab(tabState.tabs, tabState.activeTabId, tabId, homeFallback)
      setTabState({ tabs: result.tabs, activeTabId: result.activeId })
      if (location.pathname !== result.activePath) {
        void navigate(result.activePath)
      }
    },
    [tabState.tabs, tabState.activeTabId, homeFallback, location.pathname, navigate],
  )

  const setTabTitle = useCallback((tabId: string, title: string) => {
    setTabState((prev) => {
      const nextTabs = updateWorkspaceTabTitle(prev.tabs, tabId, title)
      return nextTabs === prev.tabs ? prev : { ...prev, tabs: nextTabs }
    })
  }, [])

  const setLeftOpen = useCallback((open: boolean | ((v: boolean) => boolean)) => {
    setLayout((prev) => ({
      ...prev,
      leftOpen: typeof open === 'function' ? open(prev.leftOpen) : open,
    }))
  }, [])

  const setRightOpen = useCallback((open: boolean | ((v: boolean) => boolean)) => {
    setLayout((prev) => ({
      ...prev,
      rightOpen: typeof open === 'function' ? open(prev.rightOpen) : open,
    }))
  }, [])

  const setLeftWidth = useCallback((width: number) => {
    setLayout((prev) => ({ ...prev, leftWidth: clamp(Math.round(width), 180, 340) }))
  }, [])

  const setRightWidth = useCallback((width: number) => {
    setLayout((prev) => ({ ...prev, rightWidth: clamp(Math.round(width), 220, 380) }))
  }, [])

  const setDisplayName = useCallback((name: string) => {
    const trimmed = name.trim() || 'Learner'
    setDisplayNameState(trimmed)
    try {
      window.localStorage.setItem(STORAGE_USER_NAME_KEY, trimmed)
    } catch {
      // Ignore quota errors
    }
  }, [])

  const registerRightSidebar = useCallback(() => {
    setRightSidebarCount((c) => c + 1)
    return () => setRightSidebarCount((c) => Math.max(0, c - 1))
  }, [])

  // Derive localized built-in tab titles at render time (no setState inside useEffect)
  const displayTabs = useMemo<WorkspaceTab[]>(
    () =>
      tabState.tabs.map((tab) => {
        const localized = localizedBuiltinTitle(tab.id, t)
        return localized && tab.title !== localized ? { ...tab, title: localized } : tab
      }),
    [tabState.tabs, t],
  )

  const value = useMemo<WorkspaceContextValue>(
    () => ({
      tabs: displayTabs,
      activeTabId: tabState.activeTabId,
      openTab,
      activateTab,
      closeTab,
      setTabTitle,
      leftOpen: layout.leftOpen,
      rightOpen: layout.rightOpen,
      leftWidth: layout.leftWidth,
      rightWidth: layout.rightWidth,
      setLeftOpen,
      setRightOpen,
      setLeftWidth,
      setRightWidth,
      mobileLeftOpen,
      mobileRightOpen,
      setMobileLeftOpen,
      setMobileRightOpen,
      displayName,
      setDisplayName,
      rightPortalNode,
      setRightPortalNode,
      mobileRightPortalNode,
      setMobileRightPortalNode,
      hasCustomRightSidebar: rightSidebarCount > 0,
      registerRightSidebar,
    }),
    [
      displayTabs,
      tabState.activeTabId,
      openTab,
      activateTab,
      closeTab,
      setTabTitle,
      layout,
      setLeftOpen,
      setRightOpen,
      setLeftWidth,
      setRightWidth,
      mobileLeftOpen,
      mobileRightOpen,
      displayName,
      setDisplayName,
      rightPortalNode,
      mobileRightPortalNode,
      rightSidebarCount,
      registerRightSidebar,
    ],
  )

  return <WorkspaceContext.Provider value={value}>{children}</WorkspaceContext.Provider>
}

export function RightSidebarSlot({ children }: { children: ReactNode }) {
  const { rightPortalNode, mobileRightPortalNode, mobileRightOpen, registerRightSidebar } =
    useWorkspace()

  useEffect(() => registerRightSidebar(), [registerRightSidebar])

  return (
    <>
      {rightPortalNode ? createPortal(children, rightPortalNode) : null}
      {mobileRightOpen && mobileRightPortalNode
        ? createPortal(children, mobileRightPortalNode)
        : null}
    </>
  )
}
