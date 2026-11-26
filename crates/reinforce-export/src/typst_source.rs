use std::collections::HashMap;

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use reinforce_core::content::{ContentBlock, DiagramNodeBinding, OcclusionMask, OutlineNode, Pin};
use reinforce_core::primitives::ResourceKind;
use reinforce_core::{KnowledgeCard, KnowledgeCardId, Note, QuestionCard, Resource, ResourceId};

use crate::model::{
    ResourceTreatment, Result, asset_extension, resource_treatment, visible_blocks,
};

/// Generated Typst source plus binary files resolved through the engine's file resolver.
#[derive(Debug, Clone, PartialEq)]
pub struct TypstDocument {
    pub source: String,
    pub binaries: Vec<(String, Vec<u8>)>,
}

/// Escape arbitrary user text as a Typst string literal.
///
/// Every user-supplied string goes through this function before reaching the
/// Typst source, so `#`, `$`, `[`, `]` and backslashes can never be parsed as code.
pub fn escape_typst_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\u{8}"),
            '\u{0c}' => out.push_str("\\u{c}"),
            other if other < '\u{20}' => {
                out.push_str(&format!("\\u{{{:x}}}", other as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

fn text_expr(value: &str) -> String {
    format!("#{}", escape_typst_string(value))
}

fn number(value: f64) -> String {
    format!("{value:.6}")
}

/// Convert Markdown events to Typst markup; text is always emitted as a string literal.
fn inline_markup<'a>(events: &[Event<'a>], index: &mut usize) -> String {
    let mut out = String::new();
    while let Some(event) = events.get(*index) {
        match event {
            Event::End(_) => break,
            Event::Text(value) => {
                out.push_str(&text_expr(value));
                *index += 1;
            }
            Event::Code(value) => {
                out.push_str(&format!("#raw({})", escape_typst_string(value)));
                *index += 1;
            }
            Event::Html(value) | Event::InlineHtml(value) => {
                out.push_str(&text_expr(value));
                *index += 1;
            }
            Event::SoftBreak | Event::HardBreak => {
                out.push(' ');
                *index += 1;
            }
            Event::TaskListMarker(checked) => {
                out.push_str(&text_expr(if *checked { "[x] " } else { "[ ] " }));
                *index += 1;
            }
            Event::FootnoteReference(value) => {
                out.push_str(&text_expr(&format!("[^{value}]")));
                *index += 1;
            }
            Event::Start(tag) => {
                let end = tag.to_end();
                *index += 1;
                let inner = inline_markup(events, index);
                if matches!(events.get(*index), Some(Event::End(actual)) if *actual == end) {
                    *index += 1;
                }
                match tag {
                    Tag::Strong => out.push_str(&format!("#strong([{inner}])")),
                    Tag::Emphasis => out.push_str(&format!("#emph([{inner}])")),
                    Tag::Strikethrough => out.push_str(&format!("#strike([{inner}])")),
                    Tag::Superscript => out.push_str(&format!("#super([{inner}])")),
                    Tag::Subscript => out.push_str(&format!("#sub([{inner}])")),
                    Tag::Link { dest_url, .. } => out.push_str(&format!(
                        "#link({}, [{inner}])",
                        escape_typst_string(dest_url)
                    )),
                    Tag::Image { dest_url, .. } => {
                        out.push_str(&text_expr(&format!("[image: {dest_url}]")));
                    }
                    _ => out.push_str(&inner),
                }
            }
            _ => {
                *index += 1;
            }
        }
    }
    out
}

/// Convert Markdown to a compact Typst fragment with repeating table headers.
pub fn markdown_to_typst(markdown: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    let events: Vec<Event<'_>> = Parser::new_ext(markdown, options).collect();
    let mut out = String::new();
    let mut index = 0;
    let mut list_depth = 0usize;
    let mut table_cells: Vec<String> = Vec::new();
    let mut table_rows: Vec<Vec<String>> = Vec::new();
    let mut table_header: Option<Vec<String>> = None;
    let mut in_table_head = false;

    while let Some(event) = events.get(index) {
        match event {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => {
                    let end = tag.to_end();
                    index += 1;
                    let inner = inline_markup(&events, &mut index);
                    if matches!(events.get(index), Some(Event::End(actual)) if *actual == end) {
                        index += 1;
                    }
                    let typst_level = match level {
                        HeadingLevel::H1 => 2,
                        HeadingLevel::H2 => 3,
                        HeadingLevel::H3 => 4,
                        HeadingLevel::H4 => 5,
                        HeadingLevel::H5 | HeadingLevel::H6 => 6,
                    };
                    out.push_str(&format!("#heading(level: {typst_level}, [{inner}])\n"));
                }
                Tag::Paragraph => {
                    index += 1;
                    let inner = inline_markup(&events, &mut index);
                    if matches!(events.get(index), Some(Event::End(TagEnd::Paragraph))) {
                        index += 1;
                    }
                    out.push_str(&inner);
                    out.push_str("\n\n");
                }
                Tag::CodeBlock(kind) => {
                    let language = match kind {
                        pulldown_cmark::CodeBlockKind::Indented => String::new(),
                        pulldown_cmark::CodeBlockKind::Fenced(value) => value.to_string(),
                    };
                    index += 1;
                    let mut code = String::new();
                    while let Some(Event::Text(value)) = events.get(index) {
                        code.push_str(value);
                        index += 1;
                    }
                    if matches!(events.get(index), Some(Event::End(TagEnd::CodeBlock))) {
                        index += 1;
                    }
                    let lang = if language.is_empty() {
                        String::new()
                    } else {
                        format!(", lang: {}", escape_typst_string(&language))
                    };
                    out.push_str(&format!(
                        "#block(width: 100%, fill: rgb(245, 245, 245), inset: 6pt, raw({}, {lang}))\n\n",
                        escape_typst_string(&code)
                    ));
                }
                Tag::BlockQuote(_) => {
                    let end = tag.to_end();
                    index += 1;
                    let inner = inline_markup(&events, &mut index);
                    if matches!(events.get(index), Some(Event::End(actual)) if *actual == end) {
                        index += 1;
                    }
                    out.push_str(&format!(
                        "#block(width: 100%, fill: rgb(248, 248, 248), inset: 6pt, [{inner}])\n\n"
                    ));
                }
                Tag::List(_) => {
                    list_depth += 1;
                    index += 1;
                }
                Tag::Item => {
                    index += 1;
                    let inner = inline_markup(&events, &mut index);
                    if matches!(events.get(index), Some(Event::End(TagEnd::Item))) {
                        index += 1;
                    }
                    out.push_str(&"  ".repeat(list_depth.saturating_sub(1)));
                    out.push_str(&format!("- {inner}\n"));
                }
                Tag::Table(_) => {
                    table_cells.clear();
                    table_rows.clear();
                    table_header = None;
                    in_table_head = false;
                    index += 1;
                }
                Tag::TableHead => {
                    in_table_head = true;
                    index += 1;
                }
                Tag::TableRow => {
                    table_cells.clear();
                    index += 1;
                }
                Tag::TableCell => {
                    index += 1;
                    let inner = inline_markup(&events, &mut index);
                    if matches!(events.get(index), Some(Event::End(TagEnd::TableCell))) {
                        index += 1;
                    }
                    table_cells.push(inner);
                }
                _ => index += 1,
            },
            Event::End(tag_end) => match tag_end {
                TagEnd::List(_) => {
                    list_depth = list_depth.saturating_sub(1);
                    out.push('\n');
                    index += 1;
                }
                TagEnd::TableHead => {
                    table_header = Some(std::mem::take(&mut table_cells));
                    in_table_head = false;
                    index += 1;
                }
                TagEnd::TableRow => {
                    if !in_table_head {
                        table_rows.push(std::mem::take(&mut table_cells));
                    }
                    index += 1;
                }
                TagEnd::Table => {
                    index += 1;
                    let columns = table_header
                        .as_ref()
                        .map(Vec::len)
                        .or_else(|| table_rows.first().map(Vec::len))
                        .unwrap_or(1);
                    out.push_str(&format!("#table(columns: {columns},\n"));
                    if let Some(header) = &table_header {
                        let cells = header
                            .iter()
                            .map(|cell| format!("[{cell}]"))
                            .collect::<Vec<_>>()
                            .join(", ");
                        out.push_str(&format!("  table.header(repeat: true, {cells}),\n"));
                    }
                    for row in &table_rows {
                        let cells = row
                            .iter()
                            .map(|cell| format!("[{cell}]"))
                            .collect::<Vec<_>>()
                            .join(", ");
                        out.push_str(&format!("  {cells},\n"));
                    }
                    out.push_str(")\n\n");
                    table_header = None;
                    table_rows.clear();
                }
                _ => index += 1,
            },
            Event::Rule => {
                out.push_str("#line(length: 100%)\n\n");
                index += 1;
            }
            _ => index += 1,
        }
    }
    out
}

