//! A small, dependency-light Markdown reader.
//!
//! Card text arrives as CommonMark. The desktop reading room needs the block
//! structure (headings, paragraphs, lists, tables, code) rather than an HTML
//! string, so this module turns the event stream into blocks the GPUI layer can
//! lay out. Inline styling is flattened to text on purpose: the source stays
//! readable and nothing is lost, while the renderer stays a few hundred lines.

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Heading {
        level: u8,
        text: String,
    },
    Paragraph(String),
    List {
        ordered: bool,
        items: Vec<String>,
    },
    Table {
        head: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    Code {
        language: Option<String>,
        text: String,
    },
    Quote(String),
    Rule,
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn push_text(target: &mut String, text: &str) {
    target.push_str(text);
}

/// Parses Markdown into display blocks.
pub fn parse(markdown: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut paragraph = String::new();
    let mut heading: Option<(u8, String)> = None;
    let mut list: Option<ListState> = None;
    let mut quote: Option<String> = None;
    let mut code: Option<(Option<String>, String)> = None;
    let mut table: Option<TableState> = None;

    for event in Parser::new_ext(markdown, Options::ENABLE_TABLES) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                heading = Some((heading_level(level), String::new()));
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some((level, text)) = heading.take() {
                    blocks.push(Block::Heading {
                        level,
                        text: text.trim().to_owned(),
                    });
                }
            }
            Event::Start(Tag::Paragraph) => paragraph.clear(),
            Event::End(TagEnd::Paragraph) => {
                let text = paragraph.trim().to_owned();
                if !text.is_empty() {
                    if let Some(quote) = quote.as_mut() {
                        push_text(quote, &text);
                    } else if let Some(list) = list.as_mut() {
                        push_text(&mut list.current, &text);
                    } else {
                        blocks.push(Block::Paragraph(text));
                    }
                }
                paragraph.clear();
            }
            Event::Start(Tag::List(start)) => {
                list = Some(ListState {
                    ordered: start.is_some(),
                    items: Vec::new(),
                    current: String::new(),
                });
            }
            Event::End(TagEnd::List(_)) => {
                if let Some(list) = list.take() {
                    let items: Vec<String> = list
                        .items
                        .into_iter()
                        .map(|item| item.trim().to_owned())
                        .filter(|item| !item.is_empty())
                        .collect();
                    if !items.is_empty() {
                        blocks.push(Block::List {
                            ordered: list.ordered,
                            items,
                        });
                    }
                }
            }
            Event::Start(Tag::Item) => {
                if let Some(list) = list.as_mut() {
                    list.current.clear();
                }
            }
            Event::End(TagEnd::Item) => {
                if let Some(list) = list.as_mut() {
                    let item = list.current.trim().to_owned();
                    if !item.is_empty() {
                        list.items.push(item);
                    }
                    list.current.clear();
                }
            }
            Event::Start(Tag::BlockQuote(_)) => quote = Some(String::new()),
            Event::End(TagEnd::BlockQuote(_)) => {
                if let Some(text) = quote.take() {
                    blocks.push(Block::Quote(text.trim().to_owned()));
                }
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                let language = match kind {
                    CodeBlockKind::Fenced(info) => {
                        let info = info.trim();
                        (!info.is_empty()).then(|| info.to_owned())
                    }
                    CodeBlockKind::Indented => None,
                };
                code = Some((language, String::new()));
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some((language, text)) = code.take() {
                    blocks.push(Block::Code {
                        language,
                        text: text.trim_end().to_owned(),
                    });
                }
            }
            Event::Start(Tag::Table(_)) => table = Some(TableState::default()),
            Event::End(TagEnd::Table) => {
                if let Some(table) = table.take() {
                    blocks.push(table.into_block());
                }
            }
            Event::Start(Tag::TableHead) => {
                if let Some(table) = table.as_mut() {
                    table.in_head = true;
                    table.row.clear();
                }
            }
            Event::End(TagEnd::TableHead) => {
                if let Some(table) = table.as_mut() {
                    // pulldown-cmark emits header cells without a TableRow.
                    let row = std::mem::take(&mut table.row);
                    if !row.is_empty() {
                        table.head = row;
                    }
                    table.in_head = false;
                }
            }
            Event::Start(Tag::TableRow) => {
                if let Some(table) = table.as_mut() {
                    table.row.clear();
                }
            }
            Event::End(TagEnd::TableRow) => {
                if let Some(table) = table.as_mut() {
                    let row = std::mem::take(&mut table.row);
                    if table.in_head {
                        table.head = row;
                    } else {
                        table.rows.push(row);
                    }
                }
            }
            Event::Start(Tag::TableCell) => {
                if let Some(table) = table.as_mut() {
                    table.cell.clear();
                }
            }
            Event::End(TagEnd::TableCell) => {
                if let Some(table) = table.as_mut() {
                    let cell = table.cell.trim().to_owned();
                    table.row.push(cell);
                    table.cell.clear();
                }
            }
            Event::Text(text) | Event::Code(text) => {
                if let Some((_, text_buffer)) = code.as_mut() {
                    text_buffer.push_str(&text);
                } else if let Some(table) = table.as_mut() {
                    push_text(&mut table.cell, &text);
                } else if let Some((_, text_buffer)) = heading.as_mut() {
                    push_text(text_buffer, &text);
                } else if let Some(list) = list.as_mut() {
                    push_text(&mut list.current, &text);
                } else if let Some(quote) = quote.as_mut() {
                    push_text(quote, &text);
                } else {
                    push_text(&mut paragraph, &text);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some((_, text)) = code.as_mut() {
                    text.push('\n');
                } else if let Some(table) = table.as_mut() {
                    table.cell.push(' ');
                } else if let Some(list) = list.as_mut() {
                    list.current.push(' ');
                } else if let Some(quote) = quote.as_mut() {
                    quote.push(' ');
                } else {
                    paragraph.push(' ');
                }
            }
            Event::Rule => blocks.push(Block::Rule),
            _ => {}
        }
    }

    blocks
}

