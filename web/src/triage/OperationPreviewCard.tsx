import {
  ArrowRight,
  BookOpen,
  CheckCircle2,
  FileText,
  GitBranch,
  HelpCircle,
  Layers,
  Pencil,
  ShieldCheck,
} from 'lucide-react'
import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import type {
  BatchOperation,
  ContentBlock,
  OutlineNode,
  ValidationErrorItem,
} from '../api/types'
import { BlockRenderer } from '../blocks/BlockRenderer'
import { OptionLinkedCards } from '../study/OptionLinkedCards'
import { getFieldValidationErrors } from './proposalHelpers'
import { FieldErrorList } from './ValidationBanner'

function OutlinePreview({ nodes, depth = 0 }: { nodes: OutlineNode[]; depth?: number }) {
  return (
    <ul className={depth === 0 ? 'space-y-2' : 'mt-1.5 space-y-1.5 border-l border-border pl-3'}>
      {nodes.map((node, idx) => {
        if (node.type === 'section') {
          return (
            <li key={`sec-${depth}-${idx}`} className="rounded-lg bg-subtle/50 p-2.5">
              <div className="text-sm font-semibold text-ink">{node.heading}</div>
              {node.intro_markdown && (
                <p className="mt-1 text-xs text-ink-muted">{node.intro_markdown}</p>
              )}
              {node.children.length > 0 && (
                <OutlinePreview nodes={node.children} depth={depth + 1} />
              )}
            </li>
          )
        }
        return (
          <li
            key={`ref-${node.card_id}-${idx}`}
            className="flex items-center gap-2 rounded-md border border-border bg-surface px-2.5 py-1.5 text-xs text-ink-muted"
          >
            <Layers className="h-3.5 w-3.5 text-accent" />
            <span className="font-mono">{node.card_id}</span>
          </li>
        )
      })}
    </ul>
  )
}

function BlocksPreview({
  blocks,
  renderInteractivePins,
}: {
  blocks: ContentBlock[]
  renderInteractivePins?: (
    block: Extract<ContentBlock, { type: 'interactive_pins' }>,
    blockIndex: number,
  ) => ReactNode
}) {
  return (
    <div className="space-y-2">
      {blocks.map((block, blockIndex) => {
        if (block.type === 'interactive_pins' && renderInteractivePins) {
          return (
            <div key={`pin-block-${blockIndex}`}>
              {renderInteractivePins(block, blockIndex)}
            </div>
          )
        }
        return <BlockRenderer key={`block-${blockIndex}`} block={block} />
      })}
    </div>
  )
}