fn resource_image(
    input: &crate::ExportInput,
    resource: &Resource,
    binaries: &mut Vec<(String, Vec<u8>)>,
) -> Result<String> {
    if resource.kind != ResourceKind::Image {
        return Ok(String::new());
    }
    let Some(bytes) = input.blob(&resource.sha256)? else {
        return Err(crate::Error::MissingBlob(resource.sha256.to_string()));
    };
    let path = format!("assets/{}.{}", resource.sha256, asset_extension(&bytes));
    binaries.push((path.clone(), bytes));
    Ok(path)
}

fn dimensions(resource: &Resource) -> (f64, f64) {
    let width = 16.0;
    let height = match (resource.width, resource.height) {
        (Some(width_px), Some(height_px)) if width_px > 0 && height_px > 0 => {
            width * height_px as f64 / width_px as f64
        }
        _ => 10.0,
    };
    (width, height)
}

fn pinned_diagram(
    resource: &Resource,
    image_path: &str,
    pins: &[Pin],
    worksheet: bool,
    output: &mut String,
) {
    let (width, height) = dimensions(resource);
    output.push_str(&format!(
        "#block(width: {width}cm, height: {height}cm, fill: white, stroke: 0.5pt + gray, [\n"
    ));
    output.push_str(&format!(
        "#image({}, width: {width}cm, height: {height}cm)\n",
        escape_typst_string(image_path)
    ));
    for pin in pins {
        let start_x = pin.x.clamp(0.0, 1.0) * width;
        let start_y = pin.y.clamp(0.0, 1.0) * height;
        let end_x = pin.label_x.clamp(0.0, 1.0) * width;
        let end_y = pin.label_y.clamp(0.0, 1.0) * height;
        output.push_str(&format!(
            "#place(top + left, dx: 0cm, dy: 0cm, line(start: ({}cm, {}cm), end: ({}cm, {}cm), stroke: 0.7pt + blue))\n",
            number(start_x),
            number(start_y),
            number(end_x),
            number(end_y)
        ));
        let label = if worksheet {
            pin.no.to_string()
        } else {
            pin.term_primary.clone()
        };
        output.push_str(&format!(
            "#place(top + left, dx: {}cm, dy: {}cm, [#text(size: 9pt, fill: blue, {})])\n",
            number(end_x),
            number(end_y),
            escape_typst_string(&label)
        ));
    }
    output.push_str("])\n\n");
}