#[derive(Default)]
struct ListState {
    ordered: bool,
    items: Vec<String>,
    current: String,
}

#[derive(Default)]
struct TableState {
    head: Vec<String>,
    rows: Vec<Vec<String>>,
    row: Vec<String>,
    cell: String,
    in_head: bool,
}

impl TableState {
    fn into_block(self) -> Block {
        Block::Table {
            head: self.head,
            rows: self.rows,
        }
    }
}

/// Splits a Markdown document into a title and body when the first block is a
/// level-one heading, which is how note intros are written.
pub fn split_title(markdown: &str) -> (Option<String>, Vec<Block>) {
    let mut blocks = parse(markdown);
    if let Some(Block::Heading { level: 1, text }) = blocks.first().cloned() {
        blocks.remove(0);
        (Some(text), blocks)
    } else {
        (None, blocks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEMO_TABLE: &str = "| 相變 | 吸放熱 | 常見例子 |\n| --- | --- | --- |\n| 熔化 | 吸熱 | 冰變成水 |\n| 汽化 | 吸熱 | 水變成蒸氣 |\n| 凝華 | 放熱 | 蒸氣直接變成霜 |";

    #[test]
    fn demo_table_becomes_one_table_block() {
        let blocks = parse(DEMO_TABLE);
        assert_eq!(
            blocks.len(),
            1,
            "a single table should not split: {blocks:?}"
        );
        let Block::Table { head, rows } = &blocks[0] else {
            panic!("expected a table, got {:?}", blocks[0]);
        };
        assert_eq!(head, &["相變", "吸放熱", "常見例子"]);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0], vec!["熔化", "吸熱", "冰變成水"]);
        assert_eq!(rows[2], vec!["凝華", "放熱", "蒸氣直接變成霜"]);
    }

    #[test]
    fn paragraphs_lists_and_code_keep_their_shape() {
        let blocks = parse(
            "Para with **bold** and `code`.\n\n- first\n- second\n\n```rust\nfn main() {}\n```",
        );
        assert_eq!(
            blocks[0],
            Block::Paragraph("Para with bold and code.".to_owned())
        );
        assert_eq!(
            blocks[1],
            Block::List {
                ordered: false,
                items: vec!["first".to_owned(), "second".to_owned()],
            }
        );
        assert_eq!(
            blocks[2],
            Block::Code {
                language: Some("rust".to_owned()),
                text: "fn main() {}".to_owned(),
            }
        );
    }
}
