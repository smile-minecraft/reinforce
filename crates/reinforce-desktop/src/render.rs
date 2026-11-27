//! Pre-rendering of math and diagrams to SVG files on disk.
//!
//! GPUI can rasterise SVG efficiently and keeps it crisp at any scale, so every
//! vector in the reading room is converted once and cached by content hash. Math
//! goes through the same embedded Typst engine the PDF exporter uses
//! (`reinforce_export::latex_to_typst` for LaTeX → Typst), then `typst-svg`
//! produces a standalone document. Diagrams already carry an `svg_cache` from the
//! pipeline, and blob-backed figures are copied out of the content-addressed
//! store. Later launches reuse the cache and never run Typst again.

use std::path::{Path, PathBuf};

use reinforce_core::ContentHash;
use reinforce_core::content::ContentBlock;
use typst_as_lib::TypstEngine;
use typst_layout::PagedDocument;

use crate::store::Store;
use crate::{Error, Result};

/// What a figure block resolved to.
///
/// Both SVG and raster blobs are returned as file paths: GPUI's image element
/// decodes `.svg` through `resvg` in full colour, while its `svg()` element only
/// paints a monochrome stencil of the alpha mask.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Figure {
    Image(PathBuf),
    /// A figure that needs an external renderer (Mermaid, TikZ) and cannot be
    /// drawn by the desktop client.
    Unsupported(String),
}

/// Owns the cache directory that also backs GPUI's `AssetSource`.
pub struct Renderer {
    dir: PathBuf,
}

impl Renderer {
    pub fn new(dir: impl Into<PathBuf>) -> Result<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Renders a LaTeX math block to a cached SVG asset.
    ///
    /// `None` means the LaTeX construct has no Typst equivalent, which the
    /// caller shows as the original source instead of a broken image.
    pub fn math(&self, latex: &str, display: bool) -> Result<Option<PathBuf>> {
        let Some(source) = math_source(latex, display) else {
            return Ok(None);
        };
        let key = ContentHash::from_bytes(source.as_bytes());
        let target = self.dir.join(format!("math-{}.svg", key.as_str()));
        if !target.is_file() {
            let svg = compile(&source)?;
            std::fs::write(&target, svg)?;
        }
        Ok(Some(target))
    }

    /// Materialises the SVG of a diagram block.
    pub fn diagram(&self, card_key: &str, index: usize, block: &ContentBlock) -> Result<Figure> {
        let ContentBlock::Diagram {
            source,
            syntax,
            svg_cache,
            ..
        } = block
        else {
            return Ok(Figure::Unsupported("not a diagram".to_owned()));
        };
        let svg = match (svg_cache, syntax) {
            (Some(svg), _) if svg.contains("<svg") => svg.clone(),
            (_, reinforce_core::primitives::DiagramSyntax::Svg) if source.contains("<svg") => {
                source.clone()
            }
            _ => {
                return Ok(Figure::Unsupported(format!(
                    "{} 圖表需要外部渲染器",
                    syntax.as_str()
                )));
            }
        };
        let key = ContentHash::from_bytes(svg.as_bytes());
        let target = self.dir.join(format!(
            "diagram-{}-{index}-{}.svg",
            sanitize(card_key),
            key.as_str()
        ));
        if !target.is_file() {
            std::fs::write(&target, svg)?;
        }
        Ok(Figure::Image(target))
    }

