import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { lazy, Suspense, type ReactNode } from 'react'
import { BrowserRouter, Navigate, Route, Routes } from 'react-router'
import { AppShell } from './components/AppShell'
import { ThemeProvider } from './components/ThemeProvider'
import { ReadingHomePage } from './pages/ReadingHomePage'
import { WorkspaceProvider } from './workspace/WorkspaceContext'

const NotePage = lazy(() => import('./pages/NotePage').then((m) => ({ default: m.NotePage })))
const CardPage = lazy(() => import('./pages/CardPage').then((m) => ({ default: m.CardPage })))
const StudyHubPage = lazy(() =>
  import('./pages/StudyHubPage').then((m) => ({ default: m.StudyHubPage })),
)
const StudyRunnerPage = lazy(() =>
  import('./pages/StudyRunnerPage').then((m) => ({ default: m.StudyRunnerPage })),
)
const AnalyticsPage = lazy(() =>
  import('./pages/AnalyticsPage').then((m) => ({ default: m.AnalyticsPage })),
)
const SettingsPage = lazy(() =>
  import('./pages/SettingsPage').then((m) => ({ default: m.SettingsPage })),
)
const LoginPage = lazy(() => import('./pages/LoginPage').then((m) => ({ default: m.LoginPage })))
const TriageListPage = lazy(() =>
  import('./pages/TriageListPage').then((m) => ({ default: m.TriageListPage })),
)
const TriageDetailPage = lazy(() =>
  import('./pages/TriageDetailPage').then((m) => ({ default: m.TriageDetailPage })),
)
const ExportCenterPage = lazy(() =>
  import('./pages/ExportCenterPage').then((m) => ({ default: m.ExportCenterPage })),
)

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 30_000,
      retry: 1,
    },
  },
})

function PageFallback() {
  return (
    <div className="flex flex-1 items-center justify-center text-sm text-ink-muted">Loading…</div>
  )
}

function ScrollPane({ children }: { children: ReactNode }) {
  return <div className="rf-pane-scroll flex-1 overflow-y-auto">{children}</div>
}

export function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <ThemeProvider>
        <BrowserRouter>
          <WorkspaceProvider>
            <Suspense fallback={<PageFallback />}>
              <Routes>
                <Route element={<AppShell />}>
                  <Route index element={<ReadingHomePage />} />
                  <Route path="notes/:id" element={<NotePage />} />
                  <Route path="cards/:id" element={<CardPage />} />
                  <Route path="study" element={<StudyHubPage key="study" />} />
                  <Route
                    path="practical"
                    element={<StudyHubPage key="practical" defaultMode="practical_exam" />}
                  />
                  <Route path="study/session/:id" element={<StudyRunnerPage />} />
                  <Route path="analytics" element={<AnalyticsPage />} />
                  <Route
                    path="triage"
                    element={
                      <ScrollPane>
                        <TriageListPage />
                      </ScrollPane>
                    }
                  />
                  <Route
                    path="triage/:id"
                    element={
                      <ScrollPane>
                        <TriageDetailPage />
                      </ScrollPane>
                    }
                  />
                  <Route
                    path="export"
                    element={
                      <ScrollPane>
                        <ExportCenterPage />
                      </ScrollPane>
                    }
                  />
                  <Route path="settings" element={<SettingsPage />} />
                  <Route
                    path="login"
                    element={
                      <ScrollPane>
                        <LoginPage />
                      </ScrollPane>
                    }
                  />
                  <Route path="*" element={<Navigate to="/" replace />} />
                </Route>
              </Routes>
            </Suspense>
          </WorkspaceProvider>
        </BrowserRouter>
      </ThemeProvider>
    </QueryClientProvider>
  )
}
