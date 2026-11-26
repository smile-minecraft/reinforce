use std::path::Path;

use reinforce_core::ContentBlock;
use typst::syntax::{FileId, RootedPath, VirtualPath, VirtualRoot};
use typst_as_lib::TypstEngine;
use typst_layout::PagedDocument;

use crate::model::{Error, Result, is_font_file};
use crate::typst_source::render_typst_source;
use crate::{ExportInput, PdfOptions};

/// Font packages usually ship in nested folders (for example `Noto/CJK/`),
/// so the fonts directory is searched recursively.
fn collect_fonts(dir: &Path, fonts: &mut Vec<Vec<u8>>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_fonts(&path, fonts)?;
        } else if path.is_file() && is_font_file(&path) {
            fonts.push(std::fs::read(path)?);
        }
    }
    Ok(())
}

fn load_fonts(dir: &Path) -> Result<Vec<Vec<u8>>> {
    let mut fonts = Vec::new();
    collect_fonts(dir, &mut fonts)?;
    if fonts.is_empty() {
        return Err(Error::NoFonts(dir.to_path_buf()));
    }
    fonts.sort();
    Ok(fonts)
}

fn has_math_table(font: &[u8]) -> bool {
    ttf_parser::Face::parse(font, 0)
        .ok()
        .is_some_and(|face| face.tables().math.is_some())
}

fn has_math_content(blocks: &[ContentBlock]) -> bool {
    blocks
        .iter()
        .any(|block| matches!(block, ContentBlock::Math { .. }))
}

fn needs_math_font(input: &ExportInput) -> bool {
    input.cards.iter().any(|card| has_math_content(&card.blocks))
        || input.questions.iter().any(|question| {
            has_math_content(&question.stem_blocks)
                || has_math_content(&question.explanation_blocks)
        })
}

fn is_font_error(detail: &str) -> bool {
    let lower = detail.to_ascii_lowercase();
    lower.contains("no font could be found")
        || (lower.contains("font")
            && (lower.contains("not found")
                || lower.contains("missing")
                || lower.contains("could not be found")
                || lower.contains("does not contain")))
}

fn file_id(path: &str) -> FileId {
    let virtual_path = VirtualPath::new(path).expect("generated asset paths are valid");
    RootedPath::new(VirtualRoot::Project, virtual_path).intern()
}

/// Render an export input to a PDF with the embedded Typst engine.
pub fn render_pdf(input: &ExportInput, options: &PdfOptions) -> Result<Vec<u8>> {
    let document = render_typst_source(input, options)?;
    let fonts = load_fonts(&options.fonts_dir)?;
    if needs_math_font(input) && !fonts.iter().any(|font| has_math_table(font)) {
        return Err(Error::FontsUnavailable(
            "the loaded fonts contain no MATH table for the math content in this export"
                .to_owned(),
        ));
    }

    let mut builder = TypstEngine::builder()
        .main_file(document.source)
        .fonts(fonts);
    if !document.binaries.is_empty() {
        builder = builder.with_static_file_resolver(
            document
                .binaries
                .into_iter()
                .map(|(path, bytes)| (file_id(&path), bytes)),
        );
    }
    let engine = builder.build();
    let paged: PagedDocument = engine.compile().output.map_err(|error| {
        let detail = format!("{error:?}");
        if is_font_error(&detail) {
            Error::FontsUnavailable(format!("typst could not use the loaded fonts: {detail}"))
        } else {
            Error::Typst(detail)
        }
    })?;
    typst_pdf::pdf(&paged, &Default::default()).map_err(|error| Error::Pdf(format!("{error:?}")))
}
