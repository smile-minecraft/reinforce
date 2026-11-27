import type {
  Anchor,
  Edge,
  ExamSet,
  GraphNode,
  KnowledgeCard,
  Note,
  Proposal,
  QuestionCard,
  Resource,
  WeaknessReport,
} from '../types'

const PHASE_DIAGRAM_SVG = `<svg xmlns="http://www.w3.org/2000/svg" width="800" height="500" viewBox="0 0 800 500">
  <rect width="800" height="500" fill="#f7fafc"/>
  <line x1="90" y1="430" x2="740" y2="430" stroke="#334155" stroke-width="3"/>
  <line x1="90" y1="430" x2="90" y2="50" stroke="#334155" stroke-width="3"/>
  <path d="M90 330 L300 210 L520 260 L740 120" fill="none" stroke="#2563eb" stroke-width="4"/>
  <path d="M300 210 L470 100 L740 120" fill="none" stroke="#dc2626" stroke-width="4"/>
  <text x="400" y="470" font-family="sans-serif" font-size="22" fill="#334155">Temperature</text>
  <text x="20" y="250" font-family="sans-serif" font-size="22" fill="#334155" transform="rotate(-90 20 250)">Pressure</text>
  <text x="120" y="380" font-family="sans-serif" font-size="20" fill="#0f172a">solid</text>
  <text x="420" y="370" font-family="sans-serif" font-size="20" fill="#0f172a">liquid</text>
  <text x="420" y="150" font-family="sans-serif" font-size="20" fill="#0f172a">vapor</text>
</svg>`

const PHASE_LABELS_SVG = `<svg xmlns="http://www.w3.org/2000/svg" width="800" height="500" viewBox="0 0 800 500">
  <circle cx="320" cy="205" r="18" fill="none" stroke="#7c3aed" stroke-width="5"/>
  <line x1="320" y1="205" x2="430" y2="125" stroke="#7c3aed" stroke-width="3"/>
  <text x="435" y="130" font-family="sans-serif" font-size="24" fill="#6d28d9">triple point</text>
  <circle cx="600" cy="125" r="18" fill="none" stroke="#059669" stroke-width="5"/>
  <line x1="600" y1="125" x2="520" y2="60" stroke="#059669" stroke-width="3"/>
  <text x="430" y="55" font-family="sans-serif" font-size="24" fill="#047857">critical point</text>
</svg>`

const ADAPTIVE_FLOW_SVG = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 160" width="640" height="160">
  <g stroke="currentColor" fill="none" stroke-width="2">
    <rect x="20" y="44" width="150" height="72" rx="10" />
    <rect x="245" y="44" width="150" height="72" rx="10" />
    <rect x="470" y="44" width="150" height="72" rx="10" />
    <line x1="170" y1="80" x2="245" y2="80" />
    <polyline points="235,73 245,80 235,87" />
    <line x1="395" y1="80" x2="470" y2="80" />
    <polyline points="460,73 470,80 460,87" />
  </g>
  <g fill="currentColor" font-family="sans-serif" font-size="16" text-anchor="middle">
    <text x="95" y="76">Solid</text>
    <text x="95" y="98" font-size="12" opacity="0.7">+ ΔH fusion</text>
    <text x="320" y="76">Liquid</text>
    <text x="320" y="98" font-size="12" opacity="0.7">+ ΔH vap</text>
    <text x="545" y="76">Vapor</text>
    <text x="545" y="98" font-size="12" opacity="0.7">T &gt; Tc supercritical</text>
  </g>
</svg>`

function svgDataUrl(svg: string): string {
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`
}

export const MOCK_BLOB_URLS: Record<string, string> = {
  '01a21714-c98c-7bfe-8cf8-a60b84573f56': svgDataUrl(PHASE_DIAGRAM_SVG),
  '01a21714-c98c-78d5-8550-3d0928b9c9d1': svgDataUrl(PHASE_LABELS_SVG),
}

