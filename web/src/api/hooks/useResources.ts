import { useQuery } from '@tanstack/react-query'
import { api } from '../client'

export const resourceKeys = {
  resource: (id: string) => ['resources', id] as const,
  anchor: (id: string) => ['anchors', id] as const,
}

export function useResource(id: string | undefined) {
  return useQuery({
    queryKey: resourceKeys.resource(id ?? ''),
    queryFn: () => api.getResource(id!),
    enabled: Boolean(id),
    staleTime: 5 * 60_000,
  })
}

export function useAnchor(id: string | undefined) {
  return useQuery({
    queryKey: resourceKeys.anchor(id ?? ''),
    queryFn: () => api.getAnchor(id!),
    enabled: Boolean(id),
    staleTime: 5 * 60_000,
  })
}
