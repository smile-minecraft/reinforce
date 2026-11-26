import DOMPurify from 'dompurify'
import { marked } from 'marked'
import { useMemo } from 'react'

/**
 * Transforms Obsidian/GitHub-style callout blockquotes (`> [!tip] Title`)
 * into collapsible HTML `<details class="rf-callout">` elements before GFM parsing.
 */
function preprocessCallouts(src: string): string {
  const lines = src.replace(/\r\n/g, '\n').split('\n')
  const out: string[] = []
  let i = 0

  while (i < lines.length) {
    const headerMatch = lines[i].match(/^>\s*\[!([a-zA-Z0-9_-]+)\]([+-])?(?:\s+(.*))?$/)
    if (!headerMatch) {
      out.push(lines[i])
      i += 1
      continue
    }

    const kind = headerMatch[1].toLowerCase()
    const foldSign = headerMatch[2]
    const rawTitle = (headerMatch[3] ?? '').trim()
    const title = rawTitle || kind.charAt(0).toUpperCase() + kind.slice(1)
    const isOpen = foldSign !== '-'

    const bodyLines: string[] = []
    i += 1
    while (i < lines.length && /^>\s?/.test(lines[i])) {
      bodyLines.push(lines[i].replace(/^>\s?/, ''))
      i += 1
    }

    const bodyHtml = marked.parse(bodyLines.join('\n'), { gfm: true, breaks: false }) as string
    const openAttr = isOpen ? ' open' : ''
    out.push(
      `<details class="rf-callout" data-callout="${kind}"${openAttr}><summary>${escapeHtml(title)}</summary><div class="rf-callout-body">${bodyHtml}</div></details>`,
    )
  }

  return out.join('\n')
}

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
}

function wrapTables(html: string): string {
  return html.replace(/<table([\s\S]*?)<\/table>/g, '<div class="rf-table-wrap"><table$1</table></div>')
}

export function MarkdownBlock({ text, className = '' }: { text: string; className?: string }) {
  const safeHtml = useMemo(() => {
    const preprocessed = preprocessCallouts(text)
    const rawHtml = marked.parse(preprocessed, { gfm: true, breaks: false }) as string
    const wrapped = wrapTables(rawHtml)
    return DOMPurify.sanitize(wrapped, {
      ADD_TAGS: ['details', 'summary'],
      ADD_ATTR: ['open', 'data-callout', 'class'],
    })
  }, [text])

  return (
    <div
      className={`rf-prose max-w-none break-words ${className}`}
      dangerouslySetInnerHTML={{ __html: safeHtml }}
    />
  )
}