export function OperationPreviewCard({
  op,
  index,
  opErrors,
  canEdit,
  isEditing,
  onToggleEdit,
  onSelectAnchor,
  renderInteractivePins,
  editorNode,
}: {
  op: BatchOperation
  index: number
  opErrors: ValidationErrorItem[]
  canEdit: boolean
  isEditing: boolean
  onToggleEdit?: () => void
  onSelectAnchor: (anchorId: string) => void
  renderInteractivePins?: (
    block: Extract<ContentBlock, { type: 'interactive_pins' }>,
    blockIndex: number,
  ) => ReactNode
  editorNode?: ReactNode
}) {
  const { t } = useTranslation()
  const hasErrors = opErrors.length > 0

  const isEditableKind =
    op.op === 'knowledge_card' ||
    op.op === 'question_card' ||
    op.op === 'note' ||
    op.op === 'exam_set' ||
    op.op === 'question_group'

  // Collect unhandled errors (those not explicitly matched to a known field section)
  const genericErrors = opErrors.filter((err) => {
    const f = err.field_path.replace(/^data\./, '')
    return !['title', 'blocks', 'stem_blocks', 'explanation_blocks', 'options', 'answer', 'answer_basis', 'answer_source_kind', 'anchor_ids', 'outline'].some(
      (known) => f === known || f.startsWith(`${known}.`) || f.startsWith(`${known}[`),
    )
  })

  return (
    <article
      id={`triage-op-${index}`}
      className={`rounded-xl border bg-surface transition-colors ${
        hasErrors ? 'border-bad ring-1 ring-bad/25' : 'border-border'
      }`}
    >
      {/* Operation header */}
      <header className="flex flex-wrap items-center justify-between gap-2 border-b border-border bg-subtle/40 px-4 py-2.5">
        <div className="flex flex-wrap items-center gap-2">
          <span className="rounded bg-accent-soft px-2 py-0.5 font-mono text-xs font-semibold text-accent">
            #{index + 1}
          </span>
          <span className="text-xs font-semibold text-ink">{t(`triage.ops.${op.op}`)}</span>
          {op.temp_ref && (
            <code className="rounded bg-subtle px-1.5 py-0.5 font-mono text-[11px] text-ink-muted">
              {op.temp_ref}
            </code>
          )}
        </div>

        <div className="flex items-center gap-1.5">
          {canEdit && isEditableKind && onToggleEdit && (
            <button
              type="button"
              onClick={onToggleEdit}
              className={`inline-flex min-h-9 items-center gap-1.5 rounded-lg border px-2.5 text-xs font-medium transition-colors ${
                isEditing
                  ? 'border-accent bg-accent-soft text-accent'
                  : 'border-border bg-surface text-ink-muted hover:bg-subtle hover:text-ink'
              }`}
            >
              <Pencil className="h-3.5 w-3.5" />
              <span>{isEditing ? t('triage.discardChanges') : t('triage.editText')}</span>
            </button>
          )}
        </div>
      </header>

      <div className="space-y-4 p-4">
        <FieldErrorList errors={genericErrors} />

        {isEditing && editorNode ? (
          editorNode
        ) : (
          <>
            {op.op === 'knowledge_card' && (
              <div className="space-y-3">
                <div className="flex flex-wrap items-center gap-2 text-xs">
                  <span className="rounded-md bg-subtle px-2 py-0.5 font-medium text-ink">
                    {op.data.subject}
                  </span>
                  {op.data.topics.map((topic) => (
                    <span
                      key={topic}
                      className="rounded-md border border-border px-2 py-0.5 text-ink-muted"
                    >
                      {topic}
                    </span>
                  ))}
                  <span
                    className={`ml-auto inline-flex items-center gap-1 rounded-md px-2 py-0.5 font-medium ${
                      op.data.verification === 'verified'
                        ? 'bg-ok-soft text-ok'
                        : 'bg-warn-soft text-warn'
                    }`}
                  >
                    <ShieldCheck className="h-3.5 w-3.5" />
                    {op.data.verification === 'verified'
                      ? t('common.verified')
                      : t('common.unchecked')}
                  </span>
                </div>

                <div>
                  <h3 className="text-lg font-semibold text-ink">{op.data.title}</h3>
                  <FieldErrorList errors={getFieldValidationErrors(opErrors, ['title'])} />
                </div>

                <div>
                  <BlocksPreview
                    blocks={op.data.blocks}
                    renderInteractivePins={renderInteractivePins}
                  />
                  <FieldErrorList errors={getFieldValidationErrors(opErrors, ['blocks'])} />
                </div>

                {op.data.anchor_ids.length > 0 && (
                  <div className="flex flex-wrap items-center gap-2 border-t border-border pt-3">
                    {op.data.anchor_ids.map((aid, idx) => (
                      <button
                        key={aid}
                        type="button"
                        onClick={() => onSelectAnchor(aid)}
                        className="inline-flex min-h-9 items-center gap-1.5 rounded-lg border border-border bg-canvas px-3 text-xs font-medium text-accent hover:bg-accent-soft"
                      >
                        <FileText className="h-3.5 w-3.5" />
                        <span>
                          {t('triage.viewInSource')} #{idx + 1}
                        </span>
                      </button>
                    ))}
                  </div>
                )}
                <FieldErrorList errors={getFieldValidationErrors(opErrors, ['anchor_ids'])} />
              </div>
            )}

            {op.op === 'question_card' && (
              <div className="space-y-4">
                <div className="flex flex-wrap items-center gap-2 text-xs">
                  <span className="rounded-md bg-subtle px-2 py-0.5 font-medium text-ink">
                    {op.data.subject}
                  </span>
                  {op.data.question_no !== null && (
                    <span className="rounded-md bg-accent-soft px-2 py-0.5 font-mono font-medium text-accent">
                      Q{op.data.question_no}
                    </span>
                  )}
                  <span className="rounded-md border border-border px-2 py-0.5 text-ink-muted">
                    {t(`study.runner.questionTypes.${op.data.question_type}`)}
                  </span>
                  <span className="rounded-md border border-border px-2 py-0.5 text-ink-muted">
                    {t(`triage.gradingMode.${op.data.grading_mode}`)}
                  </span>
                  <span
                    className={`ml-auto inline-flex items-center gap-1 rounded-md px-2 py-0.5 font-medium ${
                      op.data.verification === 'verified'
                        ? 'bg-ok-soft text-ok'
                        : 'bg-warn-soft text-warn'
                    }`}
                  >
                    <ShieldCheck className="h-3.5 w-3.5" />
                    {op.data.verification === 'verified'
                      ? t('common.verified')
                      : t('common.unchecked')}
                  </span>
                </div>

                {/* Stem */}
                <div>
                  <BlocksPreview
                    blocks={op.data.stem_blocks}
                    renderInteractivePins={renderInteractivePins}
                  />
                  <FieldErrorList errors={getFieldValidationErrors(opErrors, ['stem_blocks'])} />
                </div>

                {/* Options with linked_card_ids */}
                {op.data.options.length > 0 && (
                  <div className="space-y-2">
                    <h4 className="text-xs font-semibold uppercase tracking-wide text-ink-muted">
                      {t('triage.optionsHeading')}
                    </h4>
                    <div className="space-y-2">
                      {op.data.options.map((opt) => {
                        const isAnswer = (op.data.answer ?? []).includes(opt.key)
                        return (
                          <div
                            key={opt.key}
                            className={`rounded-lg border p-3 ${
                              isAnswer
                                ? 'border-ok/50 bg-ok-soft/60'
                                : 'border-border bg-canvas/60'
                            }`}
                          >
                            <div className="flex items-start gap-2.5">
                              <span
                                className={`inline-flex h-6 w-6 shrink-0 items-center justify-center rounded-md font-mono text-xs font-bold ${
                                  isAnswer
                                    ? 'bg-ok text-white'
                                    : 'border border-border bg-surface text-ink'
                                }`}
                              >
                                {opt.key}
                              </span>
                              <div className="min-w-0 flex-1">
                                <div className="text-sm font-medium text-ink">{opt.content}</div>
                                {opt.explanation && (
                                  <p className="mt-1 text-xs text-ink-muted">{opt.explanation}</p>
                                )}
                                {opt.linked_card_ids.length > 0 && (
                                  <OptionLinkedCards
                                    cardIds={opt.linked_card_ids}
                                    onOpenAnchor={onSelectAnchor}
                                  />
                                )}
                              </div>
                              {isAnswer && (
                                <CheckCircle2 className="h-4 w-4 shrink-0 text-ok" />
                              )}
                            </div>
                          </div>
                        )
                      })}
                    </div>
                    <FieldErrorList
                      errors={getFieldValidationErrors(opErrors, ['options', 'answer'])}
                    />
                  </div>
                )}

                {/* Answer provenance */}
                <div className="rounded-lg border border-border bg-subtle/40 p-3.5 space-y-2">
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <span className="inline-flex items-center gap-1.5 text-xs font-semibold text-ink">
                      <HelpCircle className="h-3.5 w-3.5 text-accent" />
                      {t('triage.answerProvenance')}
                    </span>
                    <span className="rounded-md bg-accent-soft px-2 py-0.5 text-xs font-medium text-accent">
                      {t(`triage.answerSource.${op.data.answer_source_kind}`)}
                    </span>
                  </div>

                  {op.data.answer && op.data.answer.length > 0 && (
                    <div className="text-xs text-ink">
                      <span className="font-medium text-ink-muted">{t('triage.answerKeys')}: </span>
                      <span className="font-mono font-semibold">{op.data.answer.join(', ')}</span>
                    </div>
                  )}

                  {op.data.accepted_answers && op.data.accepted_answers.length > 0 && (
                    <div className="text-xs text-ink">
                      <span className="font-medium text-ink-muted">
                        {t('triage.acceptedAnswers')}:{' '}
                      </span>
                      <span className="font-mono">
                        {op.data.accepted_answers.map((grp) => grp.join('+')).join(' / ')}
                      </span>
                    </div>
                  )}

                  <div className="text-xs">
                    <span className="font-medium text-ink-muted">{t('triage.answerBasis')}: </span>
                    <span className={op.data.answer_basis?.trim() ? 'text-ink' : 'italic text-warn'}>
                      {op.data.answer_basis?.trim() || t('triage.noAnswerBasis')}
                    </span>
                  </div>

                  <FieldErrorList
                    errors={getFieldValidationErrors(opErrors, [
                      'answer_source_kind',
                      'answer_basis',
                    ])}
                  />
                </div>

                {/* Explanation */}
                {op.data.explanation_blocks && op.data.explanation_blocks.length > 0 && (
                  <div>
                    <h4 className="mb-1.5 text-xs font-semibold uppercase tracking-wide text-ink-muted">
                      {t('triage.explanationHeading')}
                    </h4>
                    <BlocksPreview
                      blocks={op.data.explanation_blocks}
                      renderInteractivePins={renderInteractivePins}
                    />
                    <FieldErrorList
                      errors={getFieldValidationErrors(opErrors, ['explanation_blocks'])}
                    />
                  </div>
                )}

                {op.data.anchor_ids.length > 0 && (
                  <div className="flex flex-wrap items-center gap-2 border-t border-border pt-3">
                    {op.data.anchor_ids.map((aid, idx) => (
                      <button
                        key={aid}
                        type="button"
                        onClick={() => onSelectAnchor(aid)}
                        className="inline-flex min-h-9 items-center gap-1.5 rounded-lg border border-border bg-canvas px-3 text-xs font-medium text-accent hover:bg-accent-soft"
                      >
                        <FileText className="h-3.5 w-3.5" />
                        <span>
                          {t('triage.viewInSource')} #{idx + 1}
                        </span>
                      </button>
                    ))}
                  </div>
                )}
                <FieldErrorList errors={getFieldValidationErrors(opErrors, ['anchor_ids'])} />
              </div>
            )}

            {op.op === 'note' && (
              <div className="space-y-3">
                <div className="flex items-center gap-2 text-xs">
                  <BookOpen className="h-4 w-4 text-accent" />
                  <span className="rounded bg-subtle px-2 py-0.5 font-medium text-ink">
                    {op.data.subject}
                  </span>
                </div>
                <h3 className="text-lg font-semibold text-ink">{op.data.title}</h3>
                <FieldErrorList errors={getFieldValidationErrors(opErrors, ['title'])} />
                <div>
                  <h4 className="mb-2 text-xs font-semibold uppercase tracking-wide text-ink-muted">
                    {t('triage.outlineHeading')}
                  </h4>
                  <OutlinePreview nodes={op.data.outline} />
                  <FieldErrorList errors={getFieldValidationErrors(opErrors, ['outline'])} />
                </div>
              </div>
            )}

            {op.op === 'exam_set' && (
              <div className="space-y-2 text-sm">
                <div className="flex flex-wrap items-center gap-2 text-xs">
                  <span className="rounded bg-subtle px-2 py-0.5 font-medium text-ink">
                    {op.data.subject}
                  </span>
                  <span className="rounded border border-border px-2 py-0.5 text-ink-muted">
                    {op.data.source_kind}
                  </span>
                  {op.data.exam_year !== null && (
                    <span className="rounded border border-border px-2 py-0.5 font-mono text-ink-muted">
                      {op.data.exam_year}
                    </span>
                  )}
                  {op.data.expected_questions !== null && (
                    <span className="rounded border border-border px-2 py-0.5 font-mono text-ink-muted">
                      {op.data.expected_questions} Q
                    </span>
                  )}
                </div>
                <h3 className="text-base font-semibold text-ink">{op.data.title}</h3>
                <FieldErrorList errors={getFieldValidationErrors(opErrors, ['title'])} />
              </div>
            )}

            {op.op === 'question_group' && (
              <div className="space-y-3">
                {op.data.title && (
                  <h3 className="text-base font-semibold text-ink">{op.data.title}</h3>
                )}
                <BlocksPreview
                  blocks={op.data.shared_stem_blocks}
                  renderInteractivePins={renderInteractivePins}
                />
              </div>
            )}

            {op.op === 'edge' && (
              <div className="flex flex-wrap items-center gap-2 text-xs">
                <GitBranch className="h-4 w-4 text-accent" />
                <span className="rounded bg-accent-soft px-2 py-0.5 font-mono font-medium text-accent">
                  {op.data.edge_type}
                </span>
                <span className="font-mono text-ink">
                  {op.data.from.kind}:{op.data.from.id.slice(0, 8)}
                </span>
                <ArrowRight className="h-3.5 w-3.5 text-ink-muted" />
                <span className="font-mono text-ink">
                  {op.data.to.kind}:{op.data.to.id.slice(0, 8)}
                </span>
              </div>
            )}

            {op.op === 'resource' && (
              <div className="flex flex-wrap items-center gap-2 text-xs text-ink-muted">
                <FileText className="h-4 w-4 text-accent" />
                <span className="font-medium text-ink">{op.data.title}</span>
                <span className="rounded bg-subtle px-2 py-0.5">{op.data.kind}</span>
                <span className="rounded bg-subtle px-2 py-0.5">{op.data.export_policy}</span>
              </div>
            )}

            {op.op === 'anchor' && (
              <div className="flex flex-wrap items-center justify-between gap-2 text-xs">
                <div className="flex flex-wrap items-center gap-2 text-ink-muted">
                  <span className="rounded bg-subtle px-2 py-0.5 font-mono text-ink">
                    {op.data.anchor_kind}
                  </span>
                  {op.data.pdf_page !== null && (
                    <span>{t('anchor.pdfPage', { page: op.data.pdf_page })}</span>
                  )}
                  {op.data.printed_page !== null && (
                    <span>{t('anchor.printedPage', { page: op.data.printed_page })}</span>
                  )}
                </div>
                <button
                  type="button"
                  onClick={() => onSelectAnchor(op.data.id)}
                  className="inline-flex min-h-9 items-center gap-1.5 rounded-lg border border-border bg-canvas px-3 text-xs font-medium text-accent hover:bg-accent-soft"
                >
                  <FileText className="h-3.5 w-3.5" />
                  <span>{t('triage.viewInSource')}</span>
                </button>
              </div>
            )}
          </>
        )}
      </div>
    </article>
  )
}
