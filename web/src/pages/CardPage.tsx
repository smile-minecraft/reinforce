import { useParams } from 'react-router'

export function CardPage() {
  const { id } = useParams()
  return <div className="mx-auto max-w-4xl px-4 py-6 text-sm text-ink-muted">Card {id}</div>
}
