import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api, ApiError } from '../client'
import type {
  CreateSubjectGroupRequest,
  SubjectGroup,
  UpdateSubjectGroupRequest,
} from '../types'

const LOCAL_GROUPS_KEY = 'reinforce:subjectGroups:fallback'

function readLocalGroups(): SubjectGroup[] {
  if (typeof window === 'undefined') return []
  try {
    const raw = window.localStorage.getItem(LOCAL_GROUPS_KEY)
    if (!raw) return []
    const parsed = JSON.parse(raw)
    return Array.isArray(parsed) ? (parsed as SubjectGroup[]) : []
  } catch {
    return []
  }
}

function writeLocalGroups(groups: SubjectGroup[]) {
  if (typeof window === 'undefined') return
  try {
    window.localStorage.setItem(LOCAL_GROUPS_KEY, JSON.stringify(groups))
  } catch {
    // Ignore storage quota errors
  }
}

function isRouteUnavailable(err: unknown): boolean {
  return err instanceof ApiError && (err.status === 404 || err.status === 405 || err.status === 501)
}

export const subjectGroupKeys = {
  all: ['subject_groups'] as const,
}

export function useSubjectGroups() {
  return useQuery({
    queryKey: subjectGroupKeys.all,
    queryFn: async () => {
      try {
        return await api.listSubjectGroups()
      } catch (err) {
        if (isRouteUnavailable(err)) {
          return readLocalGroups()
        }
        throw err
      }
    },
  })
}

export function useCreateSubjectGroup() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: async (body: CreateSubjectGroupRequest): Promise<SubjectGroup> => {
      try {
        return await api.createSubjectGroup(body)
      } catch (err) {
        if (!isRouteUnavailable(err)) throw err
        const now = new Date().toISOString()
        const next: SubjectGroup = {
          id: `local-sg-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`,
          parent_id: body.parent_id ?? null,
          name: body.name.trim(),
          sort_order: body.sort_order ?? 0,
          subjects: Array.from(new Set(body.subjects ?? [])).sort(),
          created_at: now,
          updated_at: now,
        }
        const existing = readLocalGroups()
        writeLocalGroups([...existing, next])
        return next
      }
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: subjectGroupKeys.all })
    },
  })
}

export function useUpdateSubjectGroup() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: async ({
      id,
      body,
    }: {
      id: string
      body: UpdateSubjectGroupRequest
    }): Promise<SubjectGroup> => {
      try {
        return await api.updateSubjectGroup(id, body)
      } catch (err) {
        if (!isRouteUnavailable(err)) throw err
        const existing = readLocalGroups()
        const now = new Date().toISOString()
        let updated: SubjectGroup | null = null
        const nextList = existing.map((group) => {
          if (group.id !== id) return group
          updated = {
            ...group,
            parent_id: body.parent_id !== undefined ? body.parent_id : group.parent_id,
            name: body.name !== undefined ? body.name.trim() : group.name,
            sort_order: body.sort_order !== undefined ? body.sort_order : group.sort_order,
            subjects:
              body.subjects !== undefined
                ? Array.from(new Set(body.subjects)).sort()
                : group.subjects,
            updated_at: now,
          }
          return updated
        })
        if (!updated) throw err
        writeLocalGroups(nextList)
        return updated
      }
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: subjectGroupKeys.all })
    },
  })
}

export function useDeleteSubjectGroup() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: async (id: string) => {
      try {
        return await api.deleteSubjectGroup(id)
      } catch (err) {
        if (!isRouteUnavailable(err)) throw err
        const existing = readLocalGroups()
        const toRemove = new Set<string>([id])
        let changed = true
        while (changed) {
          changed = false
          for (const g of existing) {
            if (g.parent_id && toRemove.has(g.parent_id) && !toRemove.has(g.id)) {
              toRemove.add(g.id)
              changed = true
            }
          }
        }
        writeLocalGroups(existing.filter((g) => !toRemove.has(g.id)))
        return { ok: true }
      }
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: subjectGroupKeys.all })
    },
  })
}