export const MOCK_RESOURCES: Resource[] = [
  {
    id: '01a21714-c98c-7bfe-8cf8-a60b84573f56',
    kind: 'image',
    title: 'phase-diagram.svg',
    sha256: '22df5f119108ae55882dc11a03d09ba426cf4771ef92eaa418457f8db40cbe4a',
    byte_size: 983,
    page_count: null,
    width: 800,
    height: 500,
    license: 'CC0-1.0',
    attribution_url: null,
    attribution_notice: 'Original diagram created for the Reinforce demo dataset.',
    sensitivity: 'normal',
    export_policy: 'allow',
    created_at: '2026-01-01T00:00:00.000000Z',
  },
  {
    id: '01a21714-c98c-78d5-8550-3d0928b9c9d1',
    kind: 'image',
    title: 'phase-labels.svg',
    sha256: 'd5115bdba60ace7b71df3758d174dec49382a539f34fd007ee263692f49e5179',
    byte_size: 619,
    page_count: null,
    width: 800,
    height: 500,
    license: 'CC0-1.0',
    attribution_url: null,
    attribution_notice: 'Original label overlay created for the Reinforce demo dataset.',
    sensitivity: 'restricted',
    export_policy: 'redact',
    created_at: '2026-01-01T00:00:00.000000Z',
  },
]

export const MOCK_ANCHORS: Anchor[] = [
  {
    id: '01a21714-c98d-7814-a2c2-b8efc8433c2c',
    resource_id: '01a21714-c98c-7bfe-8cf8-a60b84573f56',
    anchor_kind: 'rect',
    pdf_page: null,
    printed_page: 42,
    legend_pdf_page: null,
    rect: { x: 0.28, y: 0.24, w: 0.34, h: 0.32 },
    media_span: null,
  },
  {
    id: '01a21714-c98d-7814-a2c2-b8efc8433c2d',
    resource_id: '01a21714-c98c-7bfe-8cf8-a60b84573f56',
    anchor_kind: 'rect',
    pdf_page: null,
    printed_page: 43,
    legend_pdf_page: null,
    rect: { x: 0.56, y: 0.1, w: 0.34, h: 0.32 },
    media_span: null,
  },
]

export const MOCK_KNOWLEDGE_CARDS: KnowledgeCard[] = [
  {
    id: '01a21714-c98d-716e-9b53-ead7c8ee810f',
    subject: 'physics',
    topics: ['thermodynamics', 'phase changes'],
    title: '水的三態與相變',
    blocks: [
      {
        type: 'markdown',
        text: `| 相變 | 吸放熱 | 常見例子 |
| --- | --- | --- |
| 熔化 | 吸熱 | 冰變成水 |
| 汽化 | 吸熱 | 水變成蒸氣 |
| 凝華 | 放熱 | 蒸氣直接變成霜 |

> [!tip] 判別口訣
> 固 → 液 → 氣分子自由度上升，都需要吸收潛熱；反向過程則釋放潛熱。`,
      },
      {
        type: 'diagram',
        syntax: 'svg',
        source: 'Solid --> Liquid --> Vapor',
        svg_cache: ADAPTIVE_FLOW_SVG,
        node_bindings: [],
      },
    ],
    anchor_ids: ['01a21714-c98d-7814-a2c2-b8efc8433c2c'],
    status: 'published',
    verification: 'verified',
    unsure_count: 0,
    metadata: {},
    created_at: '2026-01-01T00:00:00.000000Z',
    updated_at: '2026-01-01T00:00:00.000000Z',
  },
  {
    id: '01a21714-c98d-7210-a469-b56decf9f22b',
    subject: 'physics',
    topics: ['thermodynamics'],
    title: '克拉珀龍方程與化學平衡',
    blocks: [
      {
        type: 'math',
        latex: '\\frac{dP}{dT} = \\frac{L}{T\\Delta v}',
        display: true,
      },
      {
        type: 'math',
        latex: '\\ce{H2O(s) <=> H2O(l) <=> H2O(g)}',
        display: true,
      },
      {
        type: 'diagram',
        syntax: 'mermaid',
        source: 'flowchart LR\n  S[Solid] -->|Melting| L[Liquid]\n  L -->|Vaporization| G[Vapor]\n  G -->|Deposition| S',
        svg_cache: null,
        node_bindings: [],
      },
    ],
    anchor_ids: [],
    status: 'published',
    verification: 'verified',
    unsure_count: 0,
    metadata: {},
    created_at: '2026-01-01T00:00:00.000000Z',
    updated_at: '2026-01-01T00:00:00.000000Z',
  },
  {
    id: '01a21714-c98d-7b1d-92e4-b2d361f54d7f',
    subject: 'physics',
    topics: ['phase diagram'],
    title: '相位圖上的特徵點',
    blocks: [
      {
        type: 'interactive_pins',
        resource_id: '01a21714-c98c-7bfe-8cf8-a60b84573f56',
        overlay_resource_id: '01a21714-c98c-78d5-8550-3d0928b9c9d1',
        pins: [
          {
            no: 1,
            x: 0.4,
            y: 0.41,
            label_x: 0.54,
            label_y: 0.25,
            term_primary: '三相點',
            term_secondary: 'triple point',
            detail: '固、液、氣三相達成熱力學平衡共存的唯一溫度與壓力點。',
            hint: '三條相邊界交會處',
            linked_card_id: '01a21714-c98d-716e-9b53-ead7c8ee810f',
          },
          {
            no: 2,
            x: 0.75,
            y: 0.25,
            label_x: 0.54,
            label_y: 0.12,
            term_primary: '臨界點',
            term_secondary: 'critical point',
            detail: '超過此溫度與壓力後，液相與氣相的邊界消失，形成超臨界流體。',
            hint: '汽化曲線終點',
            linked_card_id: null,
          },
        ],
      },
      {
        type: 'image_occlusion',
        resource_id: '01a21714-c98c-7bfe-8cf8-a60b84573f56',
        masks: [
          { group_id: 'solid', rect: { x: 0.13, y: 0.71, w: 0.11, h: 0.08 } },
          { group_id: 'liquid', rect: { x: 0.5, y: 0.69, w: 0.12, h: 0.08 } },
          { group_id: 'vapor', rect: { x: 0.5, y: 0.25, w: 0.12, h: 0.08 } },
        ],
      },
    ],
    anchor_ids: ['01a21714-c98d-7814-a2c2-b8efc8433c2c'],
    status: 'published',
    verification: 'verified',
    unsure_count: 0,
    metadata: {},
    created_at: '2026-01-01T00:00:00.000000Z',
    updated_at: '2026-01-01T00:00:00.000000Z',
  },
]

