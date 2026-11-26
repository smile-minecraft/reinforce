import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { BrowserRouter, Navigate, Route, Routes } from 'react-router'
import { AppShell } from './components/AppShell'
import { ThemeProvider } from './components/ThemeProvider'
import { CardPage } from './pages/CardPage'
import { LoginPage } from './pages/LoginPage'
import { NotePage } from './pages/NotePage'
import { ReadingHomePage } from './pages/ReadingHomePage'
import { StudyHubPage } from './pages/StudyHubPage'
import { StudyRunnerPage } from './pages/StudyRunnerPage'

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 30_000,
      retry: 1,
    },
  },
})

export function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <ThemeProvider>
        <BrowserRouter>
          <Routes>
            <Route element={<AppShell />}>
              <Route index element={<ReadingHomePage />} />
              <Route path="notes/:id" element={<NotePage />} />
              <Route path="cards/:id" element={<CardPage />} />
              <Route path="study" element={<StudyHubPage />} />
              <Route path="practical" element={<StudyHubPage defaultMode="practical_exam" />} />
              <Route path="study/session/:id" element={<StudyRunnerPage />} />
              <Route path="login" element={<LoginPage />} />
              <Route path="*" element={<Navigate to="/" replace />} />
            </Route>
          </Routes>
        </BrowserRouter>
      </ThemeProvider>
    </QueryClientProvider>
  )
}
