use std::path::Path;

use typst::syntax::{FileId, RootedPath, VirtualPath, VirtualRoot};
use typst_as_lib::TypstEngine;
use typst_layout::PagedDocument;

use crate::model::{Error, Result, is_font_file};
use crate::typst_source::render_typst_source;
use crate::{ExportInput, PdfOptions};

fn load_fonts(dir: &Path) -> Result<Vec<Vec<u8>>> {
    let mut fonts = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_file() && is_font_file(&path) {
            fonts.push(std::fs::read(path)?);
        }
    }
    if fonts.is_empty() {
        return Err(Error::NoFonts(dir.to_path_buf()));
    }
    fonts.sort();
    Ok(fonts)
}

fn file_id(path: &str) -> FileId {
    let virtual_path = VirtualPath::new(path).expect("generated asset paths are valid");
    RootedPath::new(VirtualRoot::Project, virtual_path).intern()
}

/// Render an export input to a PDF with the embedded Typst engine.
pub fn render_pdf(input: &ExportInput, options: &PdfOptions) -> Result<Vec<u8>> {
    let document = render_typst_source(input, options)?;
    let fonts = load_fonts(&options.fonts_dir)?;

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
    let paged: PagedDocument = engine
        .compile()
        .output
        .map_err(|error| Error::Typst(format!("{error:?}")))?;
    typst_pdf::pdf(&paged, &Default::default()).map_err(|error| Error::Pdf(format!("{error:?}")))
}