export const MOCK_NOTES: Note[] = [
  {
    id: '01a21714-c98d-769f-be7a-f92b11e5cc60',
    subject: 'physics',
    title: '相位變化入門',
    outline: [
      {
        type: 'section',
        heading: '讀圖與相變',
        intro_markdown: '從相位圖出發，認識熔化、汽化與臨界現象。',
        children: [
          { type: 'card_ref', card_id: '01a21714-c98d-716e-9b53-ead7c8ee810f' },
          { type: 'card_ref', card_id: '01a21714-c98d-7b1d-92e4-b2d361f54d7f' },
        ],
      },
      {
        type: 'section',
        heading: '定量關係',
        intro_markdown: '利用克拉珀龍方程理解相界曲線的斜率變化。',
        children: [
          { type: 'card_ref', card_id: '01a21714-c98d-7210-a469-b56decf9f22b' },
        ],
      },
    ],
    view_settings: {},
    status: 'published',
    created_at: '2026-01-01T00:00:00.000000Z',
    updated_at: '2026-01-01T00:00:00.000000Z',
  },
]

export const MOCK_EXAM_SETS: ExamSet[] = [
  {
    id: '01a21714-c98d-7931-a48a-6a1d5cd348bd',
    subject: 'physics',
    title: '相位變化小測',
    source_kind: 'original',
    exam_year: null,
    source_resource_id: '01a21714-c98c-7bfe-8cf8-a60b84573f56',
    expected_questions: 4,
    metadata: {},
    is_complete: true,
  },
]