fn answer_table(pins: &[Pin], output: &mut String) {
    output.push_str("#pagebreak()\n");
    output.push_str("#heading(level: 2, \"Answer key\")\n");
    output.push_str("#table(columns: 4,\n");
    output.push_str("  table.header(repeat: true, [No.], [Primary], [Secondary], [Hint]),\n");
    for pin in pins {
        let cell = |value: &str| format!("[{}]", text_expr(value));
        output.push_str(&format!(
            "  {}, {}, {}, {},\n",
            cell(&pin.no.to_string()),
            cell(&pin.term_primary),
            cell(pin.term_secondary.as_deref().unwrap_or("")),
            cell(pin.hint.as_deref().unwrap_or(""))
        ));
    }
    output.push_str(")\n\n");
}

fn render_image_occlusion(
    input: &crate::ExportInput,
    resource_id: &ResourceId,
    masks: &[OcclusionMask],
    options: &crate::PdfOptions,
    binaries: &mut Vec<(String, Vec<u8>)>,
    output: &mut String,
) -> Result<()> {
    match resource_treatment(input, resource_id, options.viewer_is_owner) {
        ResourceTreatment::Skip => Ok(()),
        ResourceTreatment::Redacted(resource) => {
            output.push_str(&format!(
                "#block(width: 100%, fill: rgb(225, 225, 225), inset: 10pt, [#text(fill: gray, {})])\n\n",
                escape_typst_string(&format!(
                    "Restricted resource: {}. Content redacted for this viewer.",
                    resource.title
                ))
            ));
            Ok(())
        }
        ResourceTreatment::Include(resource) => {
            let path = resource_image(input, resource, binaries)?;
            if path.is_empty() {
                return Ok(());
            }
            let (width, height) = dimensions(resource);
            output.push_str(&format!(
                "#block(width: {width}cm, height: {height}cm, [\n#image({}, width: {width}cm, height: {height}cm)\n",
                escape_typst_string(&path)
            ));
            for mask in masks {
                let x = mask.rect.x.clamp(0.0, 1.0) * width;
                let y = mask.rect.y.clamp(0.0, 1.0) * height;
                let w = mask.rect.w.clamp(0.0, 1.0) * width;
                let h = mask.rect.h.clamp(0.0, 1.0) * height;
                output.push_str(&format!(
                    "#place(top + left, dx: {}cm, dy: {}cm, rect(width: {}cm, height: {}cm, fill: rgb(30, 30, 30)))\n",
                    number(x),
                    number(y),
                    number(w),
                    number(h)
                ));
            }
            output.push_str("])\n\n");
            Ok(())
        }
    }
}

