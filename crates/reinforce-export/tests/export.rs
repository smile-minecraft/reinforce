use std::path::Path;

use lopdf::Document;
use reinforce_core::content::{NormalizedRect, OcclusionMask, OutlineNode, Pin, QuestionOption};
use reinforce_core::primitives::{
    AnswerSourceKind, CardStatus, DiagramSyntax, ExportPolicy, GradingMode, QuestionType,
    ResourceKind, Sensitivity, Verification,
};
use reinforce_core::{
    ContentBlock, ContentHash, KnowledgeCard, KnowledgeCardId, Note, QuestionCard, Resource,
    ResourceId, Timestamp,
};
use reinforce_export::{
    ExportInput, MarkdownOptions, PdfOptions, PdfPreset, export_markdown, import_markdown,
    render_pdf, render_typst_source,
};
use serde_json::json;
use tempfile::TempDir;

fn fonts() -> TempDir {
    let dir = TempDir::new().expect("temp dir");
    for (index, font) in typst_assets::fonts().enumerate() {
        std::fs::write(dir.path().join(format!("font-{index}.ttf")), font).expect("font file");
    }
    dir
}

fn svg(label: &str) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"160\" height=\"100\"><rect width=\"160\" height=\"100\" fill=\"#eef2ff\"/><text x=\"8\" y=\"20\">{}</text></svg>",
        label
    )
}

struct Fixture {
    input: ExportInput,
    allow_bytes: Vec<u8>,
    redact_bytes: Vec<u8>,
    owner_bytes: Vec<u8>,
}