export const MOCK_QUESTIONS: QuestionCard[] = [
  {
    id: '01a21714-c98d-70f7-90ba-1bced11dd072',
    exam_set_id: '01a21714-c98d-7931-a48a-6a1d5cd348bd',
    question_group_id: null,
    question_no: 1,
    subject: 'physics',
    topics: ['phase changes'],
    stem_blocks: [{ type: 'markdown', text: '冰變成液態水時，系統吸收還是放出熱量？' }],
    explanation_blocks: [
      { type: 'markdown', text: '熔化過程需要克服固體晶格鍵結，因此吸收熔化潛熱。' },
    ],
    anchor_ids: ['01a21714-c98d-7814-a2c2-b8efc8433c2c'],
    question_type: 'single_choice',
    grading_mode: 'exact',
    options: [
      {
        key: 'A',
        content: '吸收熱量',
        explanation: '正確：固態轉為液態屬於吸熱相變。',
        linked_card_ids: ['01a21714-c98d-716e-9b53-ead7c8ee810f'],
      },
      {
        key: 'B',
        content: '放出熱量',
        explanation: '凝固（液態變固態）才是放熱。',
        linked_card_ids: ['01a21714-c98d-716e-9b53-ead7c8ee810f'],
      },
      {
        key: 'C',
        content: '不吸不放',
        explanation: '溫度雖暫時不變，但仍需吸收潛熱。',
        linked_card_ids: [],
      },
    ],
    answer: ['A'],
    accepted_answers: [],
    answer_source_kind: 'original_authored',
    answer_basis: '原創題，依據熔化需要吸收潛熱。',
    duplicate_question_id: null,
    status: 'published',
    verification: 'verified',
    unsure_count: 0,
    metadata: {},
    created_at: '2026-01-01T00:00:00.000000Z',
    updated_at: '2026-01-01T00:00:00.000000Z',
  },
  {
    id: '01a21714-c98d-7dae-9885-d1852753b72a',
    exam_set_id: '01a21714-c98d-7931-a48a-6a1d5cd348bd',
    question_group_id: null,
    question_no: 2,
    subject: 'physics',
    topics: ['phase changes'],
    stem_blocks: [{ type: 'markdown', text: '哪些過程需要吸收熱量？' }],
    explanation_blocks: [
      { type: 'markdown', text: '熔化（固→液）與汽化（液→氣）皆為吸熱過程。' },
    ],
    anchor_ids: [],
    question_type: 'multiple_choice',
    grading_mode: 'exact',
    options: [
      {
        key: 'A',
        content: '熔化',
        explanation: '固態轉液態需吸收熱量。',
        linked_card_ids: ['01a21714-c98d-716e-9b53-ead7c8ee810f'],
      },
      { key: 'B', content: '凝結', explanation: '氣態轉液態為放熱。', linked_card_ids: [] },
      {
        key: 'C',
        content: '汽化',
        explanation: '液態轉氣態需吸收汽化熱。',
        linked_card_ids: ['01a21714-c98d-716e-9b53-ead7c8ee810f'],
      },
      { key: 'D', content: '凝固', explanation: '液態轉固態為放熱。', linked_card_ids: [] },
    ],
    answer: ['A', 'C'],
    accepted_answers: [],
    answer_source_kind: 'original_authored',
    answer_basis: '原創題，熔化與汽化皆為吸熱相變。',
    duplicate_question_id: null,
    status: 'published',
    verification: 'verified',
    unsure_count: 0,
    metadata: {},
    created_at: '2026-01-01T00:00:00.000000Z',
    updated_at: '2026-01-01T00:00:00.000000Z',
  },
  {
    id: '01a21714-c98d-7c8f-800b-0200f1cba221',
    exam_set_id: '01a21714-c98d-7931-a48a-6a1d5cd348bd',
    question_group_id: null,
    question_no: 3,
    subject: 'physics',
    topics: ['phase changes'],
    stem_blocks: [
      { type: 'markdown', text: '液態水在一大氣壓下達到 100 °C 時發生的相變叫什麼？' },
    ],
    explanation_blocks: [
      { type: 'markdown', text: '本題接受中文「沸騰」或英文「boiling」（申覆多解模式）。' },
    ],
    anchor_ids: [],
    question_type: 'short_answer',
    grading_mode: 'any_accepted',
    options: [],
    answer: ['沸騰'],
    accepted_answers: [['沸騰'], ['boiling']],
    answer_source_kind: 'original_authored',
    answer_basis: '原創題，接受中文或英文答案。',
    duplicate_question_id: null,
    status: 'published',
    verification: 'verified',
    unsure_count: 0,
    metadata: {},
    created_at: '2026-01-01T00:00:00.000000Z',
    updated_at: '2026-01-01T00:00:00.000000Z',
  },
  {
    id: '01a21714-c98d-71cc-aab1-7b9fa93a2977',
    exam_set_id: '01a21714-c98d-7931-a48a-6a1d5cd348bd',
    question_group_id: null,
    question_no: 4,
    subject: 'physics',
    topics: [],
    stem_blocks: [
      { type: 'markdown', text: '此題保留為完整試卷的送分題，不計入有效分母與 FSRS 排程。' },
    ],
    explanation_blocks: [],
    anchor_ids: [],
    question_type: 'short_answer',
    grading_mode: 'voided',
    options: [],
    answer: [],
    accepted_answers: [],
    answer_source_kind: 'unanswered',
    answer_basis: null,
    duplicate_question_id: null,
    status: 'published',
    verification: 'verified',
    unsure_count: 0,
    metadata: {},
    created_at: '2026-01-01T00:00:00.000000Z',
    updated_at: '2026-01-01T00:00:00.000000Z',
  },
  {
    id: '01a21714-c98d-7e22-89a1-111111111101',
    exam_set_id: null,
    question_group_id: null,
    question_no: null,
    subject: 'physics',
    topics: ['phase diagram'],
    stem_blocks: [
      { type: 'markdown', text: '請在相位圖上點擊標記 **#1 三相點（triple point）** 的位置。' },
      {
        type: 'interactive_pins',
        resource_id: '01a21714-c98c-7bfe-8cf8-a60b84573f56',
        overlay_resource_id: '01a21714-c98c-78d5-8550-3d0928b9c9d1',
        pins: [
          {
            no: 1,
            x: 0.4,
            y: 0.41,
            label_x: 0.54,
            label_y: 0.25,
            term_primary: '三相點',
            term_secondary: 'triple point',
            detail: '三條相邊界交會處',
            hint: '固、液、氣三相交會點',
            linked_card_id: '01a21714-c98d-7b1d-92e4-b2d361f54d7f',
          },
        ],
      },
    ],
    explanation_blocks: [
      { type: 'markdown', text: '三相點位於溫度與壓力相圖中三條相界線交會處（x=0.40, y=0.41）。' },
    ],
    anchor_ids: ['01a21714-c98d-7814-a2c2-b8efc8433c2c'],
    question_type: 'pin_locate',
    grading_mode: 'exact',
    options: [],
    answer: ['1'],
    accepted_answers: [],
    answer_source_kind: 'original_authored',
    answer_basis: '依據相位圖三相交會座標。',
    duplicate_question_id: null,
    status: 'published',
    verification: 'verified',
    unsure_count: 0,
    metadata: {},
    created_at: '2026-01-01T00:00:00.000000Z',
    updated_at: '2026-01-01T00:00:00.000000Z',
  },
  {
    id: '01a21714-c98d-7e22-89a1-111111111102',
    exam_set_id: null,
    question_group_id: null,
    question_no: null,
    subject: 'physics',
    topics: ['phase diagram'],
    stem_blocks: [
      { type: 'markdown', text: '請寫出圖中圖釘 **#2** 所指構造的名稱（中文或英文皆可）。' },
      {
        type: 'interactive_pins',
        resource_id: '01a21714-c98c-7bfe-8cf8-a60b84573f56',
        overlay_resource_id: null,
        pins: [
          {
            no: 2,
            x: 0.75,
            y: 0.25,
            label_x: 0.54,
            label_y: 0.12,
            term_primary: '臨界點',
            term_secondary: 'critical point',
            detail: '汽化曲線終點',
            hint: '超過此點進入超臨界流體',
            linked_card_id: '01a21714-c98d-7b1d-92e4-b2d361f54d7f',
          },
        ],
      },
    ],
    explanation_blocks: [{ type: 'markdown', text: '圖釘 #2 為臨界點（critical point）。' }],
    anchor_ids: [],
    question_type: 'pin_identify',
    grading_mode: 'any_accepted',
    options: [],
    answer: ['2'],
    accepted_answers: [['臨界點'], ['critical point']],
    answer_source_kind: 'original_authored',
    answer_basis: '依據相位圖臨界點標註。',
    duplicate_question_id: null,
    status: 'published',
    verification: 'verified',
    unsure_count: 0,
    metadata: {},
    created_at: '2026-01-01T00:00:00.000000Z',
    updated_at: '2026-01-01T00:00:00.000000Z',
  },
]

