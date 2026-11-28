import {
  ChevronDown,
  ChevronRight,
  Folder,
  FolderOpen,
  FolderPlus,
  Pencil,
  Play,
  Plus,
  Trash2,
  X,
} from 'lucide-react'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  useCreateSubjectGroup,
  useDeleteSubjectGroup,
  useUpdateSubjectGroup,
} from '../api/hooks/useSubjectGroups'
import type { SubjectGroup } from '../api/types'
import {
  buildSubjectGroupTree,
  flattenSubjectGroupTree,
  isDescendantOrSelf,
  type SubjectGroupNode,
} from '../workspace/workspaceLogic'

interface SubjectGroupTreeProps {
  groups: SubjectGroup[]
  availableSubjects: string[]
  selectedGroupId: string | null
  onSelectGroup: (groupId: string | null) => void
  onReviewGroup?: (group: SubjectGroup, subjects: string[]) => void
  editable?: boolean
  compact?: boolean
}

interface EditorState {
  mode: 'create' | 'edit'
  groupId?: string
  parentId: string | null
  name: string
  subjects: string[]
}

export function SubjectGroupTree({
  groups,
  availableSubjects,
  selectedGroupId,
  onSelectGroup,
  onReviewGroup,
  editable = true,
  compact = false,
}: SubjectGroupTreeProps) {
  const { t } = useTranslation()
  const createGroup = useCreateSubjectGroup()
  const updateGroup = useUpdateSubjectGroup()
  const deleteGroup = useDeleteSubjectGroup()

  const [collapsedIds, setCollapsedIds] = useState<Set<string>>(() => new Set())
  const [editor, setEditor] = useState<EditorState | null>(null)
  const [formError, setFormError] = useState<string | null>(null)

  const tree = useMemo(() => buildSubjectGroupTree(groups), [groups])
  const flatNodes = useMemo(() => flattenSubjectGroupTree(tree), [tree])

  function toggleCollapse(id: string) {
    setCollapsedIds((prev) => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }

  function openCreate(parentId: string | null) {
    setFormError(null)
    setEditor({
      mode: 'create',
      parentId,
      name: '',
      subjects: [],
    })
  }

  function openEdit(group: SubjectGroup) {
    setFormError(null)
    setEditor({
      mode: 'edit',
      groupId: group.id,
      parentId: group.parent_id,
      name: group.name,
      subjects: [...group.subjects],
    })
  }

  function toggleEditorSubject(subject: string) {
    setEditor((prev) => {
      if (!prev) return prev
      const exists = prev.subjects.includes(subject)
      return {
        ...prev,
        subjects: exists
          ? prev.subjects.filter((s) => s !== subject)
          : [...prev.subjects, subject],
      }
    })
  }

  async function handleSaveEditor() {
    if (!editor) return
    const trimmed = editor.name.trim()
    if (!trimmed) {
      setFormError(t('workspace.groups.namePlaceholder'))
      return
    }
    setFormError(null)
    try {
      if (editor.mode === 'create') {
        const created = await createGroup.mutateAsync({
          parent_id: editor.parentId,
          name: trimmed,
          subjects: editor.subjects,
        })
        onSelectGroup(created.id)
      } else if (editor.groupId) {
        await updateGroup.mutateAsync({
          id: editor.groupId,
          body: {
            parent_id: editor.parentId,
            name: trimmed,
            subjects: editor.subjects,
          },
        })
      }
      setEditor(null)
    } catch (err) {
      setFormError(err instanceof Error ? err.message : t('common.errorGeneric'))
    }
  }

  async function handleDelete(group: SubjectGroup) {
    if (!window.confirm(t('workspace.groups.deleteConfirm'))) return
    await deleteGroup.mutateAsync(group.id)
    if (selectedGroupId === group.id) {
      onSelectGroup(null)
    }
  }

  function renderNode(node: SubjectGroupNode) {
    const isCollapsed = collapsedIds.has(node.group.id)
    const isSelected = selectedGroupId === node.group.id
    const hasChildren = node.children.length > 0

    return (
      <div key={node.group.id}>
        <div
          style={{ paddingLeft: `${node.depth * 14 + 6}px` }}
          className={`group flex items-center gap-1 rounded-lg pr-1.5 transition-colors ${
            compact ? 'min-h-8 py-1' : 'min-h-9 py-1.5'
          } ${
            isSelected
              ? 'bg-accent-soft text-accent'
              : 'text-ink hover:bg-subtle'
          }`}
        >
          <button
            type="button"
            onClick={() => hasChildren && toggleCollapse(node.group.id)}
            className={`flex h-5 w-5 shrink-0 items-center justify-center rounded ${
              hasChildren ? 'text-ink-muted hover:text-ink' : 'opacity-0 pointer-events-none'
            }`}
            aria-label={isCollapsed ? 'Expand' : 'Collapse'}
          >
            {isCollapsed ? (
              <ChevronRight className="h-3.5 w-3.5" />
            ) : (
              <ChevronDown className="h-3.5 w-3.5" />
            )}
          </button>

          <button
            type="button"
            onClick={() => onSelectGroup(node.group.id)}
            className="flex min-w-0 flex-1 items-center gap-1.5 text-left"
          >
            {isSelected || (!isCollapsed && hasChildren) ? (
              <FolderOpen className="h-3.5 w-3.5 shrink-0 text-accent" />
            ) : (
              <Folder className="h-3.5 w-3.5 shrink-0 text-ink-muted" />
            )}
            <span className="truncate text-xs font-medium">{node.group.name}</span>
            <span className="ml-auto shrink-0 rounded bg-surface/80 px-1.5 py-0.5 font-mono text-[10px] text-ink-muted">
              {node.resolvedSubjects.length}
            </span>
          </button>

          {onReviewGroup && node.resolvedSubjects.length > 0 && (
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation()
                onReviewGroup(node.group, node.resolvedSubjects)
              }}
              title={t('workspace.groups.reviewThisGroup')}
              className="flex h-6 w-6 shrink-0 items-center justify-center rounded text-accent opacity-80 hover:bg-surface hover:opacity-100"
            >
              <Play className="h-3 w-3 fill-current" />
            </button>
          )}

          {editable && (
            <div className="hidden shrink-0 items-center gap-0.5 group-hover:flex group-focus-within:flex">
              <button
                type="button"
                onClick={(e) => {
                  e.stopPropagation()
                  openCreate(node.group.id)
                }}
                title={t('workspace.groups.createChild')}
                className="flex h-6 w-6 items-center justify-center rounded text-ink-muted hover:bg-surface hover:text-ink"
              >
                <FolderPlus className="h-3 w-3" />
              </button>
              <button
                type="button"
                onClick={(e) => {
                  e.stopPropagation()
                  openEdit(node.group)
                }}
                title={t('workspace.groups.edit')}
                className="flex h-6 w-6 items-center justify-center rounded text-ink-muted hover:bg-surface hover:text-ink"
              >
                <Pencil className="h-3 w-3" />
              </button>
              <button
                type="button"
                onClick={(e) => {
                  e.stopPropagation()
                  void handleDelete(node.group)
                }}
                title={t('workspace.groups.delete')}
                className="flex h-6 w-6 items-center justify-center rounded text-ink-muted hover:bg-bad-soft hover:text-bad"
              >
                <Trash2 className="h-3 w-3" />
              </button>
            </div>
          )}
        </div>

        {!isCollapsed && hasChildren && (
          <div className="mt-0.5 space-y-0.5">{node.children.map(renderNode)}</div>
        )}
      </div>
    )
  }

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center justify-between gap-2">
        <span className="text-xs font-semibold text-ink">{t('workspace.groups.title')}</span>
        {editable && (
          <button
            type="button"
            onClick={() => openCreate(null)}
            className="inline-flex h-7 items-center gap-1 rounded-md border border-border bg-surface px-2 text-[11px] font-medium text-ink hover:border-accent hover:text-accent"
          >
            <Plus className="h-3 w-3" />
            <span>{t('workspace.groups.createRoot')}</span>
          </button>
        )}
      </div>

      {/* All subjects root pill */}
      <button
        type="button"
        onClick={() => onSelectGroup(null)}
        className={`flex min-h-8 items-center justify-between rounded-lg px-2.5 py-1.5 text-left text-xs font-medium transition-colors ${
          selectedGroupId === null
            ? 'bg-accent-soft text-accent'
            : 'text-ink-muted hover:bg-subtle hover:text-ink'
        }`}
      >
        <span>{t('workspace.groups.allGroups')}</span>
        <span className="font-mono text-[10px]">{availableSubjects.length}</span>
      </button>

      {/* Tree items */}
      {tree.length === 0 ? (
        <p className="rounded-lg border border-dashed border-border px-2.5 py-3 text-[11px] leading-relaxed text-ink-muted">
          {t('workspace.groups.empty')}
        </p>
      ) : (
        <div className="space-y-0.5">{tree.map(renderNode)}</div>
      )}

      {/* Inline Create / Edit Form */}
      {editor && (
        <div className="mt-1 space-y-2.5 rounded-xl border border-accent/40 bg-surface p-3 shadow-xs">
          <div className="flex items-center justify-between">
            <span className="text-xs font-semibold text-ink">
              {editor.mode === 'create'
                ? editor.parentId
                  ? t('workspace.groups.createChild')
                  : t('workspace.groups.createRoot')
                : t('workspace.groups.edit')}
            </span>
            <button
              type="button"
              onClick={() => setEditor(null)}
              className="flex h-6 w-6 items-center justify-center rounded text-ink-muted hover:bg-subtle"
            >
              <X className="h-3.5 w-3.5" />
            </button>
          </div>

          <label className="block">
            <span className="text-[11px] font-medium text-ink-muted">
              {t('workspace.groups.nameLabel')}
            </span>
            <input
              type="text"
              value={editor.name}
              onChange={(e) => setEditor({ ...editor, name: e.target.value })}
              placeholder={t('workspace.groups.namePlaceholder')}
              className="mt-1 h-9 w-full rounded-lg border border-border bg-canvas px-2.5 text-xs text-ink"
            />
          </label>

          <label className="block">
            <span className="text-[11px] font-medium text-ink-muted">
              {t('workspace.groups.parentLabel')}
            </span>
            <select
              value={editor.parentId ?? ''}
              onChange={(e) =>
                setEditor({ ...editor, parentId: e.target.value || null })
              }
              className="mt-1 h-9 w-full rounded-lg border border-border bg-canvas px-2 text-xs text-ink"
            >
              <option value="">{t('workspace.groups.parentNone')}</option>
              {flatNodes
                .filter((n) =>
                  editor.groupId
                    ? !isDescendantOrSelf(groups, n.group.id, editor.groupId)
                    : true,
                )
                .map((n) => (
                  <option key={n.group.id} value={n.group.id}>
                    {'— '.repeat(n.depth)}
                    {n.group.name}
                  </option>
                ))}
            </select>
          </label>

          <div>
            <span className="text-[11px] font-medium text-ink-muted">
              {t('workspace.groups.subjectsLabel')}
            </span>
            {availableSubjects.length === 0 ? (
              <p className="mt-1 text-[11px] text-ink-muted">
                {t('workspace.groups.noSubjectsAvailable')}
              </p>
            ) : (
              <div className="mt-1.5 flex flex-wrap gap-1.5">
                {availableSubjects.map((subject) => {
                  const active = editor.subjects.includes(subject)
                  return (
                    <button
                      key={subject}
                      type="button"
                      onClick={() => toggleEditorSubject(subject)}
                      className={`rounded-full border px-2.5 py-1 text-[11px] font-medium transition-colors ${
                        active
                          ? 'border-accent bg-accent-soft text-accent'
                          : 'border-border bg-canvas text-ink-muted hover:text-ink'
                      }`}
                    >
                      {subject}
                    </button>
                  )
                })}
              </div>
            )}
          </div>

          {formError && <p className="text-[11px] text-bad">{formError}</p>}

          <div className="flex items-center justify-end gap-1.5 pt-1">
            <button
              type="button"
              onClick={() => setEditor(null)}
              className="h-8 rounded-lg border border-border px-2.5 text-xs text-ink-muted hover:bg-subtle"
            >
              {t('common.cancel')}
            </button>
            <button
              type="button"
              onClick={() => void handleSaveEditor()}
              disabled={createGroup.isPending || updateGroup.isPending}
              className="h-8 rounded-lg bg-accent px-3 text-xs font-medium text-white hover:bg-accent-hover disabled:opacity-50"
            >
              {t('workspace.groups.save')}
            </button>
          </div>
        </div>
      )}
    </div>
  )
}