    /// Copies the blob behind a resource into the cache and reports how to draw it.
    ///
    /// SVG blobs are written as-is; raster blobs are returned by path so GPUI can
    /// decode them. Missing bytes are reported as unsupported instead of drawing
    /// a blank frame.
    pub fn resource(
        &self,
        store: &Store,
        resource_id: &reinforce_core::ResourceId,
    ) -> Result<Figure> {
        let Some(path) = store.resource_blob(resource_id)? else {
            return Ok(Figure::Unsupported("找不到原始檔".to_owned()));
        };
        let bytes = std::fs::read(&path)?;
        let hash = ContentHash::from_bytes(&bytes);
        let is_svg = bytes.windows(4).any(|window| window == b"<svg")
            || String::from_utf8_lossy(&bytes[..bytes.len().min(512)]).contains("<svg");
        let extension = if is_svg {
            "svg"
        } else {
            match bytes.first() {
                Some(0xFF) => "jpg",
                Some(0x89) => "png",
                _ => "bin",
            }
        };
        let target = self.dir.join(format!("blob-{}.{extension}", hash.as_str()));
        if !target.is_file() {
            std::fs::write(&target, &bytes)?;
        }
        Ok(Figure::Image(target))
    }
}

/// Builds the Typst document for one math block.
pub fn math_source(latex: &str, display: bool) -> Option<String> {
    let math = reinforce_export::latex_to_typst(latex)?;
    let body = if display {
        format!("$ {math} $")
    } else {
        format!("${math}$")
    };
    Some(format!(
        "#set page(width: auto, height: auto, margin: 6pt, fill: white)\n\
         #set text(size: 11pt, font: (\"New Computer Modern\", \"DejaVu Sans Mono\"))\n\
         {body}\n"
    ))
}

fn compile(source: &str) -> Result<String> {
    let fonts: Vec<Vec<u8>> = typst_assets::fonts().map(<[u8]>::to_vec).collect();
    let engine = TypstEngine::builder()
        .main_file(source.to_owned())
        .fonts(fonts)
        .build();
    let document: PagedDocument = engine
        .compile()
        .output
        .map_err(|error| Error::Render(format!("{error:?}")))?;
    let page = document
        .pages()
        .first()
        .ok_or_else(|| Error::Render("typst produced an empty document".to_owned()))?;
    Ok(typst_svg::svg(
        page,
        &typst_svg::SvgOptions {
            render_bleed: false,
            pretty: false,
        },
    ))
}

fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' {
                character
            } else {
                '-'
            }
        })
        .collect()
}

/// Reads the intrinsic size of an SVG file in points, which is what GPUI uses
/// to preserve the aspect ratio of pre-rendered math.
pub fn svg_size(path: &Path) -> Option<(f32, f32)> {
    let content = std::fs::read_to_string(path).ok()?;
    let head: String = content.chars().take(600).collect();
    Some((attribute(&head, "width")?, attribute(&head, "height")?))
}

fn attribute(content: &str, name: &str) -> Option<f32> {
    let needle = format!("{name}=\"");
    let start = content.find(&needle)? + needle.len();
    let rest = &content[start..];
    let end = rest.find('"')?;
    let value = &rest[..end];
    let number: String = value
        .chars()
        .take_while(|character| character.is_ascii_digit() || *character == '.')
        .collect();
    number.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn math_renders_to_svg_and_is_cached() {
        let dir =
            std::env::temp_dir().join(format!("reinforce-desktop-render-{}", uuid::Uuid::now_v7()));
        let renderer = Renderer::new(&dir).unwrap();
        let path = renderer
            .math("\\frac{dP}{dT}=\\frac{L}{T\\Delta v}", true)
            .unwrap()
            .expect("the demo formula has a Typst equivalent");
        let svg = std::fs::read_to_string(&path).unwrap();
        assert!(svg.contains("<svg"), "expected SVG output, got {svg}");
        assert!(svg.contains("<path"), "math should draw glyph outlines");

        // A second call must hit the cache and leave the file untouched.
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        renderer
            .math("\\frac{dP}{dT}=\\frac{L}{T\\Delta v}", true)
            .unwrap();
        assert_eq!(
            modified,
            std::fs::metadata(&path).unwrap().modified().unwrap()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_latex_is_reported_instead_of_drawn() {
        assert!(math_source("\\begin{unknownenv}", false).is_none());
    }
}
