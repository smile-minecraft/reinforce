import { AlertTriangle, Check, Loader2 } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ApiError, extractValidationReport } from '../api/client'
import { useUpdateProposalOperation } from '../api/hooks/useProposals'
import type {
  BatchOperation,
  ContentBlock,
  ExamSet,
  KnowledgeCard,
  Note,
  OutlineNode,
  QuestionCard,
  QuestionGroup,
  ValidationErrorItem,
  ValidationReport,
} from '../api/types'
import { getFieldValidationErrors } from './proposalHelpers'
import { FieldErrorList } from './ValidationBanner'

function updateMarkdownBlock(blocks: ContentBlock[], blockIdx: number, text: string): ContentBlock[] {
  return blocks.map((b, idx) => (idx === blockIdx && b.type === 'markdown' ? { ...b, text } : b))
}

function updateOutlineSection(
  nodes: OutlineNode[],
  path: number[],
  updater: (node: Extract<OutlineNode, { type: 'section' }>) => Extract<OutlineNode, { type: 'section' }>,
): OutlineNode[] {
  const [head, ...rest] = path
  return nodes.map((node, idx) => {
    if (idx !== head || node.type !== 'section') return node
    if (rest.length === 0) return updater(node)
    return { ...node, children: updateOutlineSection(node.children, rest, updater) }
  })
}

function flattenSectionPaths(
  nodes: OutlineNode[],
  prefix: number[] = [],
): Array<{ path: number[]; node: Extract<OutlineNode, { type: 'section' }> }> {
  const list: Array<{ path: number[]; node: Extract<OutlineNode, { type: 'section' }> }> = []
  nodes.forEach((node, idx) => {
    if (node.type === 'section') {
      const path = [...prefix, idx]
      list.push({ path, node })
      list.push(...flattenSectionPaths(node.children, path))
    }
  })
  return list
}