fn render_interactive_pins(
    input: &crate::ExportInput,
    resource_id: &ResourceId,
    overlay_resource_id: Option<&ResourceId>,
    pins: &[Pin],
    options: &crate::PdfOptions,
    binaries: &mut Vec<(String, Vec<u8>)>,
    output: &mut String,
) -> Result<()> {
    let base = match resource_treatment(input, resource_id, options.viewer_is_owner) {
        ResourceTreatment::Skip => return Ok(()),
        ResourceTreatment::Redacted(resource) => {
            output.push_str(&format!(
                "#block(width: 100%, fill: rgb(225, 225, 225), inset: 10pt, [#text(fill: gray, {})])\n\n",
                escape_typst_string(&format!(
                    "Restricted resource: {}. Content redacted for this viewer.",
                    resource.title
                ))
            ));
            return Ok(());
        }
        ResourceTreatment::Include(resource) => resource,
    };
    if let Some(id) = overlay_resource_id {
        if matches!(
            resource_treatment(input, id, options.viewer_is_owner),
            ResourceTreatment::Skip | ResourceTreatment::Redacted(_)
        ) {
            return Ok(());
        }
    }

    let path = resource_image(input, base, binaries)?;
    if path.is_empty() {
        return Ok(());
    }
    let worksheet = options.preset == crate::PdfPreset::Worksheet;
    pinned_diagram(base, &path, pins, worksheet, output);
    if worksheet {
        output.push_str("#heading(level: 3, \"Answer lines\")\n");
        for pin in pins {
            output.push_str(&format!(
                "#text(\"{}. ______________________________________________\")\n",
                pin.no
            ));
        }
        output.push('\n');
        answer_table(pins, output);
    }
    Ok(())
}

