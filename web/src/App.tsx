import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { lazy, Suspense } from 'react'
import { BrowserRouter, Navigate, Route, Routes } from 'react-router'
import { AppShell } from './components/AppShell'
import { ThemeProvider } from './components/ThemeProvider'
import { ReadingHomePage } from './pages/ReadingHomePage'

const NotePage = lazy(() => import('./pages/NotePage').then((m) => ({ default: m.NotePage })))
const CardPage = lazy(() => import('./pages/CardPage').then((m) => ({ default: m.CardPage })))
const StudyHubPage = lazy(() =>
  import('./pages/StudyHubPage').then((m) => ({ default: m.StudyHubPage })),
)
const StudyRunnerPage = lazy(() =>
  import('./pages/StudyRunnerPage').then((m) => ({ default: m.StudyRunnerPage })),
)
const LoginPage = lazy(() => import('./pages/LoginPage').then((m) => ({ default: m.LoginPage })))
const TriageListPage = lazy(() =>
  import('./pages/TriageListPage').then((m) => ({ default: m.TriageListPage })),
)
const TriageDetailPage = lazy(() =>
  import('./pages/TriageDetailPage').then((m) => ({ default: m.TriageDetailPage })),
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
  return <div className="py-16 text-center text-sm text-ink-muted">Loading…</div>
}

export function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <ThemeProvider>
        <BrowserRouter>
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
                <Route path="triage" element={<TriageListPage />} />
                <Route path="triage/:id" element={<TriageDetailPage />} />
                <Route path="login" element={<LoginPage />} />
                <Route path="*" element={<Navigate to="/" replace />} />
              </Route>
            </Routes>
          </Suspense>
        </BrowserRouter>
      </ThemeProvider>
    </QueryClientProvider>
  )
}
