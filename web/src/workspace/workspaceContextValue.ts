import { createContext, useContext } from 'react'
import type { WorkspaceTab, WorkspaceTabKind } from './workspaceLogic'

export interface WorkspaceContextValue {
  tabs: WorkspaceTab[]
  activeTabId: string
  openTab: (tab: {
    id: string
    kind: WorkspaceTabKind
    path: string
    title: string
    navState?: unknown
  }) => void
  activateTab: (tabId: string) => void
  closeTab: (tabId: string) => void
  setTabTitle: (tabId: string, title: string) => void

  leftOpen: boolean
  rightOpen: boolean
  leftWidth: number
  rightWidth: number
  setLeftOpen: (open: boolean | ((v: boolean) => boolean)) => void
  setRightOpen: (open: boolean | ((v: boolean) => boolean)) => void
  setLeftWidth: (width: number) => void
  setRightWidth: (width: number) => void

  mobileLeftOpen: boolean
  mobileRightOpen: boolean
  setMobileLeftOpen: (open: boolean) => void
  setMobileRightOpen: (open: boolean) => void

  displayName: string
  setDisplayName: (name: string) => void

  rightPortalNode: HTMLDivElement | null
  setRightPortalNode: (node: HTMLDivElement | null) => void
  mobileRightPortalNode: HTMLDivElement | null
  setMobileRightPortalNode: (node: HTMLDivElement | null) => void
  hasCustomRightSidebar: boolean
  registerRightSidebar: () => () => void
}

export const WorkspaceContext = createContext<WorkspaceContextValue | null>(null)

export function useWorkspace(): WorkspaceContextValue {
  const ctx = useContext(WorkspaceContext)
  if (!ctx) {
    throw new Error('useWorkspace must be used inside WorkspaceProvider')
  }
  return ctx
}