export function OperationTextEditor({
  proposalId,
  op,
  opIndex,
  opErrors,
  onSaved,
  onCancel,
  onValidationError,
}: {
  proposalId: string
  op: BatchOperation
  opIndex: number
  opErrors: ValidationErrorItem[]
  onSaved: () => void
  onCancel: () => void
  onValidationError: (report: ValidationReport) => void
}) {
  const { t } = useTranslation()
  const updateOp = useUpdateProposalOperation()
  const [draft, setDraft] = useState<BatchOperation['data']>(() => structuredClone(op.data))
  const [errorMessage, setErrorMessage] = useState<string | null>(null)

  async function handleSave() {
    setErrorMessage(null)
    try {
      await updateOp.mutateAsync({
        id: proposalId,
        index: opIndex,
        body: { data: draft },
      })
      onSaved()
    } catch (err) {
      const report = extractValidationReport(err)
      if (report) {
        onValidationError(report)
        return
      }
      if (err instanceof ApiError && err.status === 404) {
        setErrorMessage(t('triage.serverUpdateRequired'))
        return
      }
      setErrorMessage(err instanceof Error ? err.message : t('common.errorGeneric'))
    }
  }

  return (
    <div data-no-swipe="true" className="space-y-4">
      {errorMessage && (
        <div
          role="alert"
          className="flex items-start gap-2 rounded-lg border border-warn/40 bg-warn-soft px-3 py-2.5 text-xs text-warn"
        >
          <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
          <span>{errorMessage}</span>
        </div>
      )}

      {op.op === 'knowledge_card' && (() => {
        const card = draft as KnowledgeCard
        return (
          <div className="space-y-3">
            <div>
              <label className="block text-xs font-medium text-ink">{t('triage.fieldTitle')}</label>
              <input
                type="text"
                value={card.title}
                onChange={(e) => setDraft({ ...card, title: e.target.value })}
                className="mt-1 min-h-11 w-full rounded-lg border border-border bg-canvas px-3 text-sm text-ink"
              />
              <FieldErrorList errors={getFieldValidationErrors(opErrors, ['title'])} />
            </div>

            <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
              <div>
                <label className="block text-xs font-medium text-ink">
                  {t('triage.fieldSubject')}
                </label>
                <input
                  type="text"
                  value={card.subject}
                  onChange={(e) => setDraft({ ...card, subject: e.target.value })}
                  className="mt-1 min-h-11 w-full rounded-lg border border-border bg-canvas px-3 text-sm text-ink"
                />
              </div>
              <div>
                <label className="block text-xs font-medium text-ink">
                  {t('triage.fieldTopics')}
                </label>
                <input
                  type="text"
                  value={card.topics.join(', ')}
                  onChange={(e) =>
                    setDraft({
                      ...card,
                      topics: e.target.value
                        .split(',')
                        .map((s) => s.trim())
                        .filter(Boolean),
                    })
                  }
                  className="mt-1 min-h-11 w-full rounded-lg border border-border bg-canvas px-3 text-sm text-ink"
                />
              </div>
            </div>

            {card.blocks.map((block, bIdx) => {
              if (block.type === 'markdown') {
                return (
                  <div key={`md-${bIdx}`}>
                    <label className="block text-xs font-medium text-ink">
                      {t('triage.fieldMarkdown', { index: bIdx + 1 })}
                    </label>
                    <textarea
                      rows={6}
                      value={block.text}
                      onChange={(e) =>
                        setDraft({
                          ...card,
                          blocks: updateMarkdownBlock(card.blocks, bIdx, e.target.value),
                        })
                      }
                      className="mt-1 w-full rounded-lg border border-border bg-canvas p-3 font-mono text-xs leading-relaxed text-ink"
                    />
                  </div>
                )
              }
              if (block.type === 'interactive_pins') {
                return (
                  <div key={`pins-${bIdx}`} className="space-y-2 rounded-lg border border-border bg-subtle/40 p-3">
                    {block.pins.map((pin, pIdx) => (
                      <div key={pin.no} className="grid grid-cols-1 gap-2 border-b border-border/60 pb-2.5 last:border-none last:pb-0 sm:grid-cols-2">
                        <div>
                          <label className="block text-xs font-medium text-ink">
                            {t('triage.fieldPinTerm', { no: pin.no })}
                          </label>
                          <input
                            type="text"
                            value={pin.term_primary}
                            onChange={(e) => {
                              const nextBlocks = card.blocks.map((b, i) => {
                                if (i !== bIdx || b.type !== 'interactive_pins') return b
                                return {
                                  ...b,
                                  pins: b.pins.map((p, pi) =>
                                    pi === pIdx ? { ...p, term_primary: e.target.value } : p,
                                  ),
                                }
                              })
                              setDraft({ ...card, blocks: nextBlocks })
                            }}
                            className="mt-1 min-h-10 w-full rounded-lg border border-border bg-canvas px-2.5 text-xs text-ink"
                          />
                        </div>
                        <div>
                          <label className="block text-xs font-medium text-ink">
                            {t('triage.fieldPinSecondary')}
                          </label>
                          <input
                            type="text"
                            value={pin.term_secondary ?? ''}
                            onChange={(e) => {
                              const nextBlocks = card.blocks.map((b, i) => {
                                if (i !== bIdx || b.type !== 'interactive_pins') return b
                                return {
                                  ...b,
                                  pins: b.pins.map((p, pi) =>
                                    pi === pIdx
                                      ? { ...p, term_secondary: e.target.value || null }
                                      : p,
                                  ),
                                }
                              })
                              setDraft({ ...card, blocks: nextBlocks })
                            }}
                            className="mt-1 min-h-10 w-full rounded-lg border border-border bg-canvas px-2.5 text-xs text-ink"
                          />
                        </div>
                        <div className="sm:col-span-2">
                          <label className="block text-xs font-medium text-ink">
                            {t('triage.fieldPinDetail')}
                          </label>
                          <input
                            type="text"
                            value={pin.detail ?? ''}
                            onChange={(e) => {
                              const nextBlocks = card.blocks.map((b, i) => {
                                if (i !== bIdx || b.type !== 'interactive_pins') return b
                                return {
                                  ...b,
                                  pins: b.pins.map((p, pi) =>
                                    pi === pIdx ? { ...p, detail: e.target.value || null } : p,
                                  ),
                                }
                              })
                              setDraft({ ...card, blocks: nextBlocks })
                            }}
                            className="mt-1 min-h-10 w-full rounded-lg border border-border bg-canvas px-2.5 text-xs text-ink"
                          />
                        </div>
                      </div>
                    ))}
                  </div>
                )
              }
              return null
            })}
            <FieldErrorList errors={getFieldValidationErrors(opErrors, ['blocks'])} />
          </div>
        )
      })()}

      {op.op === 'question_card' && (() => {
        const q = draft as QuestionCard
        const explBlocks = q.explanation_blocks ?? []
        return (
          <div className="space-y-4">
            {/* Stem markdown blocks */}
            {q.stem_blocks.map((block, bIdx) =>
              block.type === 'markdown' ? (
                <div key={`stem-${bIdx}`}>
                  <label className="block text-xs font-medium text-ink">
                    {t('triage.fieldStemMarkdown', { index: bIdx + 1 })}
                  </label>
                  <textarea
                    rows={4}
                    value={block.text}
                    onChange={(e) =>
                      setDraft({
                        ...q,
                        stem_blocks: updateMarkdownBlock(q.stem_blocks, bIdx, e.target.value),
                      })
                    }
                    className="mt-1 w-full rounded-lg border border-border bg-canvas p-3 text-sm text-ink"
                  />
                </div>
              ) : null,
            )}
            <FieldErrorList errors={getFieldValidationErrors(opErrors, ['stem_blocks'])} />

            {/* Options */}
            {q.options.length > 0 && (
              <div className="space-y-3">
                {q.options.map((opt, optIdx) => (
                  <div
                    key={opt.key}
                    className="space-y-2 rounded-lg border border-border bg-subtle/40 p-3"
                  >
                    <div>
                      <label className="block text-xs font-medium text-ink">
                        {t('triage.fieldOptionContent', { key: opt.key })}
                      </label>
                      <input
                        type="text"
                        value={opt.content}
                        onChange={(e) =>
                          setDraft({
                            ...q,
                            options: q.options.map((o, i) =>
                              i === optIdx ? { ...o, content: e.target.value } : o,
                            ),
                          })
                        }
                        className="mt-1 min-h-10 w-full rounded-lg border border-border bg-canvas px-3 text-sm text-ink"
                      />
                    </div>
                    <div>
                      <label className="block text-xs font-medium text-ink">
                        {t('triage.fieldOptionExplanation', { key: opt.key })}
                      </label>
                      <textarea
                        rows={2}
                        value={opt.explanation ?? ''}
                        onChange={(e) =>
                          setDraft({
                            ...q,
                            options: q.options.map((o, i) =>
                              i === optIdx
                                ? { ...o, explanation: e.target.value || null }
                                : o,
                            ),
                          })
                        }
                        className="mt-1 w-full rounded-lg border border-border bg-canvas p-2.5 text-xs text-ink"
                      />
                    </div>
                  </div>
                ))}
                <FieldErrorList errors={getFieldValidationErrors(opErrors, ['options'])} />
              </div>
            )}

            {/* Answer basis */}
            <div>
              <label className="block text-xs font-medium text-ink">
                {t('triage.fieldAnswerBasis')}
              </label>
              <textarea
                rows={3}
                value={q.answer_basis ?? ''}
                onChange={(e) =>
                  setDraft({
                    ...q,
                    answer_basis: e.target.value,
                  })
                }
                className="mt-1 w-full rounded-lg border border-border bg-canvas p-3 text-sm text-ink"
              />
              <FieldErrorList
                errors={getFieldValidationErrors(opErrors, ['answer_basis', 'answer_source_kind'])}
              />
            </div>

            {/* Explanation markdown blocks */}
            {explBlocks.map((block, bIdx) =>
              block.type === 'markdown' ? (
                <div key={`expl-${bIdx}`}>
                  <label className="block text-xs font-medium text-ink">
                    {t('triage.fieldExplanationMarkdown', { index: bIdx + 1 })}
                  </label>
                  <textarea
                    rows={3}
                    value={block.text}
                    onChange={(e) =>
                      setDraft({
                        ...q,
                        explanation_blocks: updateMarkdownBlock(explBlocks, bIdx, e.target.value),
                      })
                    }
                    className="mt-1 w-full rounded-lg border border-border bg-canvas p-3 text-sm text-ink"
                  />
                </div>
              ) : null,
            )}
            <FieldErrorList errors={getFieldValidationErrors(opErrors, ['explanation_blocks'])} />
          </div>
        )
      })()}

      {op.op === 'note' && (() => {
        const note = draft as Note
        const sections = flattenSectionPaths(note.outline)
        return (
          <div className="space-y-3">
            <div>
              <label className="block text-xs font-medium text-ink">{t('triage.fieldTitle')}</label>
              <input
                type="text"
                value={note.title}
                onChange={(e) => setDraft({ ...note, title: e.target.value })}
                className="mt-1 min-h-11 w-full rounded-lg border border-border bg-canvas px-3 text-sm text-ink"
              />
            </div>
            <div>
              <label className="block text-xs font-medium text-ink">{t('triage.fieldSubject')}</label>
              <input
                type="text"
                value={note.subject}
                onChange={(e) => setDraft({ ...note, subject: e.target.value })}
                className="mt-1 min-h-11 w-full rounded-lg border border-border bg-canvas px-3 text-sm text-ink"
              />
            </div>
            {sections.map(({ path, node }) => (
              <div
                key={path.join('.')}
                className="space-y-2 rounded-lg border border-border bg-subtle/40 p-3"
              >
                <div>
                  <label className="block text-xs font-medium text-ink">
                    {t('triage.fieldSectionHeading')}
                  </label>
                  <input
                    type="text"
                    value={node.heading}
                    onChange={(e) =>
                      setDraft({
                        ...note,
                        outline: updateOutlineSection(note.outline, path, (sec) => ({
                          ...sec,
                          heading: e.target.value,
                        })),
                      })
                    }
                    className="mt-1 min-h-10 w-full rounded-lg border border-border bg-canvas px-3 text-sm text-ink"
                  />
                </div>
                <div>
                  <label className="block text-xs font-medium text-ink">
                    {t('triage.fieldSectionIntro')}
                  </label>
                  <textarea
                    rows={2}
                    value={node.intro_markdown ?? ''}
                    onChange={(e) =>
                      setDraft({
                        ...note,
                        outline: updateOutlineSection(note.outline, path, (sec) => ({
                          ...sec,
                          intro_markdown: e.target.value || null,
                        })),
                      })
                    }
                    className="mt-1 w-full rounded-lg border border-border bg-canvas p-2.5 text-xs text-ink"
                  />
                </div>
              </div>
            ))}
          </div>
        )
      })()}

      {op.op === 'exam_set' && (() => {
        const examSet = draft as ExamSet
        return (
          <div className="space-y-3">
            <div>
              <label className="block text-xs font-medium text-ink">{t('triage.fieldTitle')}</label>
              <input
                type="text"
                value={examSet.title}
                onChange={(e) => setDraft({ ...examSet, title: e.target.value })}
                className="mt-1 min-h-11 w-full rounded-lg border border-border bg-canvas px-3 text-sm text-ink"
              />
            </div>
            <div>
              <label className="block text-xs font-medium text-ink">{t('triage.fieldSubject')}</label>
              <input
                type="text"
                value={examSet.subject}
                onChange={(e) => setDraft({ ...examSet, subject: e.target.value })}
                className="mt-1 min-h-11 w-full rounded-lg border border-border bg-canvas px-3 text-sm text-ink"
              />
            </div>
          </div>
        )
      })()}

      {op.op === 'question_group' && (() => {
        const group = draft as QuestionGroup
        return (
          <div className="space-y-3">
            <div>
              <label className="block text-xs font-medium text-ink">{t('triage.fieldTitle')}</label>
              <input
                type="text"
                value={group.title ?? ''}
                onChange={(e) => setDraft({ ...group, title: e.target.value || null })}
                className="mt-1 min-h-11 w-full rounded-lg border border-border bg-canvas px-3 text-sm text-ink"
              />
            </div>
            {group.shared_stem_blocks.map((block, bIdx) =>
              block.type === 'markdown' ? (
                <div key={`shared-${bIdx}`}>
                  <label className="block text-xs font-medium text-ink">
                    {t('triage.fieldStemMarkdown', { index: bIdx + 1 })}
                  </label>
                  <textarea
                    rows={4}
                    value={block.text}
                    onChange={(e) =>
                      setDraft({
                        ...group,
                        shared_stem_blocks: updateMarkdownBlock(
                          group.shared_stem_blocks,
                          bIdx,
                          e.target.value,
                        ),
                      })
                    }
                    className="mt-1 w-full rounded-lg border border-border bg-canvas p-3 text-sm text-ink"
                  />
                </div>
              ) : null,
            )}
          </div>
        )
      })()}

      <div className="flex flex-wrap justify-end gap-2 border-t border-border pt-3">
        <button
          type="button"
          disabled={updateOp.isPending}
          onClick={onCancel}
          className="min-h-11 rounded-lg border border-border bg-canvas px-4 text-sm font-medium text-ink hover:bg-subtle"
        >
          {t('triage.discardChanges')}
        </button>
        <button
          type="button"
          disabled={updateOp.isPending}
          onClick={() => void handleSave()}
          className="inline-flex min-h-11 items-center gap-2 rounded-lg bg-accent px-4 text-sm font-medium text-white hover:bg-accent-hover disabled:opacity-50"
        >
          {updateOp.isPending ? (
            <Loader2 className="h-4 w-4 animate-spin" />
          ) : (
            <Check className="h-4 w-4" />
          )}
          <span>{updateOp.isPending ? t('triage.saving') : t('triage.saveChanges')}</span>
        </button>
      </div>
    </div>
  )
}