fn fixture() -> Fixture {
    let allow_bytes = svg("allow").into_bytes();
    let redact_bytes = svg("redact").into_bytes();
    let owner_bytes = svg("owner").into_bytes();
    let allow_hash = ContentHash::from_bytes(&allow_bytes);
    let redact_hash = ContentHash::from_bytes(&redact_bytes);
    let owner_hash = ContentHash::from_bytes(&owner_bytes);

    let resource = |title: &str, hash: &ContentHash, policy: ExportPolicy| Resource {
        id: ResourceId::generate(),
        kind: ResourceKind::Image,
        title: title.to_owned(),
        sha256: hash.clone(),
        byte_size: 32,
        page_count: None,
        width: Some(160),
        height: Some(100),
        license: Some("CC0".to_owned()),
        attribution_url: None,
        attribution_notice: None,
        sensitivity: Sensitivity::Restricted,
        export_policy: policy,
        created_at: Timestamp::unix_epoch(),
    };
    let allow = resource("allow specimen", &allow_hash, ExportPolicy::Allow);
    let redact = resource("redact specimen", &redact_hash, ExportPolicy::Redact);
    let owner_only = resource("owner specimen", &owner_hash, ExportPolicy::OwnerOnly);

    let card_id = KnowledgeCardId::generate();
    let pin_card = KnowledgeCard {
        id: card_id.clone(),
        subject: "Test subject".to_owned(),
        topics: vec!["diagram".to_owned()],
        title: "Pinned diagram".to_owned(),
        blocks: vec![
            ContentBlock::Markdown {
                text: "# Inner heading\n\n| A | B |\n| --- | --- |\n| 1 | 2 |\n\n**bold** and *italic*".to_owned(),
            },
            ContentBlock::Math {
                latex: "\\alpha + \\beta".to_owned(),
                display: true,
            },
            ContentBlock::Diagram {
                source: "A --> B".to_owned(),
                syntax: DiagramSyntax::Mermaid,
                svg_cache: Some(svg("diagram")),
                node_bindings: Vec::new(),
            },
            ContentBlock::ImageOcclusion {
                resource_id: redact.id.clone(),
                masks: vec![OcclusionMask {
                    group_id: "mask".to_owned(),
                    rect: NormalizedRect {
                        x: 0.1,
                        y: 0.1,
                        w: 0.2,
                        h: 0.2,
                    },
                }],
            },
            ContentBlock::InteractivePins {
                resource_id: allow.id.clone(),
                overlay_resource_id: None,
                pins: vec![Pin {
                    no: 1,
                    x: 0.2,
                    y: 0.3,
                    label_x: 0.6,
                    label_y: 0.35,
                    term_primary: "Primary term".to_owned(),
                    term_secondary: Some("Secondary term".to_owned()),
                    detail: Some("Detail".to_owned()),
                    hint: Some("Hint text".to_owned()),
                    linked_card_id: None,
                }],
            },
            ContentBlock::InteractivePins {
                resource_id: owner_only.id.clone(),
                overlay_resource_id: None,
                pins: vec![Pin {
                    no: 2,
                    x: 0.4,
                    y: 0.5,
                    label_x: 0.7,
                    label_y: 0.6,
                    term_primary: "Owner secret".to_owned(),
                    term_secondary: None,
                    detail: None,
                    hint: None,
                    linked_card_id: None,
                }],
            },
        ],
        anchor_ids: Vec::new(),
        status: CardStatus::Published,
        verification: Verification::Verified,
        unsure_count: 0,
        metadata: json!({"source": "fixture"}),
        created_at: Timestamp::unix_epoch(),
        updated_at: Timestamp::unix_epoch().plus_days(1.0),
    };

    let note = Note {
        id: reinforce_core::NoteId::generate(),
        subject: "Test subject".to_owned(),
        title: "Test note".to_owned(),
        outline: vec![OutlineNode::Section {
            heading: "Section one".to_owned(),
            intro_markdown: Some("Intro paragraph".to_owned()),
            children: vec![OutlineNode::CardRef {
                card_id: card_id.clone(),
            }],
        }],
        view_settings: json!({}),
        status: CardStatus::Published,
        created_at: Timestamp::unix_epoch(),
        updated_at: Timestamp::unix_epoch(),
    };

    let question = QuestionCard {
        id: reinforce_core::QuestionCardId::generate(),
        exam_set_id: None,
        question_group_id: None,
        question_no: Some(1),
        subject: "Test subject".to_owned(),
        topics: vec!["test".to_owned()],
        stem_blocks: vec![ContentBlock::Markdown {
            text: "What is the answer?".to_owned(),
        }],
        explanation_blocks: vec![ContentBlock::Markdown {
            text: "Because of reasoning.".to_owned(),
        }],
        anchor_ids: Vec::new(),
        question_type: QuestionType::SingleChoice,
        grading_mode: GradingMode::Exact,
        options: vec![QuestionOption {
            key: "A".to_owned(),
            content: "Answer".to_owned(),
            explanation: None,
            linked_card_ids: vec![card_id],
        }],
        answer: vec!["A".to_owned()],
        accepted_answers: Vec::new(),
        answer_source_kind: AnswerSourceKind::Official,
        answer_basis: Some("Official key".to_owned()),
        duplicate_question_id: None,
        status: CardStatus::Published,
        verification: Verification::Verified,
        unsure_count: 0,
        metadata: json!({}),
        created_at: Timestamp::unix_epoch(),
        updated_at: Timestamp::unix_epoch(),
    };

    let mut input = ExportInput {
        notes: vec![note],
        cards: vec![pin_card],
        questions: vec![question],
        resources: vec![allow, redact, owner_only],
        blobs: vec![
            reinforce_export::ResourceBlob {
                sha256: allow_hash,
                bytes: allow_bytes.clone(),
            },
            reinforce_export::ResourceBlob {
                sha256: redact_hash,
                bytes: redact_bytes.clone(),
            },
            reinforce_export::ResourceBlob {
                sha256: owner_hash,
                bytes: owner_bytes.clone(),
            },
        ],
        blob_dir: None,
    };
    input
        .blobs
        .sort_by(|left, right| left.sha256.as_str().cmp(right.sha256.as_str()));
    Fixture {
        input,
        allow_bytes,
        redact_bytes,
        owner_bytes,
    }
}

fn options(preset: PdfPreset, fonts_dir: &Path, owner: bool) -> PdfOptions {
    PdfOptions {
        preset,
        fonts_dir: fonts_dir.to_path_buf(),
        web_base_url: Some("https://notes.example.test".to_owned()),
        viewer_is_owner: owner,
    }
}

fn assert_valid_pdf(bytes: &[u8], min_pages: usize) {
    assert!(bytes.starts_with(b"%PDF-"), "missing PDF header");
    let document = Document::load_mem(bytes).expect("lopdf can parse generated PDF");
    assert!(
        document.get_pages().len() >= min_pages,
        "PDF has too few pages"
    );
}

