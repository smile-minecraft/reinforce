import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api } from '../client'
import type {
  AdjustPinsRequest,
  Proposal,
  ProposalStatus,
  UpdateOperationRequest,
} from '../types'

export const proposalKeys = {
  all: ['proposals'] as const,
  list: (status: ProposalStatus) => ['proposals', 'list', status] as const,
  detail: (id: string) => ['proposals', 'detail', id] as const,
}

export function useProposals(status: ProposalStatus = 'pending') {
  return useQuery({
    queryKey: proposalKeys.list(status),
    queryFn: () => api.listProposals(status),
  })
}

export function useProposal(id: string | undefined) {
  return useQuery({
    queryKey: proposalKeys.detail(id ?? ''),
    queryFn: () => api.getProposal(id!),
    enabled: Boolean(id),
  })
}

export function useApproveProposal() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ id, promoteVerified }: { id: string; promoteVerified: boolean }) =>
      api.approveProposal(id, { promote_verified: promoteVerified }),
    onSuccess: (updated) => {
      queryClient.setQueryData<Proposal>(proposalKeys.detail(updated.id), updated)
      void queryClient.invalidateQueries({ queryKey: proposalKeys.all })
    },
  })
}

export function useRejectProposal() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ id, reviewNote }: { id: string; reviewNote: string }) =>
      api.rejectProposal(id, { review_note: reviewNote }),
    onSuccess: (updated) => {
      queryClient.setQueryData<Proposal>(proposalKeys.detail(updated.id), updated)
      void queryClient.invalidateQueries({ queryKey: proposalKeys.all })
    },
  })
}

export function useAdjustProposalPins() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ id, body }: { id: string; body: AdjustPinsRequest }) =>
      api.adjustProposalPins(id, body),
    onSuccess: (_res, variables) => {
      void queryClient.invalidateQueries({ queryKey: proposalKeys.detail(variables.id) })
    },
  })
}

export function useUpdateProposalOperation() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({
      id,
      index,
      body,
    }: {
      id: string
      index: number
      body: UpdateOperationRequest
    }) => api.updateProposalOperation(id, index, body),
    onSuccess: (updated) => {
      queryClient.setQueryData<Proposal>(proposalKeys.detail(updated.id), updated)
      void queryClient.invalidateQueries({ queryKey: proposalKeys.all })
    },
  })
}