fn diagram_binding_rows(bindings: &[DiagramNodeBinding]) -> String {
    bindings
        .iter()
        .map(|binding| {
            format!(
                "[{}, {}]",
                text_expr(&binding.node_id),
                text_expr(&binding.card_id.to_string())
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_blocks(
    input: &crate::ExportInput,
    blocks: &[ContentBlock],
    options: &crate::PdfOptions,
    binaries: &mut Vec<(String, Vec<u8>)>,
    output: &mut String,
) -> Result<()> {
    let visible = visible_blocks(input, blocks, options.viewer_is_owner);
    for (index, block) in visible.iter().enumerate() {
        match block {
            ContentBlock::Markdown { text } => output.push_str(&markdown_to_typst(text)),
            ContentBlock::Math { latex, display } => {
                if *display {
                    output.push_str("#align(center, ");
                }
                output.push_str(&format!(
                    "block(width: 100%, fill: rgb(250, 250, 250), inset: 4pt, raw({}, lang: \"latex\"))",
                    escape_typst_string(latex)
                ));
                if *display {
                    output.push(')');
                }
                output.push_str("\n\n");
            }
            ContentBlock::Diagram {
                source,
                syntax,
                svg_cache,
                node_bindings,
            } => {
                if let Some(svg) = svg_cache {
                    let path = format!("diagrams/{index}.svg");
                    binaries.push((path.clone(), svg.as_bytes().to_vec()));
                    output.push_str(&format!(
                        "#image({}, width: 100%)\n\n",
                        escape_typst_string(&path)
                    ));
                } else {
                    output.push_str(&format!(
                        "#block(width: 100%, fill: rgb(250, 250, 250), inset: 4pt, raw({}, lang: {}))\n\n",
                        escape_typst_string(source),
                        escape_typst_string(syntax.as_str())
                    ));
                }
                if !node_bindings.is_empty() {
                    output.push_str(&format!(
                        "#text(size: 8pt, fill: gray, [Node bindings: {}])\n\n",
                        diagram_binding_rows(node_bindings)
                    ));
                }
            }
            ContentBlock::ImageOcclusion { resource_id, masks } => {
                render_image_occlusion(input, resource_id, masks, options, binaries, output)?
            }
            ContentBlock::InteractivePins {
                resource_id,
                overlay_resource_id,
                pins,
            } => render_interactive_pins(
                input,
                resource_id,
                overlay_resource_id.as_ref(),
                pins,
                options,
                binaries,
                output,
            )?,
        }
    }
    Ok(())
}

fn deep_links(card_id: &str, web_base_url: Option<&str>, output: &mut String) {
    let native = format!("reinforce://card/{card_id}");
    output.push_str(&format!(
        "#text(size: 8pt, fill: gray, [#link({}, [deep link])])",
        escape_typst_string(&native)
    ));
    if let Some(base) = web_base_url {
        let web = format!("{}/cards/{}", base.trim_end_matches('/'), card_id);
        output.push_str("  ");
        output.push_str(&format!(
            "#text(size: 8pt, fill: gray, [#link({}, [web])])",
            escape_typst_string(&web)
        ));
    }
    output.push('\n');
}

fn render_note(
    input: &crate::ExportInput,
    note: &Note,
    cards: &HashMap<&KnowledgeCardId, &KnowledgeCard>,
    options: &crate::PdfOptions,
    binaries: &mut Vec<(String, Vec<u8>)>,
    output: &mut String,
) -> Result<()> {
    output.push_str(&format!(
        "#heading(level: 1, {})\n",
        escape_typst_string(&note.title)
    ));
    output.push_str(&format!(
        "#text(size: 8pt, fill: gray, {})\n\n",
        escape_typst_string(&format!("{} · {}", note.subject, note.id))
    ));
    fn walk(
        nodes: &[OutlineNode],
        level: u8,
        input: &crate::ExportInput,
        cards: &HashMap<&KnowledgeCardId, &KnowledgeCard>,
        options: &crate::PdfOptions,
        binaries: &mut Vec<(String, Vec<u8>)>,
        output: &mut String,
    ) -> Result<()> {
        for node in nodes {
            match node {
                OutlineNode::Section {
                    heading,
                    intro_markdown,
                    children,
                } => {
                    output.push_str(&format!(
                        "#heading(level: {}, {})\n",
                        level.min(6),
                        escape_typst_string(heading)
                    ));
                    if let Some(intro) = intro_markdown {
                        output.push_str(&markdown_to_typst(intro));
                    }
                    walk(children, level + 1, input, cards, options, binaries, output)?;
                }
                OutlineNode::CardRef { card_id } => {
                    if let Some(card) = cards.get(card_id) {
                        render_card(input, card, 3, options, binaries, output)?;
                    } else {
                        output.push_str(&format!(
                            "#text(size: 9pt, fill: gray, {})\n\n",
                            escape_typst_string(&format!("Referenced card: {card_id}"))
                        ));
                        deep_links(
                            &card_id.to_string(),
                            options.web_base_url.as_deref(),
                            output,
                        );
                    }
                }
            }
        }
        Ok(())
    }
    walk(&note.outline, 2, input, cards, options, binaries, output)
}

fn render_card(
    input: &crate::ExportInput,
    card: &KnowledgeCard,
    level: u8,
    options: &crate::PdfOptions,
    binaries: &mut Vec<(String, Vec<u8>)>,
    output: &mut String,
) -> Result<()> {
    output.push_str(&format!(
        "#heading(level: {}, {})\n",
        level.min(6),
        escape_typst_string(&card.title)
    ));
    deep_links(
        &card.id.to_string(),
        options.web_base_url.as_deref(),
        output,
    );
    output.push_str(&format!(
        "#text(size: 8pt, fill: gray, {})\n\n",
        escape_typst_string(&format!(
            "{} · {} · {} · {}",
            card.subject,
            card.status.as_str(),
            card.verification.as_str(),
            card.topics.join(", ")
        ))
    ));
    render_blocks(input, &card.blocks, options, binaries, output)
}

fn render_question(
    input: &crate::ExportInput,
    question: &QuestionCard,
    options: &crate::PdfOptions,
    binaries: &mut Vec<(String, Vec<u8>)>,
    output: &mut String,
) -> Result<()> {
    let title = question
        .question_no
        .map(|number| format!("Question {number}"))
        .unwrap_or_else(|| "Question".to_owned());
    output.push_str(&format!(
        "#heading(level: 1, {})\n",
        escape_typst_string(&title)
    ));
    deep_links(
        &question.id.to_string(),
        options.web_base_url.as_deref(),
        output,
    );
    output.push_str(&format!(
        "#text(size: 8pt, fill: gray, {})\n\n",
        escape_typst_string(&format!(
            "{} · {} · {}",
            question.subject,
            question.question_type.as_str(),
            question.verification.as_str()
        ))
    ));
    output.push_str("#heading(level: 2, \"Stem\")\n");
    render_blocks(input, &question.stem_blocks, options, binaries, output)?;
    if !question.options.is_empty() {
        output.push_str("#heading(level: 2, \"Options\")\n");
        for option in &question.options {
            output.push_str(&format!(
                "#text({})\n",
                escape_typst_string(&format!("{}. {}", option.key, option.content))
            ));
        }
        output.push('\n');
    }
    output.push_str("#heading(level: 2, \"Explanation\")\n");
    render_blocks(
        input,
        &question.explanation_blocks,
        options,
        binaries,
        output,
    )
}

/// Build Typst source for an export input without running the compiler.
pub fn render_typst_source(
    input: &crate::ExportInput,
    options: &crate::PdfOptions,
) -> Result<TypstDocument> {
    let mut output = String::new();
    let mut binaries = Vec::new();
    match options.preset {
        crate::PdfPreset::Standard => {
            output.push_str("#set page(paper: \"a4\", margin: 1.8cm)\n");
            output.push_str("#set par(leading: 0.65em)\n");
        }
        crate::PdfPreset::GoodNotes => {
            output.push_str("#set page(paper: \"a4\", margin: (left: 1.8cm, right: 32%, top: 1.8cm, bottom: 1.8cm))\n");
            output.push_str("#set par(leading: 1.15em)\n");
        }
        crate::PdfPreset::Worksheet => {
            output.push_str("#set page(paper: \"a4\", margin: (x: 1.5cm, y: 1.5cm))\n");
            output.push_str("#set par(leading: 0.85em)\n");
        }
    }
    output.push_str(
        "#set text(size: 10pt, font: (\"New Computer Modern\", \"Noto Sans CJK SC\"))\n\n",
    );

    let cards: HashMap<&KnowledgeCardId, &KnowledgeCard> =
        input.cards.iter().map(|card| (&card.id, card)).collect();

    for note in &input.notes {
        render_note(input, note, &cards, options, &mut binaries, &mut output)?;
    }
    for card in &input.cards {
        render_card(input, card, 1, options, &mut binaries, &mut output)?;
    }
    for question in &input.questions {
        render_question(input, question, options, &mut binaries, &mut output)?;
    }
    if options.preset == crate::PdfPreset::Worksheet {
        output.push_str("#pagebreak()\n#heading(level: 1, \"Answer key\")\n#text(\"No pinned cards in this export.\")\n");
    }

    Ok(TypstDocument {
        source: output,
        binaries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_text_is_a_string_literal() {
        let escaped = escape_typst_string("#let pwned = panic(\"x\")\n$[bad]");
        assert!(escaped.starts_with('"'));
        assert!(escaped.contains("\\\"x\\\""));
        assert!(escaped.contains("\\n"));
        assert!(escaped.starts_with("\"#let"));
    }

    #[test]
    fn markdown_table_header_repeats() {
        let source = markdown_to_typst("| A | B |\n| --- | --- |\n| 1 | 2 |\n");
        assert!(source.contains("table.header(repeat: true"));
        assert!(source.contains("#\"1\""));
    }

    #[test]
    fn source_contains_both_deep_links() {
        let mut output = String::new();
        deep_links(
            "0199c000-0000-7000-8000-000000000000",
            Some("https://notes.example.test"),
            &mut output,
        );
        assert!(output.contains("reinforce://card/"));
        assert!(output.contains("https://notes.example.test/cards/"));
    }
}