#[test]
fn all_presets_produce_valid_pdfs() {
    let fonts = fonts();
    let fixture = fixture();
    for preset in [
        PdfPreset::Standard,
        PdfPreset::GoodNotes,
        PdfPreset::Worksheet,
    ] {
        let bytes = render_pdf(&fixture.input, &options(preset, fonts.path(), true))
            .unwrap_or_else(|error| panic!("{preset:?} failed: {error}"));
        assert!(!bytes.is_empty());
        let minimum = if preset == PdfPreset::Worksheet { 2 } else { 1 };
        assert_valid_pdf(&bytes, minimum);
    }
}

#[test]
fn diagrams_fit_the_text_column_in_every_preset() {
    let fixture = fixture();
    for preset in [
        PdfPreset::Standard,
        PdfPreset::GoodNotes,
        PdfPreset::Worksheet,
    ] {
        let source = render_typst_source(
            &fixture.input,
            &options(preset, Path::new("unused-fonts"), true),
        )
        .unwrap();
        assert!(
            source.source.contains("layout(size => {"),
            "{preset:?} does not measure the text column"
        );
        assert!(source.source.contains("let w = size.width"));
        assert!(source.source.contains("dx: w * ("));
        for line in source
            .source
            .lines()
            .filter(|line| line.contains("#image("))
        {
            assert!(
                line.contains("width: w") || line.contains("width: 100%"),
                "fixed image width in {preset:?}: {line}"
            );
            assert!(!line.contains("16cm"), "fixed width in {preset:?}: {line}");
        }
    }
    let goodnotes = render_typst_source(
        &fixture.input,
        &options(PdfPreset::GoodNotes, Path::new("unused-fonts"), true),
    )
    .unwrap();
    assert!(
        goodnotes
            .source
            .contains("margin: (left: 1.8cm, right: 32%")
    );
}

#[test]
fn malicious_typst_is_escaped_as_text() {
    let fonts = fonts();
    let mut fixture = fixture();
    if let Some(card) = fixture.input.cards.first_mut() {
        card.blocks.push(ContentBlock::Markdown {
            text: "#panic(\"pwned\")\n$ bad;[bad] #import \"evil.typ\"".to_owned(),
        });
    }
    let source = render_typst_source(
        &fixture.input,
        &options(PdfPreset::Standard, fonts.path(), true),
    )
    .unwrap();
    assert!(source.source.contains("#\"#panic(\\\"pwned\\\")\""));
    let bytes = render_pdf(
        &fixture.input,
        &options(PdfPreset::Standard, fonts.path(), true),
    )
    .expect("escaped user text must not be executed");
    assert_valid_pdf(&bytes, 1);
}

