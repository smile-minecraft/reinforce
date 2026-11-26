import { useParams } from 'react-router'

export function NotePage() {
  const { id } = useParams()
  return <div className="mx-auto max-w-4xl px-4 py-6 text-sm text-ink-muted">Note {id}</div>
}
