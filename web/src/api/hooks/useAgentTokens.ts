import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api } from '../client'

export const agentTokenKeys = {
  all: ['auth', 'agent-tokens'] as const,
}

export function useAgentTokens(enabled: boolean) {
  return useQuery({
    queryKey: agentTokenKeys.all,
    queryFn: api.listAgentTokens,
    enabled,
    retry: false,
  })
}

export function useCreateAgentToken() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: api.createAgentToken,
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: agentTokenKeys.all })
    },
  })
}

export function useRevokeAgentToken() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: api.revokeAgentToken,
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: agentTokenKeys.all })
    },
  })
}