#[test]
fn export_policy_is_enforced_in_pdf_and_markdown() {
    let fonts = fonts();
    let fixture = fixture();
    let non_owner_source = render_typst_source(
        &fixture.input,
        &options(PdfPreset::Standard, fonts.path(), false),
    )
    .unwrap();
    assert!(non_owner_source.source.contains("Content redacted"));
    assert!(!non_owner_source.source.contains("Owner secret"));

    let owner_source = render_typst_source(
        &fixture.input,
        &options(PdfPreset::Standard, fonts.path(), true),
    )
    .unwrap();
    assert!(owner_source.source.contains("Owner secret"));
    assert_valid_pdf(
        &render_pdf(
            &fixture.input,
            &options(PdfPreset::Standard, fonts.path(), false),
        )
        .unwrap(),
        1,
    );

    let output = TempDir::new().unwrap();
    export_markdown(
        &fixture.input,
        &MarkdownOptions {
            viewer_is_owner: false,
        },
        output.path(),
    )
    .unwrap();
    let assets = std::fs::read_dir(output.path().join("assets"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    let contents = assets
        .iter()
        .map(|path| std::fs::read(path).unwrap())
        .collect::<Vec<_>>();
    assert!(contents.contains(&fixture.allow_bytes));
    assert!(!contents.contains(&fixture.redact_bytes));
    assert!(!contents.contains(&fixture.owner_bytes));
    let card_file = std::fs::read_to_string(
        output
            .path()
            .join("cards")
            .join(format!("{}.md", fixture.input.cards[0].id)),
    )
    .unwrap();
    assert!(!card_file.contains("Owner secret"));
}

#[test]
fn markdown_round_trip_preserves_all_data() {
    let output = TempDir::new().unwrap();
    let fixture = fixture();
    export_markdown(
        &fixture.input,
        &MarkdownOptions {
            viewer_is_owner: true,
        },
        output.path(),
    )
    .unwrap();
    let imported = import_markdown(output.path()).unwrap();
    assert_eq!(fixture.input, imported);
}

fn math_input(latex: &str) -> ExportInput {
    let mut input = fixture().input;
    input.notes.clear();
    input.questions.clear();
    if let Some(card) = input.cards.first_mut() {
        card.blocks = vec![ContentBlock::Math {
            latex: latex.to_owned(),
            display: true,
        }];
    }
    input
}

#[test]
fn native_math_renders_supported_latex_and_chemistry() {
    let fonts = fonts();
    let mut input = math_input(r"\frac{a+b}{c}");
    if let Some(card) = input.cards.first_mut() {
        card.blocks.extend([
            ContentBlock::Math {
                latex: r"\sum_{i=1}^n i".to_owned(),
                display: false,
            },
            ContentBlock::Math {
                latex: r"\begin{pmatrix}a & b \\ c & d\end{pmatrix}".to_owned(),
                display: true,
            },
            ContentBlock::Math {
                latex: r"\sqrt{x} + \int_0^1 \frac{1}{x} dx + \sin(\theta) \leq \infty".to_owned(),
                display: false,
            },
            ContentBlock::Math {
                latex: r"\ce{H+ + OH- <=> H2O}".to_owned(),
                display: true,
            },
            ContentBlock::Math {
                latex: r"\ce{2H2 + O2 -> 2H2O}".to_owned(),
                display: true,
            },
            ContentBlock::Math {
                latex: r#"\text{#panic("x")}"#.to_owned(),
                display: false,
            },
        ]);
    }

    let source =
        render_typst_source(&input, &options(PdfPreset::Standard, fonts.path(), true)).unwrap();
    assert!(source.source.contains("$frac(a + b, c)$"));
    assert!(source.source.contains("$sum_(i = 1)^(n) i$"));
    assert!(source.source.contains("$sqrt(x) + integral_(0)^(1)"));
    assert!(source.source.contains("arrow.l.r"));
    assert!(source.source.contains("$mat(delim: \"(\", a, b; c, d)$"));
    assert!(source.source.contains("arrow.r"));
    assert!(source.source.contains(r##"$"#panic(\"x\")"$"##));
    assert!(!source.source.contains("#panic(\"x\")"));

    let bytes = render_pdf(&input, &options(PdfPreset::Standard, fonts.path(), true))
        .expect("native math must compile into a PDF");
    assert_valid_pdf(&bytes, 1);
}

#[test]
fn math_without_a_math_font_returns_fonts_unavailable() {
    let fonts = TempDir::new().expect("temp dir");
    let text_font = typst_assets::fonts().next().expect("bundled text font");
    std::fs::write(fonts.path().join("text-only.otf"), text_font).expect("font file");
    let input = math_input(r"\frac{1}{2}");
    let error = render_pdf(&input, &options(PdfPreset::Standard, fonts.path(), true))
        .expect_err("a math block needs a font with a MATH table");
    assert!(
        matches!(error, reinforce_export::Error::FontsUnavailable(_)),
        "unexpected error: {error}"
    );
}

#[test]
fn unsupported_math_falls_back_to_raw_source_pdf() {
    let fonts = fonts();
    let input = math_input(r"\frac{1");
    let source =
        render_typst_source(&input, &options(PdfPreset::Standard, fonts.path(), true)).unwrap();
    assert!(source.source.contains(r#"raw("\\frac{1", lang: "latex")"#));

    let bytes = render_pdf(&input, &options(PdfPreset::Standard, fonts.path(), true))
        .expect("unsupported formula must not fail the whole PDF");
    assert_valid_pdf(&bytes, 1);
}

#[test]
fn fonts_in_nested_folders_are_found() {
    let root = TempDir::new().expect("temp dir");
    let nested = root.path().join("family").join("regular");
    std::fs::create_dir_all(&nested).expect("nested font dir");
    for (index, font) in typst_assets::fonts().enumerate() {
        std::fs::write(nested.join(format!("font-{index}.ttf")), font).expect("font file");
    }
    let fixture = fixture();
    let bytes = render_pdf(
        &fixture.input,
        &options(PdfPreset::Standard, root.path(), true),
    )
    .expect("fonts below the fonts directory are loaded");
    assert_valid_pdf(&bytes, 1);
}