export const MOCK_EDGES: Edge[] = [
  {
    id: '01a21714-c98d-7153-8d11-f9450c86de70',
    edge_type: 'confusable_with',
    from: { kind: 'card', id: '01a21714-c98d-7210-a469-b56decf9f22b' },
    to: { kind: 'card', id: '01a21714-c98d-716e-9b53-ead7c8ee810f' },
    created_at: '2026-01-01T00:00:00.000000Z',
  },
  {
    id: '01a21714-c98d-7153-8d11-f9450c86de71',
    edge_type: 'prerequisite',
    from: { kind: 'card', id: '01a21714-c98d-716e-9b53-ead7c8ee810f' },
    to: { kind: 'card', id: '01a21714-c98d-7b1d-92e4-b2d361f54d7f' },
    created_at: '2026-01-01T00:00:00.000000Z',
  },
]

export const MOCK_GRAPH: Record<string, GraphNode[]> = {
  'card:01a21714-c98d-716e-9b53-ead7c8ee810f': [
    { endpoint: { kind: 'card', id: '01a21714-c98d-7210-a469-b56decf9f22b' }, depth: 1 },
    { endpoint: { kind: 'question', id: '01a21714-c98d-70f7-90ba-1bced11dd072' }, depth: 1 },
  ],
  'card:01a21714-c98d-7210-a469-b56decf9f22b': [
    { endpoint: { kind: 'card', id: '01a21714-c98d-716e-9b53-ead7c8ee810f' }, depth: 1 },
  ],
  'card:01a21714-c98d-7b1d-92e4-b2d361f54d7f': [
    { endpoint: { kind: 'card', id: '01a21714-c98d-716e-9b53-ead7c8ee810f' }, depth: 1 },
  ],
}

