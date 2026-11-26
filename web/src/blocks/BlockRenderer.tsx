import type { ContentBlock } from '../api/types'
import { DiagramBlock } from './DiagramBlock'
import { ImageOcclusionBlock } from './ImageOcclusionBlock'
import { InteractivePinsBlock } from './InteractivePinsBlock'
import { MarkdownBlock } from './MarkdownBlock'
import { MathBlock } from './MathBlock'

export function BlockRenderer({
  block,
  hidePinTerms = false,
}: {
  block: ContentBlock
  hidePinTerms?: boolean
}) {
  switch (block.type) {
    case 'markdown':
      return <MarkdownBlock text={block.text} />
    case 'math':
      return <MathBlock latex={block.latex} display={block.display} />
    case 'diagram':
      return (
        <DiagramBlock
          source={block.source}
          syntax={block.syntax}
          svgCache={block.svg_cache}
          nodeBindings={block.node_bindings}
        />
      )
    case 'image_occlusion':
      return <ImageOcclusionBlock resourceId={block.resource_id} masks={block.masks} />
    case 'interactive_pins':
      return (
        <InteractivePinsBlock
          resourceId={block.resource_id}
          overlayResourceId={block.overlay_resource_id}
          pins={block.pins}
          hideLabelsAndTerms={hidePinTerms}
        />
      )
    default: {
      const exhaustive: never = block
      return exhaustive
    }
  }
}