export const MOCK_WEAKNESS: WeaknessReport = {
  knowledge_cards: [
    {
      card_id: '01a21714-c98d-7b1d-92e4-b2d361f54d7f',
      attempts: 4,
      error_rate: 0.5,
      hesitant_rate: 0.5,
      average_duration_ms: 19200,
      score: 0.535,
    },
    {
      card_id: '01a21714-c98d-716e-9b53-ead7c8ee810f',
      attempts: 6,
      error_rate: 0.17,
      hesitant_rate: 0.33,
      average_duration_ms: 11400,
      score: 0.26,
    },
  ],
  questions: [
    {
      question_id: '01a21714-c98d-7dae-9885-d1852753b72a',
      attempts: 3,
      error_rate: 0.67,
      hesitant_rate: 0.67,
      average_duration_ms: 21500,
      score: 0.68,
      linked_card_ids: ['01a21714-c98d-716e-9b53-ead7c8ee810f'],
    },
  ],
  confusable_pairs: [
    {
      question_id: '01a21714-c98d-70f7-90ba-1bced11dd072',
      wrong_option: 'B',
      correct_option: 'A',
      count: 2,
    },
  ],
}

export const MOCK_PROPOSALS: Proposal[] = [
  {
    id: '01a21714-d100-7000-8000-000000000101',
    target: { knowledge_card: '01a21714-d100-7000-8000-000000000201' },
    payload: {
      format: 'reinforce_batch_v1',
      actor: 'agent:ingestor',
      operations: [
        {
          op: 'knowledge_card',
          temp_ref: '$card_phase_pins',
          id: '01a21714-d100-7000-8000-000000000201',
          data: {
            id: '01a21714-d100-7000-8000-000000000201',
            subject: 'physics',
            topics: ['thermodynamics', 'phase diagram'],
            title: '相圖關鍵特徵點（待校準圖釘草稿）',
            blocks: [
              {
                type: 'markdown',
                text: `| 特徵點 | 物理意義 | 相邊界狀態 |
| --- | --- | --- |
| 三相點 | 固、液、氣三相平衡共存 | 三條相界線唯一交點 |
| 臨界點 | 液相與氣相密度相等、界面消失 | 汽化曲線終點 |`,
              },
              {
                type: 'interactive_pins',
                resource_id: '01a21714-c98c-7bfe-8cf8-a60b84573f56',
                overlay_resource_id: '01a21714-c98c-78d5-8550-3d0928b9c9d1',
                pins: [
                  {
                    no: 1,
                    x: 0.37,
                    y: 0.44,
                    label_x: 0.56,
                    label_y: 0.28,
                    term_primary: '三相點',
                    term_secondary: 'triple point',
                    detail: '固、液、氣三相達平衡共存的唯一 (T, P) 點。',
                    hint: '試著拖拉針尖精準對準三條相線交會處',
                    linked_card_id: '01a21714-c98d-716e-9b53-ead7c8ee810f',
                  },
                  {
                    no: 2,
                    x: 0.72,
                    y: 0.28,
                    label_x: 0.58,
                    label_y: 0.14,
                    term_primary: '臨界點',
                    term_secondary: 'critical point',
                    detail: '高於臨界溫度與壓力即形成超臨界流體。',
                    hint: '位於汽化曲線右上端點',
                    linked_card_id: '01a21714-c98d-7210-a469-b56decf9f22b',
                  },
                ],
              },
            ],
            anchor_ids: [
              '01a21714-c98d-7814-a2c2-b8efc8433c2c',
              '01a21714-c98d-7814-a2c2-b8efc8433c2d',
            ],
            status: 'staging',
            verification: 'unchecked',
            unsure_count: 1,
            metadata: {},
            created_at: '2026-11-26T09:10:00.000000Z',
            updated_at: '2026-11-26T09:10:00.000000Z',
          },
        },
      ],
    },
    submitted_by: 'agent:ingestor',
    status: 'pending',
    review_note: null,
    created_at: '2026-11-26T09:10:00.000000Z',
    updated_at: '2026-11-26T09:10:00.000000Z',
    sync_cursor: 101,
  },
  {
    id: '01a21714-d100-7000-8000-000000000102',
    target: { question_card: '01a21714-d100-7000-8000-000000000202' },
    payload: {
      format: 'reinforce_batch_v1',
      actor: 'agent:quiz-extractor',
      operations: [
        {
          op: 'question_card',
          temp_ref: '$q_supercritical',
          id: '01a21714-d100-7000-8000-000000000202',
          data: {
            id: '01a21714-d100-7000-8000-000000000202',
            exam_set_id: '01a21714-c98d-7931-a48a-6a1d5cd348bd',
            question_group_id: null,
            question_no: 5,
            subject: 'physics',
            topics: ['phase diagram', 'critical point'],
            stem_blocks: [
              {
                type: 'markdown',
                text: '當純物質的溫度與壓力同時高於臨界點（critical point）時，下列敘述何者正確？',
              },
            ],
            explanation_blocks: [
              {
                type: 'markdown',
                text: '超過臨界點後，液相與氣相的密度趨於一致、相界面消失，系統成為單一的超臨界流體（supercritical fluid）。',
              },
            ],
            anchor_ids: ['01a21714-c98d-7814-a2c2-b8efc8433c2d'],
            question_type: 'single_choice',
            grading_mode: 'exact',
            options: [
              {
                key: 'A',
                content: '固、液、氣三相同時平衡共存',
                explanation: '此為三相點的定義，非臨界點以上狀態。',
                linked_card_ids: ['01a21714-c98d-7b1d-92e4-b2d361f54d7f'],
              },
              {
                key: 'B',
                content: '液相與氣相界面消失，形成超臨界流體',
                explanation: '正確：超過 Tc 與 Pc 後不再有明顯液氣相界。',
                linked_card_ids: [
                  '01a21714-c98d-7b1d-92e4-b2d361f54d7f',
                  '01a21714-c98d-716e-9b53-ead7c8ee810f',
                ],
              },
              {
                key: 'C',
                content: '物質必然發生凝華轉為固態',
                explanation: '高溫高壓下不會直接凝華。',
                linked_card_ids: ['01a21714-c98d-716e-9b53-ead7c8ee810f'],
              },
            ],
            answer: ['B'],
            accepted_answers: [],
            answer_source_kind: 'inferred',
            answer_basis: '依據相圖 p.43 臨界點標註區域：汽化曲線終止於臨界點，上方為超臨界流體區。',
            duplicate_question_id: null,
            status: 'staging',
            verification: 'unchecked',
            unsure_count: 0,
            metadata: {},
            created_at: '2026-11-26T09:20:00.000000Z',
            updated_at: '2026-11-26T09:20:00.000000Z',
          },
        },
        {
          op: 'edge',
          temp_ref: '$edge_tests_1',
          id: '01a21714-d100-7000-8000-000000000203',
          data: {
            id: '01a21714-d100-7000-8000-000000000203',
            edge_type: 'tests',
            from: { kind: 'question', id: '01a21714-d100-7000-8000-000000000202' },
            to: { kind: 'card', id: '01a21714-c98d-7b1d-92e4-b2d361f54d7f' },
            created_at: '2026-11-26T09:20:00.000000Z',
          },
        },
      ],
    },
    submitted_by: 'agent:quiz-extractor',
    status: 'pending',
    review_note: null,
    created_at: '2026-11-26T09:20:00.000000Z',
    updated_at: '2026-11-26T09:20:00.000000Z',
    sync_cursor: 102,
  },
  {
    id: '01a21714-d100-7000-8000-000000000103',
    target: { note: '01a21714-d100-7000-8000-000000000204' },
    payload: {
      format: 'reinforce_batch_v1',
      actor: 'agent:outliner',
      operations: [
        {
          op: 'note',
          temp_ref: '$note_thermo',
          id: '01a21714-d100-7000-8000-000000000204',
          data: {
            id: '01a21714-d100-7000-8000-000000000204',
            subject: 'physics',
            title: '熱力學相變總複習（草稿）',
            outline: [
              {
                type: 'section',
                heading: '核心概念與方程式',
                intro_markdown: '快速整理潛熱與克拉珀龍方程的重點對應。',
                children: [
                  { type: 'card_ref', card_id: '01a21714-c98d-716e-9b53-ead7c8ee810f' },
                  { type: 'card_ref', card_id: '01a21714-c98d-7210-a469-b56decf9f22b' },
                ],
              },
            ],
            view_settings: {},
            status: 'staging',
            created_at: '2026-11-26T09:30:00.000000Z',
            updated_at: '2026-11-26T09:30:00.000000Z',
          },
        },
      ],
    },
    submitted_by: 'agent:outliner',
    status: 'pending',
    review_note: null,
    created_at: '2026-11-26T09:30:00.000000Z',
    updated_at: '2026-11-26T09:30:00.000000Z',
    sync_cursor: 103,
  },
  {
    id: '01a21714-d100-7000-8000-000000000104',
    target: { knowledge_card: '01a21714-c98d-716e-9b53-ead7c8ee810f' },
    payload: {
      format: 'reinforce_batch_v1',
      actor: 'agent:ingestor',
      operations: [
        {
          op: 'knowledge_card',
          temp_ref: '$card_water',
          id: '01a21714-c98d-716e-9b53-ead7c8ee810f',
          data: MOCK_KNOWLEDGE_CARDS[0],
        },
      ],
    },
    submitted_by: 'agent:ingestor',
    status: 'approved',
    review_note: null,
    created_at: '2026-11-25T14:00:00.000000Z',
    updated_at: '2026-11-25T14:12:00.000000Z',
    sync_cursor: 90,
  },
  {
    id: '01a21714-d100-7000-8000-000000000105',
    target: { question_card: '01a21714-d100-7000-8000-000000000205' },
    payload: {
      format: 'reinforce_batch_v1',
      actor: 'agent:quiz-extractor',
      operations: [
        {
          op: 'question_card',
          temp_ref: '$q_missing_basis',
          id: '01a21714-d100-7000-8000-000000000205',
          data: {
            id: '01a21714-d100-7000-8000-000000000205',
            exam_set_id: null,
            question_group_id: null,
            question_no: null,
            subject: 'physics',
            topics: ['phase changes'],
            stem_blocks: [{ type: 'markdown', text: '水的三相點溫度約為幾 K？' }],
            explanation_blocks: [],
            anchor_ids: ['01a21714-c98d-7814-a2c2-b8efc8433c2c'],
            question_type: 'short_answer',
            grading_mode: 'exact',
            options: [],
            answer: ['273.16 K'],
            accepted_answers: [],
            answer_source_kind: 'inferred',
            answer_basis: '',
            duplicate_question_id: null,
            status: 'staging',
            verification: 'unchecked',
            unsure_count: 1,
            metadata: {},
            created_at: '2026-11-25T12:00:00.000000Z',
            updated_at: '2026-11-25T12:30:00.000000Z',
          },
        },
      ],
    },
    submitted_by: 'agent:quiz-extractor',
    status: 'rejected',
    review_note: '請補齊 answer_basis，並標明原檔頁碼與數據出處。',
    created_at: '2026-11-25T12:00:00.000000Z',
    updated_at: '2026-11-25T12:30:00.000000Z',
    sync_cursor: 91,
  },
]
